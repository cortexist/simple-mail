//! Background worker that downloads full message bodies for accounts where
//! the user has enabled offline download. One task runs per enabled account.
//!
//! Design notes:
//! - Each batch opens a fresh IMAP connection (short-lived, matches the rest
//!   of the app). Avoids holding a session during sleeps.
//! - Cancellation is checked before every IMAP call and between batches, and
//!   `tokio::select!` races the fetch against the cancel token so in-flight
//!   FETCHes abort cleanly.
//! - Progress cursor is implicit: we pick rows with `body = ''`. No explicit
//!   "last position" needs to be stored.
//! - Quota accounting matches `get_storage_info`: `SUM(LENGTH(body))` padded
//!   by 20% to approximate SQLite page overhead.
//! - When quota is hit or disk floor is crossed, the worker *pauses* (sleeps
//!   and re-checks) rather than exiting, so raising the quota resumes work
//!   without a restart. It only exits on cancellation.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::imap_client;
use crate::sync_state;

/// UIDs fetched per IMAP round trip. Smaller = faster cancellation, more
/// per-row overhead. Matches the chunk size already used in `fetch_bodies_full_batch`.
const BATCH_SIZE: i64 = 25;
/// Pause between batches — a politeness throttle to avoid provider rate limits.
const BATCH_SLEEP: Duration = Duration::from_millis(500);
/// Pause when there's nothing to do (all bodies present).
const IDLE_SLEEP: Duration = Duration::from_secs(30);
/// Pause when quota or disk-floor is blocking progress; we re-check at this
/// cadence to pick up quota increases.
const PAUSED_SLEEP: Duration = Duration::from_secs(60);
/// Pause after a recoverable error (network blip, IMAP NO).
const RECOVER_SLEEP: Duration = Duration::from_secs(30);

/// Hard absolute floor: pause if free disk space drops below this regardless
/// of quota. Protects against filling the user's disk with *our* downloads
/// when something else also starts eating space.
const DISK_FLOOR_BYTES: u64 = 500 * 1024 * 1024;

/// SQLite page overhead pad for quota accounting (20%). Must match
/// `get_storage_info` in lib.rs.
const QUOTA_OVERHEAD_NUM: u64 = 12;
const QUOTA_OVERHEAD_DEN: u64 = 10;

/// Payload emitted to the frontend after each batch so the UI can update
/// `searchText`, attachment flags, etc. without re-reading the DB.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfflineBodyUpdate {
    pub id: String,
    pub body: String,
    pub preview: String,
    pub has_attachment: bool,
    pub auth_results: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfflineBodiesEvent {
    pub account_id: String,
    pub updates: Vec<OfflineBodyUpdate>,
}

pub fn spawn_worker(
    app: AppHandle,
    db: Arc<Mutex<Connection>>,
    storage_dir: PathBuf,
    account_id: String,
    cancel: CancellationToken,
) {
    // Use Tauri's async runtime rather than `tokio::spawn` directly — the
    // setup callback runs before the tokio runtime context is entered, and
    // calling `tokio::spawn` from there panics.
    tauri::async_runtime::spawn(async move {
        log::info!("offline_worker [{}] started", account_id);
        run_loop(app, db, storage_dir, account_id.clone(), cancel).await;
        log::info!("offline_worker [{}] stopped", account_id);
    });
}

