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

        if features.junk_filter && v.is_junk {
            let seq = state.next_seq();
            match mail_service::move_email(db, app, seq, account_id, &v.id, "junk") {
                Ok(()) => { applied += 1; }
                Err(e) => log::error!("ai classify: junk move failed for {}: {}", v.id, e),
            }
            // Junk takes precedence; don't also flip focus.
            continue;
        }

        if features.priority_detect && v.is_priority {
            let changed = {
                let conn = db.lock().map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE emails SET is_focused = 1
                     WHERE id = ?1 AND account_id = ?2 AND is_focused_manual = 0 AND is_focused = 0",
                    rusqlite::params![v.id, account_id],
                ).map_err(|e| e.to_string())?
            };
            if changed > 0 {
                let seq = state.next_seq();
                events::emit_email_flag_changed(app, account_id, &v.id, "focused", true, seq);
                applied += 1;
            }
        }
    }

    Ok(applied)
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
