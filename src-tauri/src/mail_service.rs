//! mail_service.rs — Service layer for mail mutations and reads.
//!
//! These are plain functions (not `#[tauri::command]`) so every surface can
//! share them: the Tauri command wrappers (UI), the internal agent harness
//! (in-process, no auth), and the external MCP server (Phase 4). Each takes the
//! shared DB handle + an `AppHandle` (to emit the same push echoes) + a `seq`
//! computed by the caller via `AppState::next_seq()`.
//!
//! `move_email` / `delete_email` are behavior-preserving extractions of the
//! former command bodies. `search` / `get` are new reads used by classification
//! candidate selection (P2) and the MCP tools (P4).

use std::sync::{Arc, Mutex};
use rusqlite::{Connection, OptionalExtension};
use tauri::AppHandle;

use crate::{events, imap_client, sync_state};

/// Move an email to `target_folder`. Applies the local DB change up front, then
/// performs the IMAP move in the background; on failure the folder is rolled
/// back. Emits `mail:email-folder-changed` with the applied folder.
pub fn move_email(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    seq: u64,
    account_id: &str,
    email_id: &str,
    target_folder: &str,
) -> Result<(), String> {
    let account_id = account_id.to_string();
    let email_id = email_id.to_string();
    let target_folder = target_folder.to_string();

    let (current_folder, uid, message_id, settings) = {
        let conn = db.lock().map_err(|e| e.to_string())?;

        let (current_folder, message_id): (String, String) = conn
            .query_row(
                "SELECT folder, COALESCE(message_id, '') FROM emails WHERE id = ?1",
                rusqlite::params![email_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Email not found".to_string())?;

        let uid: Option<u32> = conn
            .query_row(
                "SELECT uid FROM imap_uid_map WHERE email_id = ?1",
                rusqlite::params![email_id],
                |row| row.get(0),
            )
            .ok();

        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?;

        (current_folder, uid, message_id, settings)
    };

    if current_folder == target_folder {
        return Ok(());
    }

    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE emails SET folder = ?1 WHERE id = ?2",
            rusqlite::params![target_folder, email_id],
        ).map_err(|e| e.to_string())?;
        // Drop the stale source-folder UID mapping now; the IMAP task will
        // re-insert the destination mapping if the move succeeds. If IMAP
        // fails we leave the row mapping-less and the next sync rebuilds it.
        conn.execute("DELETE FROM imap_uid_map WHERE email_id = ?1", rusqlite::params![email_id]).ok();
    }

    let db_arc = db.clone();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        let mut applied_folder = target_folder.clone();
        if let (Some(uid), Some(settings)) = (uid, settings) {
            let outcome = async {
                let mut session = imap_client::connect(&settings).await?;
                let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();

                let source_imap = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == current_folder)
                    .map(|(name, _)| name.clone());
                let target_imap = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == target_folder)
                    .map(|(name, _)| name.clone());

                let res = if let (Some(source), Some(target)) = (source_imap, target_imap) {
                    imap_client::move_message(&mut session, &source, &target, uid, &message_id).await
                } else {
                    log::warn!("Could not find IMAP folder mapping for move: {} -> {}", current_folder, target_folder);
                    Ok(None)
                };
                let _ = session.logout().await;
                res
            }.await;

            match outcome {
                Ok(new_uid) => {
                    if let Some(new_uid) = new_uid {
                        if let Ok(conn) = db_arc.lock() {
                            sync_state::insert_imap_uid(&conn, &account_id, &target_folder, new_uid, &email_id).ok();
                        }
                    }
                }
                Err(e) => {
                    log::error!("IMAP move failed for UID {} from {} to {}: {}", uid, current_folder, target_folder, e);
                    applied_folder = current_folder.clone();
                    if let Ok(conn) = db_arc.lock() {
                        let _ = conn.execute(
                            "UPDATE emails SET folder = ?1 WHERE id = ?2",
                            rusqlite::params![applied_folder, email_id],
                        );
                    }
                }
            }
        }
        events::emit_email_folder_changed(&app, &account_id, &email_id, &applied_folder, seq);
    });

    Ok(())
}

