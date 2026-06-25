//! ai_config.rs — Global LLM configuration (one shared API for all accounts)
//! and per-account AI feature toggles.
//!
//! The model API is OpenAI-compatible (`/chat/completions`); swap providers
//! (OpenAI, OpenRouter, Azure, or a local llama.cpp/Ollama server) via
//! `base_url`. The API key is encrypted at rest with the same AES-256-GCM
//! master key used for mail passwords (see crypto.rs).

use rusqlite::{Connection, Result, params};
use crate::crypto;

/// Map a crypto error string into a rusqlite error for propagation.
fn crypto_err(msg: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some(msg),
    )
}

/// Global, singleton LLM configuration shared by every account.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub enabled: bool,
    pub base_url: String,
    /// Plaintext only in memory; encrypted at rest. Empty string = unset, and
    /// on save an empty value means "keep the existing stored key".
    #[serde(default)]
    pub api_key: String,
    pub classify_model: String,
    pub draft_model: String,
    pub max_tokens: i32,
    pub temperature: f64,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            enabled: false,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            classify_model: "gpt-4o-mini".to_string(),
            draft_model: "gpt-4o-mini".to_string(),
            max_tokens: 1024,
            temperature: 0.2,
        }
    }
}

/// Per-account AI feature toggles. All default off.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountAiFeatures {
    pub junk_filter: bool,
    pub priority_detect: bool,
    pub auto_draft: bool,
    pub external_access: bool,
}

impl Default for AccountAiFeatures {
    fn default() -> Self {
        AccountAiFeatures {
            junk_filter: false,
            priority_detect: false,
            auto_draft: false,
            external_access: false,
        }
    }
}

/// Create AI tables (idempotent) and seed the singleton config row.
pub fn init_ai_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        -- Singleton global LLM config (one API for all accounts).
        CREATE TABLE IF NOT EXISTS ai_config (
            id             INTEGER PRIMARY KEY CHECK (id = 1),
            enabled        INTEGER NOT NULL DEFAULT 0,
            base_url       TEXT NOT NULL DEFAULT 'https://api.openai.com/v1',
            api_key_enc    TEXT NOT NULL DEFAULT '',
            classify_model TEXT NOT NULL DEFAULT 'gpt-4o-mini',
            draft_model    TEXT NOT NULL DEFAULT 'gpt-4o-mini',
            max_tokens     INTEGER NOT NULL DEFAULT 1024,
            temperature    REAL NOT NULL DEFAULT 0.2
        );

        -- Per-account AI feature toggles (absent row = all off).
        CREATE TABLE IF NOT EXISTS account_ai_features (
            account_id      TEXT PRIMARY KEY,
            junk_filter     INTEGER NOT NULL DEFAULT 0,
            priority_detect INTEGER NOT NULL DEFAULT 0,
            auto_draft      INTEGER NOT NULL DEFAULT 0,
            external_access INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- One LLM verdict per email. Presence of a row means \"already
        -- classified\" — the candidate query selects inbox emails NOT in this
        -- table, so re-sync never reclassifies. Verdicts are advisory: applying
        -- them (move to junk / flip is_focused) respects user manual overrides.
        CREATE TABLE IF NOT EXISTS email_ai_classification (
            email_id      TEXT PRIMARY KEY,
            account_id    TEXT NOT NULL,
            is_junk       INTEGER NOT NULL DEFAULT 0,
            is_priority   INTEGER NOT NULL DEFAULT 0,
            reason        TEXT NOT NULL DEFAULT '',
            model         TEXT NOT NULL DEFAULT '',
            classified_at TEXT NOT NULL DEFAULT '',
            seq           INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (email_id)   REFERENCES emails(id)   ON DELETE CASCADE,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- App keys for the external MCP server. Only the SHA-256 of each key is
        -- stored; the plaintext is shown to the user exactly once at creation.
        CREATE TABLE IF NOT EXISTS agent_api_keys (
            id           TEXT PRIMARY KEY,
            label        TEXT NOT NULL DEFAULT '',
            key_sha256   TEXT NOT NULL UNIQUE,
            created_at   TEXT NOT NULL DEFAULT '',
            last_used_at TEXT
        );
        ",
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_email_ai_classification_account ON email_ai_classification(account_id)",
        [],
    )?;
    // Seed the singleton row if absent (all columns use their defaults).
    conn.execute("INSERT OR IGNORE INTO ai_config (id) VALUES (1)", [])?;
    Ok(())
}

/// Load the global config with the API key decrypted into memory.
pub fn load_ai_config(conn: &Connection) -> Result<AiConfig> {
    let mut stmt = conn.prepare(
        "SELECT enabled, base_url, api_key_enc, classify_model, draft_model, max_tokens, temperature
         FROM ai_config WHERE id = 1",
    )?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let enc: String = row.get(2)?;
        Ok(AiConfig {
            enabled: row.get::<_, i32>(0)? != 0,
            base_url: row.get(1)?,
            api_key: crypto::decrypt_password(&enc).map_err(crypto_err)?,
            classify_model: row.get(3)?,
            draft_model: row.get(4)?,
            max_tokens: row.get(5)?,
            temperature: row.get(6)?,
        })
    } else {
        Ok(AiConfig::default())
    }
}

/// Save the global config, encrypting the API key. An empty `api_key`
/// preserves the existing stored key, so a masked round-trip from the UI
/// (which never receives the real key) doesn't wipe it.
pub fn save_ai_config(conn: &Connection, cfg: &AiConfig) -> Result<()> {
    let api_key_enc = if cfg.api_key.is_empty() {
        conn.query_row("SELECT api_key_enc FROM ai_config WHERE id = 1", [], |r| r.get::<_, String>(0))
            .unwrap_or_default()
    } else {
        crypto::encrypt_password(&cfg.api_key).map_err(crypto_err)?
    };
    conn.execute(
        "INSERT INTO ai_config (id, enabled, base_url, api_key_enc, classify_model, draft_model, max_tokens, temperature)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           enabled = ?1, base_url = ?2, api_key_enc = ?3,
           classify_model = ?4, draft_model = ?5, max_tokens = ?6, temperature = ?7",
        params![
            cfg.enabled as i32, cfg.base_url, api_key_enc,
            cfg.classify_model, cfg.draft_model, cfg.max_tokens, cfg.temperature
        ],
    )?;
    Ok(())
}

/// Load a account's feature toggles, defaulting to all-off when no row exists.
pub fn load_account_features(conn: &Connection, account_id: &str) -> Result<AccountAiFeatures> {
    let mut stmt = conn.prepare(
        "SELECT junk_filter, priority_detect, auto_draft, external_access
         FROM account_ai_features WHERE account_id = ?1",
    )?;
    let mut rows = stmt.query(params![account_id])?;
    if let Some(row) = rows.next()? {
        Ok(AccountAiFeatures {
            junk_filter: row.get::<_, i32>(0)? != 0,
            priority_detect: row.get::<_, i32>(1)? != 0,
            auto_draft: row.get::<_, i32>(2)? != 0,
            external_access: row.get::<_, i32>(3)? != 0,
        })
    } else {
        Ok(AccountAiFeatures::default())
    }
}

pub fn save_account_features(conn: &Connection, account_id: &str, f: &AccountAiFeatures) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO account_ai_features
         (account_id, junk_filter, priority_detect, auto_draft, external_access)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![account_id, f.junk_filter as i32, f.priority_detect as i32, f.auto_draft as i32, f.external_access as i32],
    )?;
    Ok(())
}
