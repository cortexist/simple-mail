//! ai_service.rs — Model-backed mail intelligence.
//!
//! Every *decision* here goes to the decision model when one is configured
//! (TypeSafe Jev protocol, see decision_client.rs) and to the language model
//! otherwise; drafting always needs the language model.
//!
//! - `classify_new_mail` judges new inbox mail as spam / phishing / priority
//!   from its sender, recipients, subject and summary, then applies verdicts
//!   through the existing service/echo layer (junk → `mail_service::move_email`,
//!   priority → flip `is_focused`). A row in `email_ai_classification` is
//!   written per judged email so re-sync never reclassifies, and user manual
//!   overrides (`is_focused_manual`) are preserved.
//! - `judge_replyability` decides whether a reply can reach anyone; see
//!   reply_policy.rs for the pattern it falls back to.
//! - `draft_reply` writes a reply with the language model.
//!
//! The LLM call is factored behind `LlmClient` so the logic can be exercised
//! with a mock in tests; the live path wraps `HttpLlmClient`.

use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use tauri::{AppHandle, Manager};

use std::collections::HashMap;
use futures::StreamExt;

use crate::ai_config::{self, AccountAiFeatures, AiConfig};
use crate::decision_client::{DecisionClient, Question};
use crate::llm_client::{ChatMessage, ChatOptions, HttpLlmClient, LlmClient};
use crate::{events, mail_service, reply_policy};

/// Max emails classified per run. Keeps the batch (and the model's output
/// budget) bounded; remaining unclassified mail is picked up on the next sync.
const BATCH_CAP: i64 = 20;
/// Body/preview characters fed to the model per email.
const SNIPPET_CHARS: usize = 500;
/// Header characters offered per email to the decision model (one email per
/// request) and to the language model (a batch per request).
const HEADER_CHARS_DECISION: usize = 4000;
const HEADER_CHARS_LANGUAGE: usize = 1500;
/// Headers dropped before a model sees the block: signatures and opaque
/// provider blobs. Their verification outcome is already in
/// Authentication-Results; the base64 itself carries no judgment signal and
/// would crowd out the Received chain under the length cap.
const NOISE_HEADERS: &[&str] = &[
    "dkim-signature", "x-google-dkim-signature", "arc-seal", "arc-message-signature",
    "x-gm-message-state", "x-google-smtp-source", "x-gm-gg", "x-gm-features",
    "x-microsoft-antispam-message-info", "x-forefront-antispam-report-untrusted",
    "x-ms-exchange-crosstenant-network-message-id", "x-ms-exchange-antispam-messagedata-0",
    "x-ms-exchange-antispam-messagedata-chunkcount",
];
/// A single header value longer than this is cut: long values are encoded
/// blobs, and the start is all a model can use.
const HEADER_VALUE_CHARS: usize = 300;

/// The header block as a model should read it: unfolded, noise headers
/// dropped, long values cut, the whole capped at `max` characters (the top —
/// most recent Received hops, then the rest in order — survives the cap). For
/// mail synced before headers were kept, fall back to the stored
/// SPF/DKIM/DMARC summary so the model still sees authentication.
fn headers_for_model(raw_headers: &str, auth_results: &str, max: usize) -> String {
    if raw_headers.trim().is_empty() {
        return if auth_results.is_empty() {
            String::new()
        } else {
            format!("Authentication-Results: {auth_results}")
        };
    }
    // Unfold: a line starting with whitespace continues the previous header.
    let mut headers: Vec<String> = Vec::new();
    for line in raw_headers.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = headers.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        } else if !line.is_empty() {
            headers.push(line.to_string());
        }
    }
    let mut out = String::new();
    for h in headers {
        let name = h.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
        if NOISE_HEADERS.contains(&name.as_str()) {
            continue;
        }
        let line = if h.chars().count() > HEADER_VALUE_CHARS {
            format!("{}…", h.chars().take(HEADER_VALUE_CHARS).collect::<String>())
        } else {
            h
        };
        if out.chars().count() + line.chars().count() + 1 > max {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Decision requests in flight at once. Each is one short forward pass; a few
/// in parallel hide network latency without swamping a local server.
const DECISION_CONCURRENCY: usize = 4;
/// Decision-model probability at or above which mail is treated as spam or
/// phishing and moved to Junk. High: a wrong move hides real mail.
const JUNK_P: f64 = 0.85;
/// Decision-model probability at or above which mail is marked priority.
const PRIORITY_P: f64 = 0.7;

/// Fixed system instructions. Kept byte-identical across calls (no per-email
/// interpolation) so server-side prompt-prefix caches can skip its prefill.
const SYSTEM_PROMPT: &str = "\
You are an email triage classifier inside a desktop mail client. For each email \
in the batch decide three booleans:
- is_junk: true only for spam, phishing, scams, or unsolicited bulk marketing. \
Legitimate mail the user likely wants (receipts, subscribed newsletters, \
account notices, real correspondence) is NOT junk.
- is_phishing: true if it tries to obtain credentials, payment, or personal data \
by impersonating a trusted party (look-alike sender domains, urgent account \
threats, login or payment links). Phishing is also junk.
- is_priority: true if it plausibly needs the user's personal attention or a \
timely response — a real person writing to them, a security/account alert, a \
deadline or time-sensitive request. Routine automated, promotional, or \
no-reply mail is NOT priority.
Each email's headers are evidence: SPF/DKIM/DMARC results, the Received chain, \
Return-Path, and List-* headers.
Be conservative: when unsure, use false for all three.
Respond with ONLY a JSON object, no prose, of exactly this shape:
{\"results\":[{\"id\":\"<id>\",\"is_junk\":false,\"is_phishing\":false,\"is_priority\":false,\"reason\":\"<=8 words\"}]}
Return exactly one entry per input email, echoing its id verbatim.";

/// One inbox email awaiting a verdict.
#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    from_name: String,
    from_email: String,
    /// `To` recipients, comma-separated.
    to: String,
    subject: String,
    snippet: String,
    raw_headers: String,
    auth_results: String,
}

/// A model verdict for a single email.
#[derive(Debug, Clone, serde::Deserialize)]
struct Verdict {
    id: String,
    #[serde(default)]
    is_junk: bool,
    #[serde(default)]
    is_phishing: bool,
    #[serde(default)]
    is_priority: bool,
    #[serde(default)]
    reason: String,
}

#[derive(serde::Deserialize)]
struct VerdictBatch {
    #[serde(default)]
    results: Vec<Verdict>,
}

/// Classify newly-synced inbox mail for an account, if AI is enabled and the
/// account opted into junk filtering and/or priority detection. Returns the
/// number of emails for which a verdict was applied. Safe to call after every
/// sync — it no-ops when disabled and skips already-classified mail.
pub async fn classify_new_mail(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
) -> Result<usize, String> {
    let (cfg, features) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let cfg = ai_config::load_ai_config(&conn).map_err(|e| e.to_string())?;
        let features = ai_config::load_account_features(&conn, account_id).map_err(|e| e.to_string())?;
        (cfg, features)
    };

    if !cfg.enabled || !cfg.can_decide() || (!features.junk_filter && !features.priority_detect) {
        return Ok(0);
    }

    let candidates = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        select_candidates(&conn, account_id, BATCH_CAP)?
    };
    if candidates.is_empty() {
        return Ok(0);
    }

    if cfg.has_decision_model() {
        let client = DecisionClient::new(&cfg.decision_base_url, &cfg.decision_api_key, &cfg.decision_model)?;
        return classify_with_decision(db, app, account_id, &client, &cfg, features, candidates).await;
    }
    let client = HttpLlmClient::new(&cfg.base_url, &cfg.api_key)?;
    classify_with(db, app, account_id, &client, &cfg, features, candidates).await
}