async fn run_loop(
    app: AppHandle,
    db: Arc<Mutex<Connection>>,
    storage_dir: PathBuf,
    account_id: String,
    cancel: CancellationToken,
) {
    loop {
        if cancel.is_cancelled() { return; }

        // 1. Gate: quota set, within quota, disk floor ok.
        let gate = match read_gate(&db, &storage_dir, &account_id) {
            Ok(g) => g,
            Err(e) => {
                log::warn!("offline_worker [{}]: read_gate failed: {}", account_id, e);
                sleep_or_cancel(&cancel, RECOVER_SLEEP).await;
                continue;
            }
        };
        match gate {
            Gate::NoQuota => {
                log::info!("offline_worker [{}]: quota cleared; exiting", account_id);
                return;
            }
            Gate::QuotaHit | Gate::DiskFloor => {
                sleep_or_cancel(&cancel, PAUSED_SLEEP).await;
                continue;
            }
            Gate::Go => {}
        }

        // 2. Find a folder with pending empty-body rows, and its UIDs.
        let (settings, folder, uid_map) = match pick_batch(&db, &account_id) {
            Ok(Some(v)) => v,
            Ok(None) => {
                // Nothing to download right now.
                sleep_or_cancel(&cancel, IDLE_SLEEP).await;
                continue;
            }
            Err(e) => {
                log::warn!("offline_worker [{}]: pick_batch failed: {}", account_id, e);
                sleep_or_cancel(&cancel, RECOVER_SLEEP).await;
                continue;
            }
        };

        let uids: Vec<u32> = uid_map.iter().map(|(_, u)| *u).collect();

        // 3. Connect + fetch. Race the fetch against cancellation so a long
        //    IMAP round trip doesn't block a toggle-off for minutes.
        let mut session = match imap_client::connect(&settings).await {
            Ok(s) => s,
            Err(e) => {
                log::warn!("offline_worker [{}]: connect failed: {}", account_id, e);
                sleep_or_cancel(&cancel, RECOVER_SLEEP).await;
                continue;
            }
        };

        let remote_folders = match imap_client::list_folders(&mut session).await {
            Ok(f) => f,
            Err(e) => {
                log::warn!("offline_worker [{}]: list_folders failed: {}", account_id, e);
                let _ = session.logout().await;
                sleep_or_cancel(&cancel, RECOVER_SLEEP).await;
                continue;
            }
        };
        let imap_name = remote_folders
            .iter()
            .find(|(_, mapped)| *mapped == folder)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| folder.clone());

        let fetched = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = session.logout().await;
                return;
            }
            r = imap_client::fetch_bodies_full_batch(&mut session, &imap_name, &uids) => {
                match r {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!("offline_worker [{}]: fetch failed on {}: {}", account_id, folder, e);
                        let _ = session.logout().await;
                        sleep_or_cancel(&cancel, RECOVER_SLEEP).await;
                        continue;
                    }
                }
            }
        };

        let _ = session.logout().await;

        if fetched.is_empty() {
            // The UIDs we asked for didn't come back. Possibly those
            // messages have been deleted on the server; proactively blank
            // their rows so we don't loop forever on the same folder.
            mark_missing(&db, &folder, &uid_map);
            sleep_or_cancel(&cancel, BATCH_SLEEP).await;
            continue;
        }

        // 4. Persist + build the frontend event payload.
        let updates = match persist(&db, &uid_map, &fetched) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("offline_worker [{}]: persist failed: {}", account_id, e);
                sleep_or_cancel(&cancel, RECOVER_SLEEP).await;
                continue;
            }
        };

        if !updates.is_empty() {
            let payload = OfflineBodiesEvent {
                account_id: account_id.clone(),
                updates,
            };
            if let Err(e) = app.emit("offline-bodies-updated", payload) {
                log::warn!("offline_worker [{}]: emit failed: {}", account_id, e);
            }
        }

        sleep_or_cancel(&cancel, BATCH_SLEEP).await;
    }
}

enum Gate {
    Go,
    NoQuota,
    QuotaHit,
    DiskFloor,
}

fn read_gate(
    db: &Arc<Mutex<Connection>>,
    storage_dir: &PathBuf,
    account_id: &str,
) -> Result<Gate, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    // Quota must still be set; the toggle may also have been flipped off
    // racing with the worker.
    let quota_bytes: Option<u64> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'storage_quota_bytes'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse::<u64>().ok());
    let quota = match quota_bytes {
        Some(q) => q,
        None => return Ok(Gate::NoQuota),
    };

    // Re-check that the per-account toggle is still enabled. If the user
    // flipped it off between our last batch and now, exit cleanly.
    let key = format!("offline_download_{}", account_id);
    let enabled: bool = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .map(|v| v == "1")
        .unwrap_or(false);
    if !enabled {
        return Ok(Gate::NoQuota);
    }

    // Quota accounting (app-wide; matches get_storage_info).
    let used_raw: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(LENGTH(body)), 0) FROM emails",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let used_padded = (used_raw.max(0) as u64)
        .saturating_mul(QUOTA_OVERHEAD_NUM)
        / QUOTA_OVERHEAD_DEN;
    drop(conn);

    if used_padded >= quota {
        return Ok(Gate::QuotaHit);
    }

    let free = fs4::available_space(storage_dir).unwrap_or(0);
    if free < DISK_FLOOR_BYTES {
        return Ok(Gate::DiskFloor);
    }

    Ok(Gate::Go)
}

