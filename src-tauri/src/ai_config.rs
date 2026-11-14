//! ai_config.rs — Global AI configuration (shared by all accounts) and
//! per-account AI feature toggles.
//!
//! Two model APIs, each optional:
//! - **Language model** — OpenAI-compatible (`/chat/completions`): OpenAI,
//!   OpenRouter, Azure, or a local llama.cpp/Ollama server. Required for
//!   drafting; also answers decisions when no decision model is set.
//! - **Decision model** — TypeSafe Jev's `/systemone` protocol: Jev, Liquid
//!   D1, or a local Laya server. When set, it answers every decision
//!   (junk/phishing/priority, replyability, reference picking).
//!
//! An API counts as set when its base URL is non-empty. Both keys are
//! encrypted at rest with the AES-256-GCM master key used for mail passwords
//! (see crypto.rs).

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
    /// Decision model API root, e.g. `https://api.typesafe.ai/v1`. Empty = unset.
    #[serde(default)]
    pub decision_base_url: String,
    /// Same keep-on-empty contract as `api_key`.
    #[serde(default)]
    pub decision_api_key: String,
    #[serde(default = "default_decision_model")]
    pub decision_model: String,
}

fn default_decision_model() -> String {
    "jev-latest".to_string()
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            enabled: false,
            base_url: String::new(),
            api_key: String::new(),
            classify_model: "gpt-4o-mini".to_string(),
            draft_model: "gpt-4o-mini".to_string(),
            max_tokens: 1024,
            temperature: 0.2,
            decision_base_url: String::new(),
            decision_api_key: String::new(),
            decision_model: default_decision_model(),
        }
    }
}

impl AiConfig {
    /// A language model is configured — drafting is possible.
    pub fn has_language_model(&self) -> bool {
        !self.base_url.trim().is_empty()
    }

    /// A decision model is configured — it answers every decision.
    pub fn has_decision_model(&self) -> bool {
        !self.decision_base_url.trim().is_empty()
    }