#[derive(serde::Serialize)]
struct Party {
    name: String,
    address: String,
}

/// The decision-model state for one email: who sent it, to whom, what it
/// says, then its headers. Named fields so questions can point at them; field
/// order is wire order, and the long headers go last so a model that truncates
/// the state cuts them rather than the sender or subject.
#[derive(serde::Serialize)]
struct ClassificationState {
    sender: Party,
    recipients: String,
    subject: String,
    summary: String,
    headers: String,
}

fn classification_state(c: &Candidate) -> ClassificationState {
    ClassificationState {
        sender: Party { name: c.from_name.clone(), address: c.from_email.clone() },
        recipients: c.to.clone(),
        subject: c.subject.clone(),
        summary: c.snippet.replace('\n', " "),
        headers: headers_for_model(&c.raw_headers, &c.auth_results, HEADER_CHARS_DECISION),
    }
}

/// Replyability state; same ordering rule as [`ClassificationState`].
#[derive(serde::Serialize)]
struct ReplyState {
    reply_address: String,
    sender: Party,
    subject: String,
    summary: String,
    headers: String,
}

/// The three independent triage questions, asked together over one state.
fn classification_questions() -> HashMap<String, Question> {
    HashMap::from([
        ("spam".to_string(), Question::noul(
            "Is this email spam: unsolicited bulk marketing, a scam, or mail the recipient never asked for? \
             Receipts, subscribed newsletters, account notices and real correspondence are not spam. \
             Use `headers` as evidence: authentication results, the Received chain, and List-* headers.",
            "Unsolicited bulk, scam, or unwanted marketing mail.",
            "Mail the recipient plausibly wants: correspondence, receipts, notices, or subscribed content.",
        )),
        ("phishing".to_string(), Question::noul(
            "Is this email a phishing attempt: does `sender` impersonate a trusted party to obtain credentials, \
             payment, or personal data (look-alike domain, urgent account threat, login or payment request)? \
             In `headers`, failing SPF/DKIM/DMARC, a Return-Path or Received chain from an unrelated domain, \
             or a Reply-To elsewhere are strong evidence.",
            "Impersonates a trusted party to extract credentials, money, or personal data.",
            "A genuine message from who `sender` claims to be, or ordinary mail with no such lure.",
        )),
        ("priority".to_string(), Question::noul(
            "Does this email need the recipient's personal attention or a timely response: a real person writing \
             to them, a security or account alert, a deadline, or a time-sensitive request?",
            "Needs the recipient's personal attention or a timely response.",
            "Routine automated, promotional, or informational mail that can wait.",
        )),
    ])
}

/// Ask the same questions about several states, a few requests at a time.
/// Returns each id with its probabilities or its error. The states are owned so
/// the stream holds no borrow of the caller's rows (a borrowed item makes the
/// future not `Send` for spawned callers).
async fn ask_each<S: serde::Serialize + Send + Sync>(
    client: &DecisionClient,
    questions: &HashMap<String, Question>,
    states: Vec<(String, S)>,
) -> Vec<(String, Result<HashMap<String, f64>, String>)> {
    futures::stream::iter(states)
        .map(|(id, state)| async move {
            let r = client.ask_nouls(&state, questions).await;
            (id, r)
        })
        .buffer_unordered(DECISION_CONCURRENCY)
        .collect()
        .await
}

/// Map decision probabilities to a verdict. Pure, for tests.
fn verdict_from_probabilities(id: &str, p: &HashMap<String, f64>) -> Verdict {
    let spam = p.get("spam").copied().unwrap_or(0.0);
    let phishing = p.get("phishing").copied().unwrap_or(0.0);
    let priority = p.get("priority").copied().unwrap_or(0.0);
    let is_phishing = phishing >= JUNK_P;
    Verdict {
        id: id.to_string(),
        is_junk: is_phishing || spam >= JUNK_P,
        is_phishing,
        is_priority: priority >= PRIORITY_P,
        reason: format!("spam {spam:.2} · phishing {phishing:.2} · priority {priority:.2}"),
    }
}

/// Classify with the decision model: one request per email, a few at a time.
/// An email whose request fails is left unjudged and retried on a later sync;
/// the run fails only when every request did.
async fn classify_with_decision(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
    client: &DecisionClient,
    cfg: &AiConfig,
    features: AccountAiFeatures,
    candidates: Vec<Candidate>,
) -> Result<usize, String> {
    let questions = classification_questions();
    let started = std::time::Instant::now();
    let states = candidates.iter().map(|c| (c.id.clone(), classification_state(c))).collect();
    let results = ask_each(client, &questions, states).await;

    let mut verdicts = Vec::new();
    let mut last_err = None;
    for (id, r) in results {
        match r {
            Ok(p) => verdicts.push(verdict_from_probabilities(&id, &p)),
            Err(e) => {
                log::warn!("ai classify [{}]: decision failed for {}: {}", account_id, id, e);
                last_err = Some(e);
            }
        }
    }
    log::info!(
        "ai classify [{}]: decision model={} judged={}/{} latency={}ms",
        account_id, cfg.decision_model, verdicts.len(), candidates.len(), started.elapsed().as_millis()
    );
    if verdicts.is_empty() {
        if let Some(e) = last_err {
            return Err(e);
        }
    }
    apply_verdicts(db, app, account_id, &candidates, &verdicts, &cfg.decision_model, features)
}

/// Core classification flow, generic over the LLM backend for testability.
async fn classify_with(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
    client: &dyn LlmClient,
    cfg: &AiConfig,
    features: AccountAiFeatures,
    candidates: Vec<Candidate>,
) -> Result<usize, String> {
    let messages = build_messages(&candidates);
    let opts = ChatOptions {
        model: cfg.classify_model.clone(),
        // Short structured output: ~one small JSON object per email.
        max_tokens: (candidates.len() as i32 * 64) + 256,
        temperature: cfg.temperature,
        json: true,
    };

    let started = std::time::Instant::now();
    let raw = client.complete(&messages, &opts).await?;
    log::info!(
        "ai classify [{}]: model={} batch={} latency={}ms",
        account_id, cfg.classify_model, candidates.len(), started.elapsed().as_millis()
    );

    let verdicts = parse_verdicts(&raw)?;
    let applied = apply_verdicts(db, app, account_id, &candidates, &verdicts, &cfg.classify_model, features)?;
    Ok(applied)
}

