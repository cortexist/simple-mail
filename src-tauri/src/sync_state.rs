//! sync_state.rs — Database tables and helpers for tracking sync state.
//!
//! Tracks IMAP UID validity / UIDNEXT / HIGHESTMODSEQ per folder,
//! CalDAV/CardDAV ctag per collection and etag per resource,
//! so we only ever pull deltas from the server.

use rusqlite::{Connection, Result, params};
use crate::crypto;

/// Map a crypto error string into a rusqlite error for propagation.
fn crypto_err(msg: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some(msg),
    )
}

/// Create sync-related tables (idempotent).
pub fn init_sync_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        -- Per-account mail server settings (IMAP/POP3 + SMTP)
        CREATE TABLE IF NOT EXISTS mail_server_settings (
            account_id       TEXT PRIMARY KEY,
            protocol         TEXT NOT NULL DEFAULT 'imap',  -- 'imap' or 'pop3'
            incoming_server  TEXT NOT NULL DEFAULT '',
            incoming_port    INTEGER NOT NULL DEFAULT 993,
            incoming_username TEXT NOT NULL DEFAULT '',
            incoming_password TEXT NOT NULL DEFAULT '',
            incoming_security TEXT NOT NULL DEFAULT 'ssl',   -- 'ssl','tls','none'
            smtp_server      TEXT NOT NULL DEFAULT '',
            smtp_port        INTEGER NOT NULL DEFAULT 587,
            smtp_username    TEXT NOT NULL DEFAULT '',
            smtp_password    TEXT NOT NULL DEFAULT '',
            smtp_security    TEXT NOT NULL DEFAULT 'tls',
            sync_interval_minutes INTEGER NOT NULL DEFAULT 5,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- Per-account CalDAV settings
        CREATE TABLE IF NOT EXISTS caldav_settings (
            account_id TEXT PRIMARY KEY,
            url        TEXT NOT NULL DEFAULT '',
            username   TEXT NOT NULL DEFAULT '',
            password   TEXT NOT NULL DEFAULT '',
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- Per-account CardDAV settings
        CREATE TABLE IF NOT EXISTS carddav_settings (
            account_id TEXT PRIMARY KEY,
            url        TEXT NOT NULL DEFAULT '',
            username   TEXT NOT NULL DEFAULT '',
            password   TEXT NOT NULL DEFAULT '',
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- IMAP folder sync state: tracks UID validity and delta markers
        CREATE TABLE IF NOT EXISTS imap_folder_state (
            account_id       TEXT NOT NULL,
            folder           TEXT NOT NULL,
            uid_validity     INTEGER NOT NULL DEFAULT 0,
            uid_next         INTEGER NOT NULL DEFAULT 0,
            highest_modseq   INTEGER NOT NULL DEFAULT 0,
            last_sync        TEXT,
            PRIMARY KEY (account_id, folder),
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- Maps local email IDs to IMAP UIDs for delta sync
        CREATE TABLE IF NOT EXISTS imap_uid_map (
            account_id TEXT NOT NULL,
            folder     TEXT NOT NULL,
            uid        INTEGER NOT NULL,
            email_id   TEXT NOT NULL,
            PRIMARY KEY (account_id, folder, uid),
            FOREIGN KEY (email_id)   REFERENCES emails(id) ON DELETE CASCADE,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- CalDAV collection (calendar) sync state
        CREATE TABLE IF NOT EXISTS caldav_collection_state (
            account_id    TEXT NOT NULL,
            collection_url TEXT NOT NULL,
            ctag          TEXT NOT NULL DEFAULT '',
            display_name  TEXT NOT NULL DEFAULT '',
            color         TEXT NOT NULL DEFAULT '#0078d4',
            visible       INTEGER NOT NULL DEFAULT 1,
            calendar_group TEXT NOT NULL DEFAULT 'my',
            last_sync     TEXT,
            PRIMARY KEY (account_id, collection_url),
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- CalDAV resource (event) etag tracking
        CREATE TABLE IF NOT EXISTS caldav_resource_state (
            account_id     TEXT NOT NULL,
            collection_url TEXT NOT NULL,
            resource_url   TEXT NOT NULL,
            etag           TEXT NOT NULL DEFAULT '',
            event_id       TEXT NOT NULL,
            uid            TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (account_id, resource_url),
            FOREIGN KEY (event_id)   REFERENCES calendar_events(id) ON DELETE CASCADE,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- CardDAV collection (address book) sync state
        CREATE TABLE IF NOT EXISTS carddav_collection_state (
            account_id     TEXT NOT NULL,
            collection_url TEXT NOT NULL,
            ctag           TEXT NOT NULL DEFAULT '',
            display_name   TEXT NOT NULL DEFAULT '',
            read_only      INTEGER NOT NULL DEFAULT 0,
            last_sync      TEXT,
            PRIMARY KEY (account_id, collection_url),
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- CardDAV resource (contact) etag tracking
        CREATE TABLE IF NOT EXISTS carddav_resource_state (
            account_id     TEXT NOT NULL,
            collection_url TEXT NOT NULL,
            resource_url   TEXT NOT NULL,
            etag           TEXT NOT NULL DEFAULT '',
            contact_id     TEXT NOT NULL,
            PRIMARY KEY (account_id, resource_url),
            FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        -- Outgoing mail queue (for reliable send-later / offline)
        CREATE TABLE IF NOT EXISTS outbox_queue (
            id           TEXT PRIMARY KEY,
            account_id   TEXT NOT NULL,
            to_addrs     TEXT NOT NULL,  -- JSON array
            cc_addrs     TEXT NOT NULL DEFAULT '[]',
            bcc_addrs    TEXT NOT NULL DEFAULT '[]',
            subject      TEXT NOT NULL DEFAULT '',
            body_html    TEXT NOT NULL DEFAULT '',
            attachments  TEXT NOT NULL DEFAULT '[]',  -- JSON array of paths
            created_at   TEXT NOT NULL,
            status       TEXT NOT NULL DEFAULT 'pending',  -- 'pending','sending','sent','failed'
            error        TEXT,
            retry_count  INTEGER NOT NULL DEFAULT 0,
            message_id   TEXT NOT NULL DEFAULT '',
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );
        ",
    )?;
    Ok(())
}

// ── Mail server settings CRUD ───────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailServerSettings {
    pub protocol: String,
    pub incoming_server: String,
    pub incoming_port: i32,
    pub incoming_username: String,
    pub incoming_password: String,
    pub incoming_security: String,
    pub smtp_server: String,
    pub smtp_port: i32,
    pub smtp_username: String,
    pub smtp_password: String,
    pub smtp_security: String,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_minutes: i32,
}

fn default_sync_interval() -> i32 { 5 }

pub fn save_mail_settings(conn: &Connection, account_id: &str, s: &MailServerSettings) -> Result<()> {
    let enc_incoming = crypto::encrypt_password(&s.incoming_password)
        .map_err(crypto_err)?;
    let enc_smtp = crypto::encrypt_password(&s.smtp_password)
        .map_err(crypto_err)?;
    conn.execute(
        "INSERT OR REPLACE INTO mail_server_settings
         (account_id, protocol, incoming_server, incoming_port, incoming_username, incoming_password, incoming_security,
          smtp_server, smtp_port, smtp_username, smtp_password, smtp_security, sync_interval_minutes)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![account_id, s.protocol, s.incoming_server, s.incoming_port,
                s.incoming_username, enc_incoming, s.incoming_security,
                s.smtp_server, s.smtp_port, s.smtp_username, enc_smtp, s.smtp_security,
                s.sync_interval_minutes],
    )?;
    Ok(())
}

pub fn load_mail_settings(conn: &Connection, account_id: &str) -> Result<Option<MailServerSettings>> {
    let mut stmt = conn.prepare(
        "SELECT protocol, incoming_server, incoming_port, incoming_username, incoming_password, incoming_security,
                smtp_server, smtp_port, smtp_username, smtp_password, smtp_security, sync_interval_minutes
         FROM mail_server_settings WHERE account_id = ?1"
    )?;
    let mut rows = stmt.query(params![account_id])?;
    if let Some(row) = rows.next()? {
        let raw_incoming: String = row.get(4)?;
        let raw_smtp: String = row.get(9)?;
        Ok(Some(MailServerSettings {
            protocol: row.get(0)?,
            incoming_server: row.get(1)?,
            incoming_port: row.get(2)?,
            incoming_username: row.get(3)?,
            incoming_password: crypto::decrypt_password(&raw_incoming).map_err(crypto_err)?,
            incoming_security: row.get(5)?,
            smtp_server: row.get(6)?,
            smtp_port: row.get(7)?,
            smtp_username: row.get(8)?,
            smtp_password: crypto::decrypt_password(&raw_smtp).map_err(crypto_err)?,
            smtp_security: row.get(10)?,
            sync_interval_minutes: row.get::<_, i32>(11).unwrap_or(5),
        }))
    } else {
        Ok(None)
    }
}

// ── CalDAV/CardDAV settings CRUD ────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DavSettings {
    pub url: String,
    pub username: String,
    pub password: String,
}

pub fn save_caldav_settings(conn: &Connection, account_id: &str, s: &DavSettings) -> Result<()> {
    let enc_pw = crypto::encrypt_password(&s.password).map_err(crypto_err)?;
    conn.execute(
        "INSERT OR REPLACE INTO caldav_settings (account_id, url, username, password) VALUES (?1,?2,?3,?4)",
        params![account_id, s.url, s.username, enc_pw],
    )?;
    Ok(())
}

pub fn load_caldav_settings(conn: &Connection, account_id: &str) -> Result<Option<DavSettings>> {
    let mut stmt = conn.prepare("SELECT url, username, password FROM caldav_settings WHERE account_id = ?1")?;
    let mut rows = stmt.query(params![account_id])?;
    if let Some(row) = rows.next()? {
        let raw_pw: String = row.get(2)?;
        Ok(Some(DavSettings {
            url: row.get(0)?,
            username: row.get(1)?,
            password: crypto::decrypt_password(&raw_pw).map_err(crypto_err)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn save_carddav_settings(conn: &Connection, account_id: &str, s: &DavSettings) -> Result<()> {
    let enc_pw = crypto::encrypt_password(&s.password).map_err(crypto_err)?;
    conn.execute(
        "INSERT OR REPLACE INTO carddav_settings (account_id, url, username, password) VALUES (?1,?2,?3,?4)",
        params![account_id, s.url, s.username, enc_pw],
    )?;
    Ok(())
}

pub fn load_carddav_settings(conn: &Connection, account_id: &str) -> Result<Option<DavSettings>> {
    let mut stmt = conn.prepare("SELECT url, username, password FROM carddav_settings WHERE account_id = ?1")?;
    let mut rows = stmt.query(params![account_id])?;
    if let Some(row) = rows.next()? {
        let raw_pw: String = row.get(2)?;
        Ok(Some(DavSettings {
            url: row.get(0)?,
            username: row.get(1)?,
            password: crypto::decrypt_password(&raw_pw).map_err(crypto_err)?,
        }))
    } else {
        Ok(None)
    }
}

// ── IMAP folder state ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct ImapFolderState {
    pub uid_validity: u32,
    pub uid_next: u32,
    pub highest_modseq: u64,
}

pub fn load_imap_folder_state(conn: &Connection, account_id: &str, folder: &str) -> Result<Option<ImapFolderState>> {
    let mut stmt = conn.prepare(
        "SELECT uid_validity, uid_next, highest_modseq FROM imap_folder_state WHERE account_id = ?1 AND folder = ?2"
    )?;
    let mut rows = stmt.query(params![account_id, folder])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ImapFolderState {
            uid_validity: row.get::<_, u32>(0)?,
            uid_next: row.get::<_, u32>(1)?,
            highest_modseq: row.get::<_, i64>(2)? as u64,
        }))
    } else {
        Ok(None)
    }
}

pub fn save_imap_folder_state(conn: &Connection, account_id: &str, folder: &str, state: &ImapFolderState) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO imap_folder_state (account_id, folder, uid_validity, uid_next, highest_modseq, last_sync)
         VALUES (?1,?2,?3,?4,?5, datetime('now'))",
        params![account_id, folder, state.uid_validity, state.uid_next, state.highest_modseq as i64],
    )?;
    Ok(())
}

/// Get all known UIDs for a folder (for delta comparison).
pub fn load_imap_uids(conn: &Connection, account_id: &str, folder: &str) -> Result<Vec<(u32, String)>> {
    let mut stmt = conn.prepare(
        "SELECT uid, email_id FROM imap_uid_map WHERE account_id = ?1 AND folder = ?2 ORDER BY uid"
    )?;
    let rows = stmt.query_map(params![account_id, folder], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn insert_imap_uid(conn: &Connection, account_id: &str, folder: &str, uid: u32, email_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO imap_uid_map (account_id, folder, uid, email_id) VALUES (?1,?2,?3,?4)",
        params![account_id, folder, uid, email_id],
    )?;
    Ok(())
}

pub fn delete_imap_uids(conn: &Connection, account_id: &str, folder: &str, uids: &[u32]) -> Result<()> {
    if uids.is_empty() { return Ok(()); }
    // Delete in batches to avoid huge IN clauses
    for chunk in uids.chunks(500) {
        let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "DELETE FROM imap_uid_map WHERE account_id = ?1 AND folder = ?2 AND uid IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut param_idx = 1;
        stmt.raw_bind_parameter(param_idx, account_id)?; param_idx += 1;
        stmt.raw_bind_parameter(param_idx, folder)?; param_idx += 1;
        for uid in chunk {
            stmt.raw_bind_parameter(param_idx, *uid)?; param_idx += 1;
        }
        stmt.raw_execute()?;
    }
    Ok(())
}