    /// Some model can answer decisions (the decision model, else the language
    /// model).
    pub fn can_decide(&self) -> bool {
        self.has_decision_model() || self.has_language_model()
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
    /// Let a model judge whether replies to this account's mail can reach
    /// anyone. Off, the address pattern alone decides (no mail leaves).
    #[serde(default)]
    pub reply_detect: bool,
}

impl Default for AccountAiFeatures {
    fn default() -> Self {
        AccountAiFeatures {
            junk_filter: false,
            priority_detect: false,
            auto_draft: false,
            external_access: false,
            reply_detect: false,
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
            base_url       TEXT NOT NULL DEFAULT '',
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

        -- One model verdict on whether a reply can reach anyone, per email.
        -- Absent row = not judged by a model; the address pattern in
        -- reply_policy.rs decides instead. `no_reply` is the final decision:
        -- the model's when it was confident either way, else the pattern's.
        -- `confidence` is the raw probability that the address is an
        -- unmonitored no-reply mailbox.
        CREATE TABLE IF NOT EXISTS email_reply_verdict (
            email_id   TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            no_reply   INTEGER NOT NULL DEFAULT 0,
            confidence REAL NOT NULL DEFAULT 0,
            source     TEXT NOT NULL DEFAULT '',
            model      TEXT NOT NULL DEFAULT '',
            judged_at  TEXT NOT NULL DEFAULT '',
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
    // Columns added after the table first shipped; the error on an existing
    // column is the expected no-op.
    conn.execute("ALTER TABLE ai_config ADD COLUMN decision_base_url TEXT NOT NULL DEFAULT ''", []).ok();
    conn.execute("ALTER TABLE ai_config ADD COLUMN decision_api_key_enc TEXT NOT NULL DEFAULT ''", []).ok();
    conn.execute("ALTER TABLE ai_config ADD COLUMN decision_model TEXT NOT NULL DEFAULT 'jev-latest'", []).ok();
    conn.execute("ALTER TABLE account_ai_features ADD COLUMN reply_detect INTEGER NOT NULL DEFAULT 0", []).ok();
    conn.execute("ALTER TABLE email_ai_classification ADD COLUMN is_phishing INTEGER NOT NULL DEFAULT 0", []).ok();
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_email_reply_verdict_account ON email_reply_verdict(account_id)",
        [],
    )?;
    // Seed the singleton row if absent (all columns use their defaults).
    conn.execute("INSERT OR IGNORE INTO ai_config (id) VALUES (1)", [])?;
    Ok(())
}

/// Load the global config with the API key decrypted into memory.
pub fn load_ai_config(conn: &Connection) -> Result<AiConfig> {
    let mut stmt = conn.prepare(
        "SELECT enabled, base_url, api_key_enc, classify_model, draft_model, max_tokens, temperature,
                decision_base_url, decision_api_key_enc, decision_model
         FROM ai_config WHERE id = 1",
    )?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let enc: String = row.get(2)?;
        let decision_enc: String = row.get(8)?;
        Ok(AiConfig {
            enabled: row.get::<_, i32>(0)? != 0,
            base_url: row.get(1)?,
            api_key: crypto::decrypt_password(&enc).map_err(crypto_err)?,
            classify_model: row.get(3)?,
            draft_model: row.get(4)?,
            max_tokens: row.get(5)?,
            temperature: row.get(6)?,
            decision_base_url: row.get(7)?,
            decision_api_key: crypto::decrypt_password(&decision_enc).map_err(crypto_err)?,
            decision_model: row.get(9)?,
        })
    } else {
        Ok(AiConfig::default())
    }
}

/// Save the global config, encrypting the API key. An empty `api_key`
/// preserves the existing stored key, so a masked round-trip from the UI
/// (which never receives the real key) doesn't wipe it.
pub fn save_ai_config(conn: &Connection, cfg: &AiConfig) -> Result<()> {
    // `column` is one of two fixed names, never user input.
    let keep_or_encrypt = |plain: &str, column: &str| -> Result<String> {
        if plain.is_empty() {
            Ok(conn
                .query_row(&format!("SELECT {column} FROM ai_config WHERE id = 1"), [], |r| r.get::<_, String>(0))
                .unwrap_or_default())
        } else {
            crypto::encrypt_password(plain).map_err(crypto_err)
        }
    };
    let api_key_enc = keep_or_encrypt(&cfg.api_key, "api_key_enc")?;
    let decision_api_key_enc = keep_or_encrypt(&cfg.decision_api_key, "decision_api_key_enc")?;
    conn.execute(
        "INSERT INTO ai_config (id, enabled, base_url, api_key_enc, classify_model, draft_model, max_tokens, temperature,
                                decision_base_url, decision_api_key_enc, decision_model)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
           enabled = ?1, base_url = ?2, api_key_enc = ?3,
           classify_model = ?4, draft_model = ?5, max_tokens = ?6, temperature = ?7,
           decision_base_url = ?8, decision_api_key_enc = ?9, decision_model = ?10",
        params![
            cfg.enabled as i32, cfg.base_url, api_key_enc,
            cfg.classify_model, cfg.draft_model, cfg.max_tokens, cfg.temperature,
            cfg.decision_base_url, decision_api_key_enc, cfg.decision_model
        ],
    )?;
    Ok(())
}

/// Load a account's feature toggles, defaulting to all-off when no row exists.
pub fn load_account_features(conn: &Connection, account_id: &str) -> Result<AccountAiFeatures> {
    let mut stmt = conn.prepare(
        "SELECT junk_filter, priority_detect, auto_draft, external_access, reply_detect
         FROM account_ai_features WHERE account_id = ?1",
    )?;
    let mut rows = stmt.query(params![account_id])?;
    if let Some(row) = rows.next()? {
        Ok(AccountAiFeatures {
            junk_filter: row.get::<_, i32>(0)? != 0,
            priority_detect: row.get::<_, i32>(1)? != 0,
            auto_draft: row.get::<_, i32>(2)? != 0,
            external_access: row.get::<_, i32>(3)? != 0,
            reply_detect: row.get::<_, i32>(4)? != 0,
        })
    } else {
        Ok(AccountAiFeatures::default())
    }
}

pub fn save_account_features(conn: &Connection, account_id: &str, f: &AccountAiFeatures) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO account_ai_features
         (account_id, junk_filter, priority_detect, auto_draft, external_access, reply_detect)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![account_id, f.junk_filter as i32, f.priority_detect as i32, f.auto_draft as i32, f.external_access as i32, f.reply_detect as i32],
    )?;
    Ok(())
}
