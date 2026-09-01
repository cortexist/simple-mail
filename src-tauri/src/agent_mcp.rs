//! agent_mcp.rs — Local MCP (Model Context Protocol) server.
//!
//! Exposes a small set of mailbox tools to external LLM agents (OpenClaw,
//! Hermes, …) over **loopback-only** HTTP using JSON-RPC 2.0. Every request
//! must carry `Authorization: Bearer <app-key>`; only the SHA-256 of each key
//! is stored (see `agent_api_keys`), compared in constant time. Tool handlers
//! wrap the shared service layer (`mail_service` / `ai_service`) so agent
//! actions flow through the same echoes the desktop UI listens to, and are
//! gated per-account by the `external_access` feature toggle.
//!
//! Transport: a single `POST /` endpoint speaking JSON-RPC. Methods:
//! `initialize`, `tools/list`, `tools/call`, `ping` (+ `notifications/*` no-op).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Body,
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use crate::{ai_config, ai_service, mail_service, smtp_client};

/// Protocol version we advertise in `initialize`.
const PROTOCOL_VERSION: &str = "2024-11-05";

// ── Shared state & handle ───────────────────────────────

#[derive(Clone)]
pub struct McpState {
    pub db: Arc<Mutex<Connection>>,
    /// Used to reach `AppState::next_seq()` and emit the same push echoes the
    /// UI consumes, so agent actions reflect live in the desktop app.
    pub app: AppHandle,
}

pub struct McpServerHandle {
    shutdown_tx: watch::Sender<bool>,
    local_addr: SocketAddr,
}

impl McpServerHandle {
    pub fn addr(&self) -> SocketAddr {
        self.local_addr
    }
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

// ── Key management ──────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentKeyInfo {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

/// SHA-256 of `s` as lowercase hex.
pub fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// Generate a new app key: returns `(id, plaintext)`. Only the hash is stored;
/// the plaintext is the caller's single chance to see it.
pub fn generate_key(conn: &Connection, label: &str) -> Result<(String, String), String> {
    let plaintext = format!(
        "am_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let id = uuid::Uuid::new_v4().to_string();
    let created = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO agent_api_keys (id, label, key_sha256, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, label, sha256_hex(&plaintext), created],
    ).map_err(|e| e.to_string())?;
    Ok((id, plaintext))
}

/// List stored keys (metadata only — never the hash or plaintext).
pub fn list_keys(conn: &Connection) -> Result<Vec<AgentKeyInfo>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, label, created_at, last_used_at FROM agent_api_keys ORDER BY created_at",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(AgentKeyInfo {
        id: r.get(0)?,
        label: r.get(1)?,
        created_at: r.get(2)?,
        last_used_at: r.get(3)?,
    })).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Revoke (delete) a key by id.
pub fn revoke_key(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM agent_api_keys WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Constant-time byte comparison (no early return on mismatch).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Verify a presented key against the stored hashes in constant time over the
/// full key set; on success, stamp `last_used_at`. Returns whether it matched.
fn verify_key(db: &Arc<Mutex<Connection>>, presented: &str) -> bool {
    let presented_hash = sha256_hex(presented);
    let conn = match db.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut matched: Option<String> = None;
    if let Ok(mut stmt) = conn.prepare("SELECT id, key_sha256 FROM agent_api_keys") {
        if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
            for (id, hash) in rows.flatten() {
                // Compare every row (no break) to keep timing independent of position.
                if ct_eq(presented_hash.as_bytes(), hash.as_bytes()) {
                    matched = Some(id);
                }
            }
        }
    }
    if let Some(id) = matched {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute("UPDATE agent_api_keys SET last_used_at = ?2 WHERE id = ?1", params![id, now]).ok();
        true
    } else {
        false
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
}

// ── Server start / stop ─────────────────────────────────

pub async fn start_server(
    db: Arc<Mutex<Connection>>,
    app: AppHandle,
    bind_addr: SocketAddr,
) -> Result<McpServerHandle, String> {
    let state = McpState { db, app };

    let router = Router::new()
        .route("/", post(rpc_handler).get(health))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("Failed to bind MCP server to {}: {}", bind_addr, e))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?;

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            while !*shutdown_rx.borrow_and_update() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .ok();
    });

    log::info!("MCP agent server started on {}", local_addr);
    Ok(McpServerHandle { shutdown_tx, local_addr })
}

async fn health() -> Response {
    (StatusCode::OK, "agentic-mail MCP server").into_response()
}

// ── JSON-RPC plumbing ───────────────────────────────────

fn rpc_ok(id: Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn rpc_err(id: Value, code: i64, message: &str) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })).into_response()
}

fn unauthorized() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("WWW-Authenticate", "Bearer")
        .body(Body::empty())
        .unwrap()
}