/// Select inbox emails **visible to `account_id`** that have no verdict yet,
/// newest first.
///
/// Scoped to the account's view: classification is a per-account feature, so a
/// mailbox's AI must judge only its own mail — not the whole login's. Without
/// the scope this would also select nothing at all for a mailbox account, since
/// mail is keyed to the login.
fn select_candidates(conn: &Connection, account_id: &str, limit: i64) -> Result<Vec<Candidate>, String> {
    let view = crate::login::view_of(conn, account_id).map_err(|e| e.to_string())?;
    let login_id = view.login.as_str().to_string();
    let snippet = SNIPPET_CHARS as i64;

    // No `emails e` alias: the mailbox predicate qualifies its columns as
    // `emails.*`, which an alias would put out of scope.
    let sql = format!(
        "SELECT id, from_name, from_email, subject,
                substr(COALESCE(NULLIF(preview, ''), body), 1, ?3),
                (SELECT group_concat(CASE WHEN r.name != '' AND r.name != r.email
                                          THEN r.name || ' <' || r.email || '>' ELSE r.email END, ', ')
                 FROM email_recipients r WHERE r.email_id = emails.id AND r.type = 'to'),
                COALESCE(raw_headers, ''), COALESCE(auth_results, '')
         FROM emails
         WHERE account_id = ?1
           AND folder = 'inbox'
           AND NOT EXISTS (
               SELECT 1 FROM email_ai_classification c WHERE c.email_id = emails.id
           ){}
         ORDER BY date DESC
         LIMIT ?2",
        crate::login::mailbox_clause(view.mailbox.as_deref(), 4),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&login_id, &limit, &snippet];
    if let Some(addr) = view.mailbox.as_ref() {
        params.push(addr);
    }
    let rows = stmt.query_map(
        params.as_slice(),
        |row| Ok(Candidate {
            id: row.get(0)?,
            from_name: row.get(1)?,
            from_email: row.get(2)?,
            subject: row.get(3)?,
            snippet: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            to: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            raw_headers: row.get(6)?,
            auth_results: row.get(7)?,
        }),
    ).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Build the chat messages: a fixed system prefix (cache-friendly) followed by
/// the variable per-email batch as the trailing user message.
fn build_messages(candidates: &[Candidate]) -> Vec<ChatMessage> {
    let mut batch = String::from("Classify these emails:\n");
    for c in candidates {
        batch.push_str(&format!(
            "\n--- id: {}\nFrom: {} <{}>\nTo: {}\nSubject: {}\nPreview: {}\nHeaders:\n{}\n",
            c.id,
            c.from_name,
            c.from_email,
            c.to,
            c.subject,
            c.snippet.replace('\n', " "),
            headers_for_model(&c.raw_headers, &c.auth_results, HEADER_CHARS_LANGUAGE),
        ));
    }
    vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(batch)]
}

/// Parse the model's JSON response into verdicts. Tolerates code-fenced output
/// and either the documented `{"results":[...]}` object or a bare `[...]` array.
fn parse_verdicts(raw: &str) -> Result<Vec<Verdict>, String> {
    let trimmed = strip_code_fence(raw.trim());
    if let Ok(batch) = serde_json::from_str::<VerdictBatch>(trimmed) {
        return Ok(batch.results);
    }
    if let Ok(list) = serde_json::from_str::<Vec<Verdict>>(trimmed) {
        return Ok(list);
    }
    Err(format!("could not parse classification response: {trimmed}"))
}

/// Strip a leading/trailing ```json … ``` fence if the model added one.
fn strip_code_fence(s: &str) -> &str {
    let s = s.strip_prefix("```json").or_else(|| s.strip_prefix("```")).unwrap_or(s);
    s.trim().strip_suffix("```").unwrap_or(s).trim()
}

/// What to do with a single verdict, after per-account feature gating. Junk
/// takes precedence over priority (a junk email is moved, not focused).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Move to the Junk folder.
    Junk,
    /// Flip `is_focused` on (subject to the manual-override guard).
    Focus,
    /// Record the verdict only; take no mailbox action.
    RecordOnly,
}

/// Pure decision: map a verdict + the account's enabled features to an action.
/// Kept side-effect-free so it can be unit-tested without a live app/DB.
fn plan_action(v: &Verdict, features: AccountAiFeatures) -> Action {
    if features.junk_filter && (v.is_junk || v.is_phishing) {
        Action::Junk
    } else if features.priority_detect && v.is_priority {
        Action::Focus
    } else {
        Action::RecordOnly
    }
}

/// Flip focus on for a priority email, but never override a user's manual focus
/// choice (`is_focused_manual = 1`) and skip rows already focused. `changes`-row
/// count tells the caller whether to emit an echo.
const FOCUS_UPDATE_SQL: &str =
    "UPDATE emails SET is_focused = 1
     WHERE id = ?1 AND account_id = ?2 AND is_focused_manual = 0 AND is_focused = 0";

/// Apply verdicts that match this run's candidates: record each verdict, move
/// junk to the Junk folder, and flip `is_focused` for priority mail (only when
/// the user hasn't manually set focus). Per-account feature toggles gate which
/// actions run. Returns how many emails had an action applied.
fn apply_verdicts(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
    candidates: &[Candidate],
    verdicts: &[Verdict],
    model: &str,
    features: AccountAiFeatures,
) -> Result<usize, String> {
    let state = app.state::<crate::AppState>();
    let classified_at = chrono::Utc::now().to_rfc3339();
    let mut applied = 0usize;

    for v in verdicts {
        // Only act on ids we actually sent this round (guards against the model
        // inventing or duplicating ids).
        if !candidates.iter().any(|c| c.id == v.id) {
            continue;
        }

        let row_seq = state.next_seq();
        {
            let conn = db.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT OR REPLACE INTO email_ai_classification
                   (email_id, account_id, is_junk, is_phishing, is_priority, reason, model, classified_at, seq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    v.id, account_id, (v.is_junk || v.is_phishing) as i32, v.is_phishing as i32,
                    v.is_priority as i32, v.reason, model, classified_at, row_seq
                ],
            ).map_err(|e| e.to_string())?;
        }

        match plan_action(v, features) {
            Action::Junk => {
                let seq = state.next_seq();
                match mail_service::move_email(db, app, seq, account_id, &v.id, "junk") {
                    Ok(()) => { applied += 1; }
                    Err(e) => log::error!("ai classify: junk move failed for {}: {}", v.id, e),
                }
            }
            Action::Focus => {
                let changed = {
                    let conn = db.lock().map_err(|e| e.to_string())?;
                    conn.execute(FOCUS_UPDATE_SQL, rusqlite::params![v.id, account_id])
                        .map_err(|e| e.to_string())?
                };
                if changed > 0 {
                    let seq = state.next_seq();
                    events::emit_email_flag_changed(app, account_id, &v.id, "focused", true, seq);
                    applied += 1;
                }
            }
            Action::RecordOnly => {}
        }
    }

    Ok(applied)
}

// ── Replyability ────────────────────────────────────────

/// Model probability of "no reply can reach anyone" at or above which the
/// reply buttons are disabled. Deliberately extreme: a wrong "no" hides Reply
/// from a real person, a wrong "yes" only lets a reply bounce. Symmetrically, a
/// model only overrules the address pattern towards "replyable" at or below
/// `1 - NO_REPLY_P`; anything between is an unsure model, and the pattern
/// decides. (An uncalibrated zero-shot model sits near 0.5 on everything —
/// measured on base Laya — and must not undo the pattern.)
const NO_REPLY_P: f64 = 0.95;

/// Final replyability from a model probability and the pattern's verdict.
fn decide_no_reply(p: f64, pattern_no_reply: bool) -> bool {
    if p >= NO_REPLY_P {
        true
    } else if p <= 1.0 - NO_REPLY_P {
        false
    } else {
        pattern_no_reply
    }
}
/// Emails judged per run by each backend. Remaining mail is judged on later
/// syncs; until then the address pattern stands in.
const REPLY_CAP_DECISION: i64 = 50;
const REPLY_CAP_LANGUAGE: i64 = 20;
/// Summary characters offered for the replyability judgment.
const REPLY_SNIPPET_CHARS: usize = 300;