/// Pick one folder with empty bodies and grab up to BATCH_SIZE UIDs from it,
/// newest first. Returns (settings, folder, [(email_id, uid)]).
fn pick_batch(
    db: &Arc<Mutex<Connection>>,
    account_id: &str,
) -> Result<Option<(sync_state::MailServerSettings, String, Vec<(String, u32)>)>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    let settings = sync_state::load_mail_settings(&conn, account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No mail settings for account".to_string())?;

    // Pick the folder of the newest empty-body message so users see recent
    // email become searchable first.
    let folder: Option<String> = conn
        .query_row(
            "SELECT e.folder
               FROM emails e
               JOIN imap_uid_map m ON m.email_id = e.id AND m.folder = e.folder
              WHERE e.account_id = ?1 AND e.body = ''
              ORDER BY e.date DESC
              LIMIT 1",
            rusqlite::params![account_id],
            |row| row.get(0),
        )
        .ok();
    let folder = match folder {
        Some(f) => f,
        None => return Ok(None),
    };

    let mut stmt = conn
        .prepare(
            "SELECT e.id, m.uid
               FROM emails e
               JOIN imap_uid_map m ON m.email_id = e.id AND m.folder = e.folder
              WHERE e.account_id = ?1 AND e.folder = ?2 AND e.body = ''
              ORDER BY e.date DESC
              LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![account_id, folder, BATCH_SIZE],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let uid_map: Vec<(String, u32)> = rows.flatten().collect();

    if uid_map.is_empty() {
        return Ok(None);
    }
    Ok(Some((settings, folder, uid_map)))
}

/// Persist fetched bodies; returns per-email updates for the frontend event.
fn persist(
    db: &Arc<Mutex<Connection>>,
    uid_map: &[(String, u32)],
    fetched: &[(u32, String, String, String, bool, Vec<imap_client::AttachmentMeta>)],
) -> Result<Vec<OfflineBodyUpdate>, String> {
    let uid_to_id: std::collections::HashMap<u32, &str> = uid_map
        .iter()
        .map(|(id, u)| (*u, id.as_str()))
        .collect();

    let mut conn = db.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(fetched.len());

    for (uid, html, preview, auth, has_attach, attach_meta) in fetched {
        let Some(id) = uid_to_id.get(uid) else { continue };
        tx.execute(
            "UPDATE emails SET body = ?2, preview = ?3, auth_results = ?4, has_attachment = ?5 WHERE id = ?1",
            rusqlite::params![*id, html, preview, auth, *has_attach as i32],
        )
        .ok();
        tx.execute(
            "DELETE FROM email_attachments WHERE email_id = ?1",
            rusqlite::params![*id],
        )
        .ok();
        for (idx, meta) in attach_meta.iter().enumerate() {
            tx.execute(
                "INSERT INTO email_attachments (email_id, idx, filename, mime_type, size) VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![*id, idx as i64, meta.filename, meta.mime_type, meta.size as i64],
            )
            .ok();
        }
        out.push(OfflineBodyUpdate {
            id: id.to_string(),
            body: html.clone(),
            preview: preview.clone(),
            has_attachment: *has_attach,
            auth_results: auth.clone(),
        });
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(out)
}

/// For UIDs that the server didn't return, mark the row so we stop picking
/// it repeatedly. We put a single space in `body` — non-empty, so the
/// `body = ''` selector skips it, but callers can still detect "no real
/// body" via LENGTH(body) == 1 if that ever matters. Cheap and unobtrusive.
fn mark_missing(
    db: &Arc<Mutex<Connection>>,
    _folder: &str,
    uid_map: &[(String, u32)],
) {
    let Ok(mut conn) = db.lock() else { return };
    let Ok(tx) = conn.transaction() else { return };
    for (id, _) in uid_map {
        let _ = tx.execute(
            "UPDATE emails SET body = ' ' WHERE id = ?1 AND body = ''",
            rusqlite::params![id],
        );
    }
    let _ = tx.commit();
}

async fn sleep_or_cancel(cancel: &CancellationToken, dur: Duration) {
    tokio::select! {
        _ = tokio::time::sleep(dur) => {}
        _ = cancel.cancelled() => {}
    }
}