async fn rpc_handler(
    State(state): State<McpState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: String,
) -> Response {
    // Loopback only — defense in depth even though we bind to 127.0.0.1.
    if !peer.ip().is_loopback() {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Auth: Bearer app-key, constant-time hash compare.
    match bearer_token(&headers) {
        Some(tok) if verify_key(&state.db, &tok) => {}
        _ => return unauthorized(),
    }

    let req: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return rpc_err(Value::Null, -32700, "Parse error"),
    };

    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => rpc_ok(id, json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "agentic-mail", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => rpc_ok(id, json!({})),
        // Notifications carry no id and expect no response body.
        m if m.starts_with("notifications/") => StatusCode::ACCEPTED.into_response(),
        "tools/list" => rpc_ok(id, json!({ "tools": tools_list() })),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match execute_tool(&state, name, &args).await {
                Ok(data) => rpc_ok(id, json!({
                    "content": [ { "type": "text", "text": data.to_string() } ],
                    "isError": false,
                })),
                Err(msg) => rpc_ok(id, json!({
                    "content": [ { "type": "text", "text": msg } ],
                    "isError": true,
                })),
            }
        }
        _ => rpc_err(id, -32601, "Method not found"),
    }
}

// ── Tool registry ───────────────────────────────────────

fn tools_list() -> Value {
    let acct = json!({ "type": "string", "description": "Account id (see mail.list_accounts)" });
    let email_id = json!({ "type": "string", "description": "Email id" });
    json!([
        {
            "name": "mail.list_accounts",
            "description": "List accounts that have granted external (agent) access.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "mail.search",
            "description": "Full-text search an account's mailbox; returns compact summaries.",
            "inputSchema": { "type": "object", "properties": {
                "accountId": acct, "query": { "type": "string" },
                "limit": { "type": "integer", "description": "Max results (default 20)" }
            }, "required": ["accountId", "query"] }
        },
        {
            "name": "mail.get",
            "description": "Fetch a single email with its full body.",
            "inputSchema": { "type": "object", "properties": { "accountId": acct, "emailId": email_id }, "required": ["accountId", "emailId"] }
        },
        {
            "name": "mail.send",
            "description": "Send an email (also saved to the Sent folder).",
            "inputSchema": { "type": "object", "properties": {
                "accountId": acct,
                "to": { "type": "array", "items": { "type": "string" } },
                "cc": { "type": "array", "items": { "type": "string" } },
                "subject": { "type": "string" },
                "bodyHtml": { "type": "string" }
            }, "required": ["accountId", "to", "subject", "bodyHtml"] }
        },
        {
            "name": "mail.move",
            "description": "Move an email to a target folder (inbox, archive, junk, deleted, …).",
            "inputSchema": { "type": "object", "properties": { "accountId": acct, "emailId": email_id, "targetFolder": { "type": "string" } }, "required": ["accountId", "emailId", "targetFolder"] }
        },
        {
            "name": "mail.delete",
            "description": "Delete an email (soft-delete to trash, or permanent if already there).",
            "inputSchema": { "type": "object", "properties": { "accountId": acct, "emailId": email_id }, "required": ["accountId", "emailId"] }
        },
        {
            "name": "mail.draft",
            "description": "Generate an AI reply draft for an email; returns the draft content.",
            "inputSchema": { "type": "object", "properties": {
                "accountId": acct, "emailId": email_id,
                "instructions": { "type": "string", "description": "Optional extra guidance for the reply" }
            }, "required": ["accountId", "emailId"] }
        },
        {
            "name": "mail.classify",
            "description": "Run AI junk/priority classification over the account's unclassified inbox mail.",
            "inputSchema": { "type": "object", "properties": { "accountId": acct }, "required": ["accountId"] }
        }
    ])
}

// ── Tool execution ──────────────────────────────────────

fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing required string argument '{}'", key))
}

fn arg_str_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

/// Fail unless the account exists and has granted external access.
fn require_external(db: &Arc<Mutex<Connection>>, account_id: &str) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let f = ai_config::load_account_features(&conn, account_id).map_err(|e| e.to_string())?;
    if f.external_access {
        Ok(())
    } else {
        Err(format!("account '{}' has not granted external access", account_id))
    }
}

fn next_seq(state: &McpState) -> u64 {
    state.app.state::<crate::AppState>().next_seq()
}