const REPLYABILITY_PROMPT: &str = "\
You decide, for each email, whether a reply sent to its reply address would be \
read by anyone. Answer no_reply=true only when the reply address is an \
automated, unmonitored mailbox: a no-reply / do-not-reply / mailer-daemon \
address, or the email itself says replies are not monitored. Support desks, \
shared team inboxes, and per-message reply addresses DO accept replies. \
confidence is your probability (0 to 1) that no_reply is true.
Respond with ONLY a JSON object, no prose, of exactly this shape:
{\"results\":[{\"id\":\"<id>\",\"no_reply\":false,\"confidence\":0.0}]}
Return exactly one entry per input email, echoing its id verbatim.";

/// One received email awaiting a replyability verdict.
#[derive(Debug, Clone)]
struct ReplyCandidate {
    id: String,
    from_name: String,
    from_email: String,
    reply_to: String,
    subject: String,
    snippet: String,
    raw_headers: String,
    auth_results: String,
}

impl ReplyCandidate {
    fn reply_address(&self) -> &str {
        reply_policy::effective_reply_address(&self.from_email, &self.reply_to)
    }
}

#[derive(serde::Deserialize)]
struct ReplyVerdictLlm {
    id: String,
    #[serde(default)]
    no_reply: bool,
    #[serde(default)]
    confidence: f64,
}

#[derive(serde::Deserialize)]
struct ReplyVerdictBatch {
    #[serde(default)]
    results: Vec<ReplyVerdictLlm>,
}

/// Decide, with a model, whether replies to newly synced mail can reach anyone.
/// No-ops when AI is off, no model is configured, or the account has not opted
/// in — the address pattern then decides at load time. A failed model call
/// leaves the mail unjudged, which also means the pattern decides. Returns how
/// many emails changed replyability relative to the pattern.
pub async fn judge_replyability(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
) -> Result<usize, String> {
    let (cfg, features) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let cfg = ai_config::load_ai_config(&conn).map_err(|e| e.to_string())?;
        let features = ai_config::load_account_features(&conn, account_id).map_err(|e| e.to_string())?;
        (cfg, features)
    };
    if !cfg.enabled || !cfg.can_decide() || !features.reply_detect {
        return Ok(0);
    }
    let decision = cfg.has_decision_model();
    let cap = if decision { REPLY_CAP_DECISION } else { REPLY_CAP_LANGUAGE };
    let candidates = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        select_reply_candidates(&conn, account_id, cap)?
    };
    if candidates.is_empty() {
        return Ok(0);
    }

    let started = std::time::Instant::now();
    // (id, probability that no reply can reach anyone)
    let (probabilities, model) = if decision {
        let client = DecisionClient::new(&cfg.decision_base_url, &cfg.decision_api_key, &cfg.decision_model)?;
        (judge_reply_with_decision(&client, &candidates, account_id).await?, cfg.decision_model.clone())
    } else {
        let client = HttpLlmClient::new(&cfg.base_url, &cfg.api_key)?;
        (judge_reply_with_llm(&client, &cfg, &candidates).await?, cfg.classify_model.clone())
    };
    log::info!(
        "ai replyability [{}]: {} model={} judged={}/{} latency={}ms",
        account_id, if decision { "decision" } else { "language" }, model,
        probabilities.len(), candidates.len(), started.elapsed().as_millis()
    );
    store_reply_verdicts(db, app, account_id, &candidates, &probabilities,
        if decision { "decision" } else { "language" }, &model)
}