/// Delete an email: soft-delete (move to "deleted") or permanent if already
/// there. Applies the local DB change up front; the IMAP move/expunge runs in
/// the background. Soft delete emits `mail:email-folder-changed`; permanent
/// delete emits `mail:email-removed`.
pub fn delete_email(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    seq: u64,
    account_id: &str,
    email_id: &str,
) -> Result<(), String> {
    let account_id = account_id.to_string();
    let email_id = email_id.to_string();

    let (folder, uid, message_id, settings) = {
        let conn = db.lock().map_err(|e| e.to_string())?;

        let (folder, message_id): (String, String) = conn
            .query_row(
                "SELECT folder, COALESCE(message_id, '') FROM emails WHERE id = ?1",
                rusqlite::params![email_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Email not found".to_string())?;

        let uid: Option<u32> = conn
            .query_row(
                "SELECT uid FROM imap_uid_map WHERE email_id = ?1",
                rusqlite::params![email_id],
                |row| row.get(0),
            )
            .ok();

        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?;

        (folder, uid, message_id, settings)
    };

    let permanent = folder == "deleted";
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        if permanent {
            conn.execute("DELETE FROM email_recipients WHERE email_id = ?1", rusqlite::params![email_id]).ok();
            conn.execute("DELETE FROM imap_uid_map WHERE email_id = ?1", rusqlite::params![email_id]).ok();
            conn.execute("DELETE FROM emails WHERE id = ?1", rusqlite::params![email_id])
                .map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "UPDATE emails SET folder = 'deleted' WHERE id = ?1",
                rusqlite::params![email_id],
            ).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM imap_uid_map WHERE email_id = ?1", rusqlite::params![email_id]).ok();
        }
    }

    let db_arc = db.clone();
    let app = app.clone();
    let folder_clone = folder.clone();

    tauri::async_runtime::spawn(async move {
        let mut soft_delete_failed = false;

        if let (Some(uid), Some(settings)) = (uid, settings) {
            let outcome = async {
                let mut session = imap_client::connect(&settings).await?;
                let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();
                let source_imap = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == folder_clone)
                    .map(|(name, _)| name.clone());

                let res: anyhow::Result<Option<u32>> = if let Some(source) = source_imap {
                    if permanent {
                        use futures::TryStreamExt;
                        session.select(&source).await?;
                        let uid_str = uid.to_string();
                        session.uid_store(&uid_str, "+FLAGS (\\Deleted)").await?
                            .try_collect::<Vec<_>>().await?;
                        session.expunge().await?.try_collect::<Vec<_>>().await?;
                        Ok(None)
                    } else {
                        let trash_imap = remote_folders.iter()
                            .find(|(_, mapped)| mapped == "deleted")
                            .map(|(name, _)| name.clone());
                        if let Some(trash) = trash_imap {
                            imap_client::move_message(&mut session, &source, &trash, uid, &message_id).await
                        } else {
                            log::warn!("No Trash folder found on IMAP server, flagging as \\Deleted");
                            use futures::TryStreamExt;
                            session.select(&source).await?;
                            let uid_str = uid.to_string();
                            session.uid_store(&uid_str, "+FLAGS (\\Deleted)").await?
                                .try_collect::<Vec<_>>().await?;
                            session.expunge().await?.try_collect::<Vec<_>>().await?;
                            Ok(None)
                        }
                    }
                } else {
                    Ok(None)
                };
                let _ = session.logout().await;
                res
            }.await;

            match outcome {
                Ok(new_uid) => {
                    if let Some(new_uid) = new_uid {
                        if let Ok(conn) = db_arc.lock() {
                            sync_state::insert_imap_uid(&conn, &account_id, "deleted", new_uid, &email_id).ok();
                        }
                    }
                }
                Err(e) => {
                    log::error!("IMAP delete/move-to-trash failed for UID {}: {}", uid, e);
                    if !permanent {
                        if let Ok(conn) = db_arc.lock() {
                            let _ = conn.execute(
                                "UPDATE emails SET folder = ?1 WHERE id = ?2",
                                rusqlite::params![folder_clone, email_id],
                            );
                        }
                        soft_delete_failed = true;
                    }
                }
            }
        }

        if permanent {
            events::emit_email_removed(&app, &account_id, vec![email_id], seq);
        } else if soft_delete_failed {
            events::emit_email_folder_changed(&app, &account_id, &email_id, &folder_clone, seq);
        } else {
            events::emit_email_folder_changed(&app, &account_id, &email_id, "deleted", seq);
        }
    });

    Ok(())
}

// ── Reads (used by P2 classification + P4 MCP) ──────────

/// Compact email summary for search results / classification candidates.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct EmailSummary {
    pub id: String,
    pub from_name: String,
    pub from_email: String,
    pub subject: String,
    pub date: String,
    pub folder: String,
    pub snippet: String,
}

/// Full email content for drafting context / MCP get.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct EmailFull {
    pub id: String,
    pub from_name: String,
    pub from_email: String,
    pub subject: String,
    pub preview: String,
    pub body: String,
    pub folder: String,
    pub date: String,
    pub message_id: String,
    pub is_read: bool,
}

/// Search an account's emails by a free-text query over subject/sender/preview/
/// body. Returns up to `limit` most-recent matches as compact summaries.
#[allow(dead_code)]
pub fn search(
    db: &Arc<Mutex<Connection>>,
    account_id: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<EmailSummary>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let like = format!("%{}%", query.trim());
    let mut stmt = conn.prepare(
        "SELECT id, from_name, from_email, subject, date, folder, substr(preview, 1, 200)
         FROM emails
         WHERE account_id = ?1
           AND (subject LIKE ?2 OR from_name LIKE ?2 OR from_email LIKE ?2 OR preview LIKE ?2 OR body LIKE ?2)
         ORDER BY date DESC
         LIMIT ?3",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params![account_id, like, limit], |row| {
        Ok(EmailSummary {
            id: row.get(0)?,
            from_name: row.get(1)?,
            from_email: row.get(2)?,
            subject: row.get(3)?,
            date: row.get(4)?,
            folder: row.get(5)?,
            snippet: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Load a single email's full content for an account.
#[allow(dead_code)]
pub fn get(
    db: &Arc<Mutex<Connection>>,
    account_id: &str,
    email_id: &str,
) -> Result<Option<EmailFull>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, from_name, from_email, subject, preview, body, folder, date, COALESCE(message_id,''), is_read
         FROM emails WHERE id = ?1 AND account_id = ?2",
        rusqlite::params![email_id, account_id],
        |row| Ok(EmailFull {
            id: row.get(0)?,
            from_name: row.get(1)?,
            from_email: row.get(2)?,
            subject: row.get(3)?,
            preview: row.get(4)?,
            body: row.get(5)?,
            folder: row.get(6)?,
            date: row.get(7)?,
            message_id: row.get(8)?,
            is_read: row.get::<_, i32>(9)? != 0,
        }),
    ).optional().map_err(|e| e.to_string())
}
