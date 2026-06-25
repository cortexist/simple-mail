//! ai_service.rs — LLM-backed mail intelligence (Phase 2: junk + priority).
//!
//! `classify_new_mail` is the entry point: it selects inbox emails that have no
//! verdict yet, asks the configured model to judge each as junk and/or priority,
//! then applies the verdicts through the existing service/echo layer (junk →
//! `mail_service::move_email`, priority → flip `is_focused`). A row in
//! `email_ai_classification` is written for every judged email so re-sync never
//! reclassifies, and user manual overrides (`is_focused_manual`) are preserved.
//!
//! The verdict-producing call is factored behind `LlmClient` so the logic can be
//! exercised with a mock in tests; the live path wraps `HttpLlmClient`.

use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use tauri::{AppHandle, Manager};

use crate::ai_config::{self, AccountAiFeatures, AiConfig};
use crate::llm_client::{ChatMessage, ChatOptions, HttpLlmClient, LlmClient};
use crate::{events, mail_service};

/// Max emails classified per run. Keeps the batch (and the model's output
/// budget) bounded; remaining unclassified mail is picked up on the next sync.
const BATCH_CAP: i64 = 20;
/// Body/preview characters fed to the model per email.
const SNIPPET_CHARS: usize = 500;

/// Fixed system instructions. Kept byte-identical across calls (no per-email
/// interpolation) so server-side prompt-prefix caches can skip its prefill.
const SYSTEM_PROMPT: &str = "\
You are an email triage classifier inside a desktop mail client. For each email \
in the batch decide two booleans:
- is_junk: true only for spam, phishing, scams, or unsolicited bulk marketing. \
Legitimate mail the user likely wants (receipts, subscribed newsletters, \
account notices, real correspondence) is NOT junk.
- is_priority: true if it plausibly needs the user's personal attention or a \
timely response — a real person writing to them, a security/account alert, a \
deadline or time-sensitive request. Routine automated, promotional, or \
no-reply mail is NOT priority.
Be conservative: when unsure, use false for both.
Respond with ONLY a JSON object, no prose, of exactly this shape:
{\"results\":[{\"id\":\"<id>\",\"is_junk\":false,\"is_priority\":false,\"reason\":\"<=8 words\"}]}
Return exactly one entry per input email, echoing its id verbatim.";

/// One inbox email awaiting a verdict.
#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    from_name: String,
    from_email: String,
    subject: String,
    snippet: String,
}

/// A model verdict for a single email.
#[derive(Debug, Clone, serde::Deserialize)]
struct Verdict {
    id: String,
    #[serde(default)]
    is_junk: bool,
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

    if !cfg.enabled || (!features.junk_filter && !features.priority_detect) {
        return Ok(0);
    }

    let candidates = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        select_candidates(&conn, account_id, BATCH_CAP)?
    };
    if candidates.is_empty() {
        return Ok(0);
    }

    let client = HttpLlmClient::new(&cfg.base_url, &cfg.api_key)?;
    classify_with(db, app, account_id, &client, &cfg, features, candidates).await
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

/// Select inbox emails for `account_id` that have no verdict yet, newest first.
fn select_candidates(conn: &Connection, account_id: &str, limit: i64) -> Result<Vec<Candidate>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, from_name, from_email, subject,
                substr(COALESCE(NULLIF(preview, ''), body), 1, ?3)
         FROM emails e
         WHERE account_id = ?1
           AND folder = 'inbox'
           AND NOT EXISTS (
               SELECT 1 FROM email_ai_classification c WHERE c.email_id = e.id
           )
         ORDER BY date DESC
         LIMIT ?2",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(
        rusqlite::params![account_id, limit, SNIPPET_CHARS as i64],
        |row| Ok(Candidate {
            id: row.get(0)?,
            from_name: row.get(1)?,
            from_email: row.get(2)?,
            subject: row.get(3)?,
            snippet: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
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
            "\n--- id: {}\nFrom: {} <{}>\nSubject: {}\nPreview: {}\n",
            c.id,
            c.from_name,
            c.from_email,
            c.subject,
            c.snippet.replace('\n', " "),
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
    if features.junk_filter && v.is_junk {
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
                   (email_id, account_id, is_junk, is_priority, reason, model, classified_at, seq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    v.id, account_id, v.is_junk as i32, v.is_priority as i32,
                    v.reason, model, classified_at, row_seq
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
    let mut stmt = conn.prepare(
        "SELECT id, from_name, from_email, subject, date,
                substr(COALESCE(NULLIF(preview, ''), body), 1, ?5)
         FROM emails
         WHERE account_id = ?1
           AND id != ?2
           AND (from_email = ?3 OR subject LIKE ?4)
         ORDER BY date DESC
         LIMIT ?6",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(
        rusqlite::params![account_id, seed_id, seed_from, subject_like, REFERENCE_SNIPPET as i64, REFERENCE_CAP],
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
/// reference ids that were used.
pub async fn draft_reply(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    account_id: &str,
    email_id: &str,
    instructions: Option<String>,
) -> Result<Vec<String>, String> {
    let cfg = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        ai_config::load_ai_config(&conn).map_err(|e| e.to_string())?
    };
    if !cfg.enabled {
        return Err("AI is not enabled".to_string());
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

    let seq = app.state::<crate::AppState>().next_seq();
    events::emit_mail_draft_generated(app, events::MailDraftGeneratedEvent {
        account_id: account_id.to_string(),
        in_reply_to: Some(seed.id.clone()),
        to: vec![seed.from_email.clone()],
        cc: Vec::new(),
        subject: reply_subject(&seed.subject),
        body_html,
        references: chosen_ids.clone(),
        seq,
    });

    Ok(chosen_ids)
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
        Verdict { id: id.into(), is_junk: junk, is_priority: prio, reason: String::new() }
    }

    fn features(junk: bool, prio: bool) -> AccountAiFeatures {
        AccountAiFeatures { junk_filter: junk, priority_detect: prio, auto_draft: false, external_access: false }
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
            "CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT, from_name TEXT,
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

    #[test]
    fn candidate_selection_skips_classified() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT, from_name TEXT,
                 from_email TEXT, subject TEXT, preview TEXT, body TEXT, folder TEXT, date TEXT);
             CREATE TABLE email_ai_classification (email_id TEXT PRIMARY KEY);
             INSERT INTO emails VALUES ('e1','a','N','n@x','s','p','b','inbox','2026-01-02');
             INSERT INTO emails VALUES ('e2','a','N','n@x','s','p','b','inbox','2026-01-01');
             INSERT INTO emails VALUES ('e3','a','N','n@x','s','p','b','sent','2026-01-03');
             INSERT INTO email_ai_classification VALUES ('e1');",
        ).unwrap();
        let cands = select_candidates(&conn, "a", 10).unwrap();
        // e1 already classified, e3 not in inbox → only e2 remains.
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].id, "e2");
    }
}