/// Received mail in `account_id`'s view with no model verdict yet, newest first.
fn select_reply_candidates(conn: &Connection, account_id: &str, limit: i64) -> Result<Vec<ReplyCandidate>, String> {
    let view = crate::login::view_of(conn, account_id).map_err(|e| e.to_string())?;
    let login_id = view.login.as_str().to_string();
    let snippet = REPLY_SNIPPET_CHARS as i64;
    let sql = format!(
        "SELECT id, from_name, from_email, COALESCE(reply_to, ''), subject,
                substr(COALESCE(NULLIF(preview, ''), body), 1, ?3),
                COALESCE(raw_headers, ''), COALESCE(auth_results, '')
         FROM emails
         WHERE account_id = ?1
           AND folder NOT IN ('sent', 'drafts')
           AND NOT EXISTS (SELECT 1 FROM email_reply_verdict v WHERE v.email_id = emails.id){}
         ORDER BY date DESC
         LIMIT ?2",
        crate::login::mailbox_clause(view.mailbox.as_deref(), 4),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&login_id, &limit, &snippet];
    if let Some(addr) = view.mailbox.as_ref() {
        params.push(addr);
    }
    let rows = stmt.query_map(params.as_slice(), |row| Ok(ReplyCandidate {
        id: row.get(0)?,
        from_name: row.get(1)?,
        from_email: row.get(2)?,
        reply_to: row.get(3)?,
        subject: row.get(4)?,
        snippet: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        raw_headers: row.get(6)?,
        auth_results: row.get(7)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn replyability_question() -> HashMap<String, Question> {
    HashMap::from([("no_reply".to_string(), Question::noul(
        "Would a reply sent to `reply_address` go unread because it is an automated, unmonitored mailbox \
         (a no-reply, do-not-reply, or mailer-daemon address, or the email says replies are not monitored)? \
         Support desks, shared team inboxes, and per-message reply addresses do accept replies. \
         Auto-Submitted, Precedence and List-* in `headers` show automation, not by themselves an unread inbox.",
        "Nobody reads replies sent to `reply_address`.",
        "A person or team could read a reply sent to `reply_address`.",
    ))])
}

async fn judge_reply_with_decision(
    client: &DecisionClient,
    candidates: &[ReplyCandidate],
    account_id: &str,
) -> Result<HashMap<String, f64>, String> {
    let questions = replyability_question();
    let states = candidates
        .iter()
        .map(|c| (c.id.clone(), ReplyState {
            reply_address: c.reply_address().to_string(),
            sender: Party { name: c.from_name.clone(), address: c.from_email.clone() },
            subject: c.subject.clone(),
            summary: c.snippet.replace('\n', " "),
            headers: headers_for_model(&c.raw_headers, &c.auth_results, HEADER_CHARS_DECISION),
        }))
        .collect();
    let results = ask_each(client, &questions, states).await;
    let mut out = HashMap::new();
    let mut last_err = None;
    for (id, r) in results {
        match r.map(|p| p.get("no_reply").copied()) {
            Ok(Some(p)) => { out.insert(id, p); }
            Ok(None) => {}
            Err(e) => {
                log::warn!("ai replyability [{}]: decision failed for {}: {}", account_id, id, e);
                last_err = Some(e);
            }
        }
    }
    match (out.is_empty(), last_err) {
        (true, Some(e)) => Err(e),
        _ => Ok(out),
    }
}

async fn judge_reply_with_llm(
    client: &dyn LlmClient,
    cfg: &AiConfig,
    candidates: &[ReplyCandidate],
) -> Result<HashMap<String, f64>, String> {
    let mut batch = String::from("Judge these emails:\n");
    for c in candidates {
        batch.push_str(&format!(
            "\n--- id: {}\nFrom: {} <{}>\nReply address: {}\nSubject: {}\nSummary: {}\nHeaders:\n{}\n",
            c.id, c.from_name, c.from_email, c.reply_address(), c.subject, c.snippet.replace('\n', " "),
            headers_for_model(&c.raw_headers, &c.auth_results, HEADER_CHARS_LANGUAGE),
        ));
    }
    let messages = [ChatMessage::system(REPLYABILITY_PROMPT), ChatMessage::user(batch)];
    let opts = ChatOptions {
        model: cfg.classify_model.clone(),
        max_tokens: (candidates.len() as i32 * 40) + 128,
        temperature: 0.0,
        json: true,
    };
    let raw = client.complete(&messages, &opts).await?;
    parse_reply_verdicts(&raw, candidates)
}

/// Parse the language model's replyability batch into no-reply probabilities,
/// keeping only ids that were asked about.
fn parse_reply_verdicts(raw: &str, candidates: &[ReplyCandidate]) -> Result<HashMap<String, f64>, String> {
    let trimmed = strip_code_fence(raw.trim());
    let list = serde_json::from_str::<ReplyVerdictBatch>(trimmed)
        .map(|b| b.results)
        .or_else(|_| serde_json::from_str::<Vec<ReplyVerdictLlm>>(trimmed))
        .map_err(|_| format!("could not parse replyability response: {trimmed}"))?;
    Ok(list
        .into_iter()
        .filter(|v| candidates.iter().any(|c| c.id == v.id))
        .map(|v| {
            // `confidence` is documented as P(no_reply); a model that instead
            // reports confidence in its own boolean is read the same way.
            let c = v.confidence.clamp(0.0, 1.0);
            let p = if v.no_reply { c } else { c.min(1.0 - c) };
            (v.id, p)
        })
        .collect())
}

/// Record verdicts and tell the UI about every email whose replyability now
/// differs from what the address pattern — shown until now — said.
fn store_reply_verdicts(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
    candidates: &[ReplyCandidate],
    probabilities: &HashMap<String, f64>,
    source: &str,
    model: &str,
) -> Result<usize, String> {
    let state = app.state::<crate::AppState>();
    let judged_at = chrono::Utc::now().to_rfc3339();
    let mut changed = 0usize;
    for c in candidates {
        let Some(&p) = probabilities.get(&c.id) else { continue };
        let pattern = reply_policy::pattern_says_no_reply(&c.from_email, &c.reply_to);
        let no_reply = decide_no_reply(p, pattern);
        {
            let conn = db.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT OR REPLACE INTO email_reply_verdict
                   (email_id, account_id, no_reply, confidence, source, model, judged_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![c.id, account_id, no_reply as i32, p, source, model, judged_at],
            ).map_err(|e| e.to_string())?;
        }
        if no_reply != pattern {
            events::emit_email_flag_changed(app, account_id, &c.id, "noReply", no_reply, state.next_seq());
            changed += 1;
        }
    }
    Ok(changed)
}

// ── Phase 3: draft reply + reference retrieval ──────────

/// Max prior emails offered to the model as candidate context.
const REFERENCE_CAP: i64 = 15;
/// Reference snippet length fed to the model.
const REFERENCE_SNIPPET: usize = 300;

/// System prompt for selecting which prior emails are useful reply context.
const PICK_REFERENCES_PROMPT: &str = "\
You help draft an email reply. Given the email being replied to and a list of \
candidate prior emails, pick only those that provide genuinely useful context \
for writing the reply (same thread, related request, prior commitments). Ignore \
unrelated mail. Respond with ONLY a JSON object: {\"ids\":[\"<id>\", ...]}. \
Return an empty list if none are relevant.";

/// System prompt for the reply draft itself. The model returns only the HTML
/// body — no subject, headers, or quoted original.
const DRAFT_PROMPT: &str = "\
You are drafting a reply email on behalf of the user. Write a clear, concise, \
professional reply to the email provided. Use the supplied context only if \
relevant. Output ONLY the email body as simple HTML (<p> paragraphs, optional \
<ul>/<li>); do not include a subject line, email headers, the quoted original, \
or a signature block.";

/// The generated draft, returned to programmatic callers (the MCP `mail.draft`
/// tool). The same content is also pushed to the UI via `mail:draft-generated`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftResult {
    pub in_reply_to: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body_html: String,
    pub references: Vec<String>,
}

/// A prior email offered as candidate reply context.
#[derive(Debug, Clone, serde::Serialize)]
struct ReferenceMail {
    id: String,
    from: String,
    subject: String,
    date: String,
    snippet: String,
}

#[derive(serde::Deserialize)]
struct PickedReferences {
    #[serde(default)]
    ids: Vec<String>,
}

/// Strip leading reply/forward prefixes (`Re:`, `Fwd:`, `Fw:`) repeatedly and
/// trim, so subject-based thread matching groups a conversation together.
fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let stripped = ["re:", "fwd:", "fw:"]
            .iter()
            .find_map(|p| lower.starts_with(p).then(|| s[p.len()..].trim_start()));
        match stripped {
            Some(rest) => s = rest,
            None => break,
        }
    }
    s.to_string()
}

/// Build the reply subject: prefix `Re: ` unless one is already present.
fn reply_subject(subject: &str) -> String {
    if subject.trim_start().to_ascii_lowercase().starts_with("re:") {
        subject.trim().to_string()
    } else {
        format!("Re: {}", subject.trim())
    }
}

/// Heuristic candidate references for replying to `seed`: prior emails from the
/// same sender or sharing the seed's normalized subject, most recent first.
fn find_reference_mail(
    conn: &Connection,
    account_id: &str,
    seed_id: &str,
    seed_from: &str,
    seed_subject: &str,
) -> Result<Vec<ReferenceMail>, String> {
    let subject_like = format!("%{}%", normalize_subject(seed_subject));
    // Scoped to the account's view: a drafted reply must not quote context from
    // a sibling mailbox that happens to share this login.
    let view = crate::login::view_of(conn, account_id).map_err(|e| e.to_string())?;
    let login_id = view.login.as_str().to_string();
    let snippet = REFERENCE_SNIPPET as i64;
    let cap = REFERENCE_CAP;

    let sql = format!(
        "SELECT id, from_name, from_email, subject, date,
                substr(COALESCE(NULLIF(preview, ''), body), 1, ?5)
         FROM emails
         WHERE account_id = ?1
           AND id != ?2
           AND (from_email = ?3 OR subject LIKE ?4){}
         ORDER BY date DESC
         LIMIT ?6",
        crate::login::mailbox_clause(view.mailbox.as_deref(), 7),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut params: Vec<&dyn rusqlite::ToSql> =
        vec![&login_id, &seed_id, &seed_from, &subject_like, &snippet, &cap];
    if let Some(addr) = view.mailbox.as_ref() {
        params.push(addr);
    }
    let rows = stmt.query_map(
        params.as_slice(),
        |row| {
            let from_name: String = row.get(1)?;
            let from_email: String = row.get(2)?;
            Ok(ReferenceMail {
                id: row.get(0)?,
                from: format!("{} <{}>", from_name, from_email),
                subject: row.get(3)?,
                date: row.get(4)?,
                snippet: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        },
    ).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Parse the pick-references response into a list of ids.
fn parse_picked(raw: &str) -> Vec<String> {
    let trimmed = strip_code_fence(raw.trim());
    if let Ok(p) = serde_json::from_str::<PickedReferences>(trimmed) {
        return p.ids;
    }
    serde_json::from_str::<Vec<String>>(trimmed).unwrap_or_default()
}

/// Generate a reply draft for `email_id` and emit `mail:draft-generated`.
/// Runs an agentic reference-retrieval step (DB heuristic → LLM pick) before
/// drafting, so relevant prior mail is folded into the prompt. Returns the
/// generated draft (also pushed to the UI via the event).
pub async fn draft_reply(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
    email_id: &str,
    instructions: Option<String>,
) -> Result<DraftResult, String> {
    let cfg = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        ai_config::load_ai_config(&conn).map_err(|e| e.to_string())?
    };
    if !cfg.enabled {
        return Err("AI is not enabled".to_string());
    }
    if !cfg.has_language_model() {
        return Err("Drafting needs a language model — set the Language Model API in AI settings".to_string());
    }

    let seed = mail_service::get(db, account_id, email_id)?
        .ok_or_else(|| "email not found".to_string())?;

    let candidates = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        find_reference_mail(&conn, account_id, &seed.id, &seed.from_email, &seed.subject)?
    };

    let client = HttpLlmClient::new(&cfg.base_url, &cfg.api_key)?;

    // Step 1 — pick relevant references (skip the call when there's nothing to pick).
    let chosen_ids: Vec<String> = if candidates.is_empty() {
        Vec::new()
    } else if cfg.has_decision_model() {
        let decider = DecisionClient::new(&cfg.decision_base_url, &cfg.decision_api_key, &cfg.decision_model)?;
        let answers = ask_each(&decider, &reference_question(), reference_states(&seed, &candidates)).await;
        let mut p = HashMap::new();
        for (id, r) in answers {
            match r {
                Ok(a) => { if let Some(&v) = a.get("useful") { p.insert(id, v); } }
                // Drafting goes ahead without that candidate.
                Err(e) => log::warn!("ai draft [{}]: reference judgment failed for {}: {}", account_id, id, e),
            }
        }
        pick_references(&p)
    } else {
        let messages = build_pick_messages(&seed, &candidates);
        let opts = ChatOptions {
            model: cfg.classify_model.clone(),
            max_tokens: 256,
            temperature: cfg.temperature,
            json: true,
        };
        match client.complete(&messages, &opts).await {
            Ok(raw) => {
                let picked = parse_picked(&raw);
                // Keep only ids we actually offered.
                picked.into_iter().filter(|id| candidates.iter().any(|c| &c.id == id)).collect()
            }
            Err(e) => {
                log::warn!("ai draft [{}]: reference pick failed, drafting without context: {}", account_id, e);
                Vec::new()
            }
        }
    };

    // Load full bodies for the chosen references.
    let mut references: Vec<mail_service::EmailFull> = Vec::new();
    for id in &chosen_ids {
        if let Ok(Some(full)) = mail_service::get(db, account_id, id) {
            references.push(full);
        }
    }

    // Step 2 — draft the reply.
    let messages = build_draft_messages(&seed, &references, instructions.as_deref());
    let opts = ChatOptions {
        model: cfg.draft_model.clone(),
        max_tokens: cfg.max_tokens.max(512),
        temperature: cfg.temperature,
        json: false,
    };
    let started = std::time::Instant::now();
    let raw = client.complete(&messages, &opts).await?;
    let body_html = strip_code_fence(raw.trim()).to_string();
    log::info!(
        "ai draft [{}]: model={} refs={} latency={}ms",
        account_id, cfg.draft_model, references.len(), started.elapsed().as_millis()
    );

    let result = DraftResult {
        in_reply_to: seed.id.clone(),
        to: vec![reply_policy::effective_reply_address(&seed.from_email, &seed.reply_to).to_string()],
        cc: Vec::new(),
        subject: reply_subject(&seed.subject),
        body_html,
        references: chosen_ids,
    };

    let seq = app.state::<crate::AppState>().next_seq();
    events::emit_mail_draft_generated(app, events::MailDraftGeneratedEvent {
        account_id: account_id.to_string(),
        in_reply_to: Some(result.in_reply_to.clone()),
        to: result.to.clone(),
        cc: result.cc.clone(),
        subject: result.subject.clone(),
        body_html: result.body_html.clone(),
        references: result.references.clone(),
        seq,
    });

    Ok(result)
}

/// Decision-model probability at or above which a prior email is used as
/// reply context.
const REFERENCE_P: f64 = 0.6;

#[derive(serde::Serialize)]
struct RefEmail {
    from: String,
    subject: String,
    summary: String,
}

#[derive(serde::Serialize)]
struct RefCandidate {
    from: String,
    date: String,
    subject: String,
    snippet: String,
}

/// One small state per candidate — the email being answered, then the
/// candidate — so every candidate is judged in full even by a model that
/// truncates long states.
#[derive(serde::Serialize)]
struct ReferenceState {
    email: RefEmail,
    candidate: RefCandidate,
}

fn reference_states(seed: &mail_service::EmailFull, candidates: &[ReferenceMail]) -> Vec<(String, ReferenceState)> {
    candidates
        .iter()
        .map(|c| (c.id.clone(), ReferenceState {
            email: RefEmail {
                from: format!("{} <{}>", seed.from_name, seed.from_email),
                subject: seed.subject.clone(),
                summary: truncate(&seed.preview, REFERENCE_SNIPPET),
            },
            candidate: RefCandidate {
                from: c.from.clone(),
                date: c.date.clone(),
                subject: c.subject.clone(),
                snippet: c.snippet.replace('\n', " "),
            },
        }))
        .collect()
}

fn reference_question() -> HashMap<String, Question> {
    HashMap::from([("useful".to_string(), Question::noul(
        "Does `candidate` give useful context for writing a reply to `email` \
         (same thread, related request, or a prior commitment)?",
        "Useful context for the reply.",
        "Unrelated to the reply.",
    ))])
}

/// Candidate ids whose probability cleared `REFERENCE_P`, most relevant first.
fn pick_references(p: &HashMap<String, f64>) -> Vec<String> {
    let mut picked: Vec<(f64, &String)> = p.iter().filter(|(_, &v)| v >= REFERENCE_P).map(|(id, &v)| (v, id)).collect();
    picked.sort_by(|a, b| b.0.total_cmp(&a.0));
    picked.into_iter().map(|(_, id)| id.clone()).collect()
}

/// Build messages for the reference-selection step.
fn build_pick_messages(seed: &mail_service::EmailFull, candidates: &[ReferenceMail]) -> Vec<ChatMessage> {
    let mut user = format!(
        "Email being replied to:\nFrom: {} <{}>\nSubject: {}\nBody: {}\n\nCandidate prior emails:\n",
        seed.from_name, seed.from_email, seed.subject, truncate(&seed.preview, REFERENCE_SNIPPET),
    );
    for c in candidates {
        user.push_str(&format!(
            "\n--- id: {}\nFrom: {}\nDate: {}\nSubject: {}\nSnippet: {}\n",
            c.id, c.from, c.date, c.subject, c.snippet.replace('\n', " "),
        ));
    }
    vec![ChatMessage::system(PICK_REFERENCES_PROMPT), ChatMessage::user(user)]
}

/// Build messages for the draft-generation step.
fn build_draft_messages(
    seed: &mail_service::EmailFull,
    references: &[mail_service::EmailFull],
    instructions: Option<&str>,
) -> Vec<ChatMessage> {
    let mut user = String::new();
    if !references.is_empty() {
        user.push_str("Relevant prior emails for context:\n");
        for r in references {
            user.push_str(&format!(
                "\n--- From: {} <{}>\nSubject: {}\n{}\n",
                r.from_name, r.from_email, r.subject, truncate(&r.preview, REFERENCE_SNIPPET),
            ));
        }
        user.push('\n');
    }
    user.push_str(&format!(
        "Email to reply to:\nFrom: {} <{}>\nSubject: {}\n\n{}\n",
        seed.from_name, seed.from_email, seed.subject, seed.body,
    ));
    if let Some(extra) = instructions.filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!("\nAdditional instructions for the reply: {}\n", extra.trim()));
    }
    vec![ChatMessage::system(DRAFT_PROMPT), ChatMessage::user(user)]
}

/// Truncate a string to at most `max` chars (char-boundary safe).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_object_form() {
        let raw = r#"{"results":[{"id":"a","is_junk":true,"is_priority":false,"reason":"spam"}]}"#;
        let v = parse_verdicts(raw).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "a");
        assert!(v[0].is_junk);
        assert!(!v[0].is_priority);
    }

    #[test]
    fn parse_bare_array_form() {
        let raw = r#"[{"id":"x","is_priority":true}]"#;
        let v = parse_verdicts(raw).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].is_priority);
        assert!(!v[0].is_junk); // defaulted
    }

    #[test]
    fn parse_code_fenced() {
        let raw = "```json\n{\"results\":[{\"id\":\"a\"}]}\n```";
        let v = parse_verdicts(raw).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "a");
    }

    #[test]
    fn parse_garbage_errors() {
        assert!(parse_verdicts("not json").is_err());
    }

    fn verdict(id: &str, junk: bool, prio: bool) -> Verdict {
        Verdict { id: id.into(), is_junk: junk, is_phishing: false, is_priority: prio, reason: String::new() }
    }

    fn features(junk: bool, prio: bool) -> AccountAiFeatures {
        AccountAiFeatures { junk_filter: junk, priority_detect: prio, auto_draft: false, external_access: false, reply_detect: false }
    }

    #[test]
    fn plan_junk_when_enabled() {
        assert_eq!(plan_action(&verdict("a", true, false), features(true, true)), Action::Junk);
    }

    #[test]
    fn plan_junk_precedes_priority() {
        // Both flags true → junk wins (the email is moved, not focused).
        assert_eq!(plan_action(&verdict("a", true, true), features(true, true)), Action::Junk);
    }

    #[test]
    fn plan_priority_when_junk_feature_off() {
        // junk_filter off → a junk+priority email falls through to Focus.
        assert_eq!(plan_action(&verdict("a", true, true), features(false, true)), Action::Focus);
    }

    #[test]
    fn plan_record_only_when_features_off() {
        assert_eq!(plan_action(&verdict("a", true, true), features(false, false)), Action::RecordOnly);
        // priority verdict but priority_detect off → no action.
        assert_eq!(plan_action(&verdict("a", false, true), features(true, false)), Action::RecordOnly);
    }

    #[test]
    fn focus_update_respects_manual_override() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT,
                 is_focused INTEGER, is_focused_manual INTEGER);
             -- auto, currently regular → should flip to focused
             INSERT INTO emails VALUES ('auto','a',0,0);
             -- user manually set to regular → must NOT be touched
             INSERT INTO emails VALUES ('manual','a',0,1);
             -- already focused → no-op
             INSERT INTO emails VALUES ('already','a',1,0);",
        ).unwrap();

        let changed_auto = conn.execute(FOCUS_UPDATE_SQL, rusqlite::params!["auto", "a"]).unwrap();
        let changed_manual = conn.execute(FOCUS_UPDATE_SQL, rusqlite::params!["manual", "a"]).unwrap();
        let changed_already = conn.execute(FOCUS_UPDATE_SQL, rusqlite::params!["already", "a"]).unwrap();

        assert_eq!(changed_auto, 1, "auto-regular email should be focused");
        assert_eq!(changed_manual, 0, "manual override must be preserved");
        assert_eq!(changed_already, 0, "already-focused email is a no-op");

        let manual_focus: i32 = conn
            .query_row("SELECT is_focused FROM emails WHERE id = 'manual'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(manual_focus, 0, "manual email stays regular");
    }

    #[test]
    fn normalize_subject_strips_prefixes() {
        assert_eq!(normalize_subject("Re: Hello"), "Hello");
        assert_eq!(normalize_subject("RE: FW: Re:  Budget"), "Budget");
        assert_eq!(normalize_subject("Fwd: Report"), "Report");
        assert_eq!(normalize_subject("No prefix"), "No prefix");
    }

    #[test]
    fn reply_subject_prefixes_once() {
        assert_eq!(reply_subject("Hello"), "Re: Hello");
        assert_eq!(reply_subject("Re: Hello"), "Re: Hello");
        assert_eq!(reply_subject("RE: Hello"), "RE: Hello");
    }

    #[test]
    fn parse_picked_forms() {
        assert_eq!(parse_picked(r#"{"ids":["a","b"]}"#), vec!["a", "b"]);
        assert_eq!(parse_picked(r#"["x"]"#), vec!["x"]);
        assert_eq!(parse_picked("```json\n{\"ids\":[]}\n```"), Vec::<String>::new());
        assert_eq!(parse_picked("garbage"), Vec::<String>::new());
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 3), "hel");
        // multi-byte char must not panic mid-boundary
        assert_eq!(truncate("héllo", 2), "hé");
    }

    #[test]
    fn find_references_matches_sender_or_subject() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            // Reads resolve account → login now, so the account must exist.
            // is_mailbox = 0 keeps this test's original scope: the whole login.
            "CREATE TABLE accounts (id TEXT PRIMARY KEY, email TEXT NOT NULL,
                 login_id TEXT NOT NULL, is_mailbox INTEGER NOT NULL DEFAULT 0);
             INSERT INTO accounts VALUES ('a','a@x','a',0);
             INSERT INTO accounts VALUES ('b','b@x','b',0);
             CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT, from_name TEXT,
                 from_email TEXT, subject TEXT, preview TEXT, body TEXT, date TEXT);
             -- same sender, different subject → matches via sender
             INSERT INTO emails VALUES ('r1','a','Bob','bob@x','Lunch?','p','b','2026-01-01');
             -- same thread subject, different sender → matches via subject LIKE
             INSERT INTO emails VALUES ('r2','a','Eve','eve@y','Re: Project Plan','p','b','2026-01-02');
             -- unrelated → no match
             INSERT INTO emails VALUES ('r3','a','Zed','zed@z','Newsletter','p','b','2026-01-03');
             -- the seed itself → excluded
             INSERT INTO emails VALUES ('seed','a','Bob','bob@x','Project Plan','p','b','2026-01-04');
             -- other account → excluded
             INSERT INTO emails VALUES ('r4','b','Bob','bob@x','Project Plan','p','b','2026-01-05');",
        ).unwrap();

        let refs = find_reference_mail(&conn, "a", "seed", "bob@x", "Project Plan").unwrap();
        let ids: Vec<&str> = refs.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"r1"), "same-sender email should match");
        assert!(ids.contains(&"r2"), "same-thread subject should match");
        assert!(!ids.contains(&"r3"), "unrelated email should not match");
        assert!(!ids.contains(&"seed"), "seed must be excluded");
        assert!(!ids.contains(&"r4"), "other account must be excluded");
    }

    /// Columns and tables the candidate queries read that the minimal test
    /// schemas predate. Added after the positional INSERTs so those stay short.
    fn add_later_columns(conn: &Connection) {
        conn.execute_batch(
            "ALTER TABLE emails ADD COLUMN raw_headers TEXT NOT NULL DEFAULT '';
             ALTER TABLE emails ADD COLUMN auth_results TEXT NOT NULL DEFAULT '';
             ALTER TABLE emails ADD COLUMN reply_to TEXT NOT NULL DEFAULT '';
             CREATE TABLE IF NOT EXISTS email_recipients (email_id TEXT, name TEXT, email TEXT,
                 initials TEXT, color TEXT, type TEXT);
             CREATE TABLE IF NOT EXISTS email_reply_verdict (email_id TEXT PRIMARY KEY);",
        ).unwrap();
    }

    #[test]
    fn reply_candidates_skip_judged_and_own_mail() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE accounts (id TEXT PRIMARY KEY, email TEXT NOT NULL,
                 login_id TEXT NOT NULL, is_mailbox INTEGER NOT NULL DEFAULT 0);
             INSERT INTO accounts VALUES ('a','a@x','a',0);
             CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT, from_name TEXT,
                 from_email TEXT, subject TEXT, preview TEXT, body TEXT, folder TEXT, date TEXT);
             INSERT INTO emails VALUES ('e1','a','N','n@x','s','p','b','inbox','2026-01-04');
             INSERT INTO emails VALUES ('e2','a','N','n@x','s','p','b','archive','2026-01-03');
             INSERT INTO emails VALUES ('e3','a','N','n@x','s','p','b','sent','2026-01-02');
             INSERT INTO emails VALUES ('e4','a','N','n@x','s','p','b','drafts','2026-01-01');",
        ).unwrap();
        add_later_columns(&conn);
        conn.execute("INSERT INTO email_reply_verdict VALUES ('e1')", []).unwrap();
        let ids: Vec<String> = select_reply_candidates(&conn, "a", 10).unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["e2"], "judged, sent and draft mail are not asked about");
    }

    #[test]
    fn candidate_selection_skips_classified() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE accounts (id TEXT PRIMARY KEY, email TEXT NOT NULL,
                 login_id TEXT NOT NULL, is_mailbox INTEGER NOT NULL DEFAULT 0);
             INSERT INTO accounts VALUES ('a','a@x','a',0);
             CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT, from_name TEXT,
                 from_email TEXT, subject TEXT, preview TEXT, body TEXT, folder TEXT, date TEXT);
             CREATE TABLE email_ai_classification (email_id TEXT PRIMARY KEY);
             INSERT INTO emails VALUES ('e1','a','N','n@x','s','p','b','inbox','2026-01-02');
             INSERT INTO emails VALUES ('e2','a','N','n@x','s','p','b','inbox','2026-01-01');
             INSERT INTO emails VALUES ('e3','a','N','n@x','s','p','b','sent','2026-01-03');
             INSERT INTO email_ai_classification VALUES ('e1');",
        ).unwrap();
        add_later_columns(&conn);
        let cands = select_candidates(&conn, "a", 10).unwrap();
        // e1 already classified, e3 not in inbox → only e2 remains.
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].id, "e2");
    }

    /// A mailbox account's AI must classify only its own mail, not everything
    /// on a login it shares with a sibling.
    #[test]
    fn candidate_selection_is_scoped_to_the_mailbox() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE accounts (id TEXT PRIMARY KEY, email TEXT NOT NULL,
                 login_id TEXT NOT NULL, is_mailbox INTEGER NOT NULL DEFAULT 0);
             -- two mailboxes sharing login 'work'
             INSERT INTO accounts VALUES ('work','takashi@x','work',1);
             INSERT INTO accounts VALUES ('eng','engineering@x','work',1);
             CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT, from_name TEXT,
                 from_email TEXT, subject TEXT, preview TEXT, body TEXT, folder TEXT,
                 date TEXT, delivered_to TEXT);
             CREATE TABLE email_recipients (email_id TEXT, name TEXT, email TEXT,
                 initials TEXT, color TEXT, type TEXT);
             CREATE TABLE email_ai_classification (email_id TEXT PRIMARY KEY);
             -- one dataset under the login, addressed to different mailboxes
             INSERT INTO emails VALUES ('m1','work','N','n@x','s','p','b','inbox','2026-01-01','takashi@x');
             INSERT INTO emails VALUES ('m2','work','N','n@x','s','p','b','inbox','2026-01-02','engineering@x');",
        ).unwrap();
        add_later_columns(&conn);

        let eng: Vec<String> = select_candidates(&conn, "eng", 10).unwrap()
            .into_iter().map(|c| c.id).collect();
        assert_eq!(eng, vec!["m2"], "engineering's AI saw a sibling's mail");

        let work: Vec<String> = select_candidates(&conn, "work", 10).unwrap()
            .into_iter().map(|c| c.id).collect();
        assert_eq!(work, vec!["m1"], "takashi's AI saw a sibling's mail");
    }

    fn probs(spam: f64, phishing: f64, priority: f64) -> HashMap<String, f64> {
        HashMap::from([
            ("spam".to_string(), spam),
            ("phishing".to_string(), phishing),
            ("priority".to_string(), priority),
        ])
    }

    #[test]
    fn decision_thresholds_are_conservative() {
        let v = verdict_from_probabilities("a", &probs(0.84, 0.10, 0.69));
        assert!(!v.is_junk && !v.is_phishing && !v.is_priority);
        let v = verdict_from_probabilities("a", &probs(0.90, 0.10, 0.75));
        assert!(v.is_junk && !v.is_phishing && v.is_priority);
    }

    #[test]
    fn phishing_alone_is_junk() {
        let v = verdict_from_probabilities("a", &probs(0.05, 0.97, 0.9));
        assert!(v.is_phishing && v.is_junk);
        assert_eq!(plan_action(&v, features(true, true)), Action::Junk);
    }

    fn reply_candidate(id: &str) -> ReplyCandidate {
        ReplyCandidate {
            id: id.into(), from_name: String::new(), from_email: "x@y.com".into(), reply_to: String::new(),
            subject: String::new(), snippet: String::new(), raw_headers: String::new(), auth_results: String::new(),
        }
    }

    #[test]
    fn llm_reply_verdicts_read_as_no_reply_probability() {
        let cands = [reply_candidate("a"), reply_candidate("b"), reply_candidate("c")];
        let raw = r#"{"results":[
            {"id":"a","no_reply":true,"confidence":0.98},
            {"id":"b","no_reply":false,"confidence":0.97},
            {"id":"c","no_reply":true,"confidence":0.6},
            {"id":"zz","no_reply":true,"confidence":1.0}]}"#;
        let p = parse_reply_verdicts(raw, &cands).unwrap();
        assert!(p["a"] >= NO_REPLY_P);
        // "false, 97% sure" and "false, P(no_reply)=3%" must both stay replyable.
        assert!(p["b"] < 0.5);
        assert!(p["c"] < NO_REPLY_P);
        assert!(!p.contains_key("zz"), "ids that were not asked about are dropped");
    }

    #[test]
    fn headers_drop_signatures_unfold_and_cap() {
        let raw = "Received: from mx.example.com\r\n\tby mx.google.com\r\n\
                   DKIM-Signature: v=1; a=rsa-sha256;\r\n b=AAAABBBBCCCC\r\n\
                   Return-Path: <bounce@spam.example>\r\n\
                   Authentication-Results: mx.google.com; spf=fail";
        let h = headers_for_model(raw, "", 4000);
        assert!(h.contains("Received: from mx.example.com by mx.google.com"));
        assert!(h.contains("Return-Path: <bounce@spam.example>"));
        assert!(h.contains("spf=fail"));
        assert!(!h.to_ascii_lowercase().contains("dkim-signature"));
        let capped = headers_for_model(raw, "", 60);
        assert!(capped.chars().count() <= 60);
        assert!(capped.starts_with("Received:"), "the top of the block survives the cap");
    }

    #[test]
    fn headers_fall_back_to_the_auth_summary() {
        assert_eq!(headers_for_model("", "spf=pass dkim=pass", 100), "Authentication-Results: spf=pass dkim=pass");
        assert_eq!(headers_for_model("", "", 100), "");
    }

    #[test]
    fn an_unsure_model_defers_to_the_pattern() {
        assert!(decide_no_reply(0.97, false), "a confident no-reply disables reply");
        assert!(!decide_no_reply(0.02, true), "a confident replyable overrules the pattern");
        assert!(decide_no_reply(0.59, true), "base Laya's ~0.5 must not re-enable noreply@medium.com");
        assert!(!decide_no_reply(0.59, false));
    }

    #[test]
    fn references_are_thresholded_and_ranked() {
        let p = HashMap::from([("r0".into(), 0.7), ("r1".into(), 0.2), ("r2".into(), 0.95)]);
        assert_eq!(pick_references(&p), vec!["r2", "r0"]);
    }

    #[test]
    fn decision_states_put_headers_last() {
        let c = Candidate {
            id: "a".into(), from_name: "N".into(), from_email: "n@x".into(), to: "me@x".into(),
            subject: "S".into(), snippet: "body".into(), raw_headers: "Received: x".into(), auth_results: String::new(),
        };
        let wire = serde_json::to_string(&classification_state(&c)).unwrap();
        let pos = |k: &str| wire.find(&format!("\"{k}\"")).unwrap();
        assert!(pos("sender") < pos("recipients") && pos("subject") < pos("summary") && pos("summary") < pos("headers"),
            "a right-truncating model must lose headers first: {wire}");
    }

}