async fn execute_tool(state: &McpState, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "mail.list_accounts" => {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(
                "SELECT a.id, a.name, a.email FROM accounts a
                 JOIN account_ai_features f ON f.account_id = a.id
                 WHERE f.external_access = 1
                 ORDER BY a.position, a.rowid",
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |r| Ok(json!({
                "id": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "email": r.get::<_, String>(2)?,
            }))).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
            Ok(json!({ "accounts": out }))
        }
        "mail.search" => {
            let account_id = arg_str(args, "accountId")?;
            require_external(&state.db, &account_id)?;
            let query = arg_str(args, "query")?;
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);
            let results = mail_service::search(&state.db, &account_id, &query, limit)?;
            Ok(json!({ "results": results }))
        }
        "mail.get" => {
            let account_id = arg_str(args, "accountId")?;
            require_external(&state.db, &account_id)?;
            let email_id = arg_str(args, "emailId")?;
            match mail_service::get(&state.db, &account_id, &email_id)? {
                Some(email) => Ok(json!({ "email": email })),
                None => Err("email not found".to_string()),
            }
        }
        "mail.send" => {
            let account_id = arg_str(args, "accountId")?;
            require_external(&state.db, &account_id)?;
            let email = smtp_client::OutboundEmail {
                to: arg_str_array(args, "to"),
                cc: arg_str_array(args, "cc"),
                bcc: arg_str_array(args, "bcc"),
                subject: arg_str(args, "subject")?,
                body_html: arg_str(args, "bodyHtml")?,
                attachments: Vec::new(),
                reply_to_email_id: None,
            };
            if email.to.is_empty() {
                return Err("'to' must contain at least one recipient".to_string());
            }
            let seq = next_seq(state);
            mail_service::send_email(&state.db, &state.app, seq, &account_id, &email).await?;
            Ok(json!({ "ok": true }))
        }
        "mail.move" => {
            let account_id = arg_str(args, "accountId")?;
            require_external(&state.db, &account_id)?;
            let email_id = arg_str(args, "emailId")?;
            let target = arg_str(args, "targetFolder")?;
            let seq = next_seq(state);
            mail_service::move_email(&state.db, &state.app, seq, &account_id, &email_id, &target)?;
            Ok(json!({ "ok": true }))
        }
        "mail.delete" => {
            let account_id = arg_str(args, "accountId")?;
            require_external(&state.db, &account_id)?;
            let email_id = arg_str(args, "emailId")?;
            let seq = next_seq(state);
            mail_service::delete_email(&state.db, &state.app, seq, &account_id, &email_id)?;
            Ok(json!({ "ok": true }))
        }
        "mail.draft" => {
            let account_id = arg_str(args, "accountId")?;
            require_external(&state.db, &account_id)?;
            let email_id = arg_str(args, "emailId")?;
            let instructions = args.get("instructions").and_then(|v| v.as_str()).map(|s| s.to_string());
            let draft = ai_service::draft_reply(&state.db, &state.app, &account_id, &email_id, instructions).await?;
            Ok(json!({ "draft": draft }))
        }
        "mail.classify" => {
            let account_id = arg_str(args, "accountId")?;
            require_external(&state.db, &account_id)?;
            let applied = ai_service::classify_new_mail(&state.db, &state.app, &account_id).await?;
            Ok(json!({ "applied": applied }))
        }
        other => Err(format!("unknown tool '{}'", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_is_stable_and_distinct() {
        assert_eq!(sha256_hex("abc"), sha256_hex("abc"));
        assert_ne!(sha256_hex("abc"), sha256_hex("abd"));
        // known SHA-256("abc")
        assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn ct_eq_matches_normal_eq() {
        assert!(ct_eq(b"hello", b"hello"));
        assert!(!ct_eq(b"hello", b"world"));
        assert!(!ct_eq(b"hello", b"hell")); // length mismatch
    }

    fn mem_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE agent_api_keys (id TEXT PRIMARY KEY, label TEXT, key_sha256 TEXT UNIQUE,
                 created_at TEXT, last_used_at TEXT);",
        ).unwrap();
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn generate_then_verify_roundtrip() {
        let db = mem_db();
        let plaintext = {
            let conn = db.lock().unwrap();
            let (_id, pt) = generate_key(&conn, "test").unwrap();
            pt
        };
        assert!(plaintext.starts_with("am_"));
        assert!(verify_key(&db, &plaintext), "freshly generated key should verify");
        assert!(!verify_key(&db, "am_bogus"), "unknown key must not verify");
        // last_used_at stamped on success
        let used: Option<String> = db.lock().unwrap()
            .query_row("SELECT last_used_at FROM agent_api_keys", [], |r| r.get(0)).unwrap();
        assert!(used.is_some());
    }

    #[test]
    fn revoke_removes_key() {
        let db = mem_db();
        let (id, plaintext) = {
            let conn = db.lock().unwrap();
            generate_key(&conn, "x").unwrap()
        };
        assert!(verify_key(&db, &plaintext));
        { let conn = db.lock().unwrap(); revoke_key(&conn, &id).unwrap(); }
        assert!(!verify_key(&db, &plaintext), "revoked key must not verify");
        assert_eq!(list_keys(&db.lock().unwrap()).unwrap().len(), 0);
    }

    #[test]
    fn list_keys_hides_secrets() {
        let db = mem_db();
        { let conn = db.lock().unwrap(); generate_key(&conn, "label-a").unwrap(); }
        let keys = list_keys(&db.lock().unwrap()).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].label, "label-a");
        // serialized form must not leak a hash/secret field
        let v = serde_json::to_value(&keys[0]).unwrap();
        assert!(v.get("keySha256").is_none());
        assert!(v.get("key_sha256").is_none());
    }
}
