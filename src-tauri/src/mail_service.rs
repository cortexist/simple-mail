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
use tauri::{AppHandle, Emitter};

use crate::{events, imap_client, smtp_client, sync_state};

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

        // Scope the lookup to what this account can actually see. The row is
        // fetched by id alone otherwise, which lets any caller holding an id
        // mutate any message — reachable across mailboxes now that a shared
        // login puts siblings in one id space, and reachable from MCP, where
        // the caller is an external agent granted access to one account only.
        if !is_visible(&conn, &account_id, &email_id)? {
            return Err("Email not found".to_string());
        }

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

        let settings = sync_state::load_mail_settings(&conn, &crate::login::resolve(&conn, &account_id).map_err(|e| e.to_string())?)
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

        // Same scope guard as move_email — deleting is the more destructive of
        // the two, so an unscoped id lookup here is the worse hole.
        if !is_visible(&conn, &account_id, &email_id)? {
            return Err("Email not found".to_string());
        }

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

        let settings = sync_state::load_mail_settings(&conn, &crate::login::resolve(&conn, &account_id).map_err(|e| e.to_string())?)
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

/// Send an email via SMTP, then APPEND it to the Sent folder over IMAP.
///
/// Queues the message first (persisted record with a stable Message-ID) so a
/// crash between SMTP submission and the status update can be reconciled against
/// the Sent folder on the next startup. On SMTP failure the outbox row is reset
/// to `pending` for retry and `mail:send-failed` is emitted. This is `async` and
/// awaits inline (unlike move/delete it has no optimistic-UI spawn).
pub async fn send_email(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle,
    seq: u64,
    account_id: &str,
    email: &smtp_client::OutboundEmail,
) -> Result<(), String> {
    let (settings, from_email, from_name) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        // SMTP credentials belong to the login; the From address below comes
        // from the account. That split is what lets a mailbox send as its own
        // address over a login it shares with a sibling.
        let login = crate::login::resolve(&conn, account_id).map_err(|e| e.to_string())?;
        let settings = sync_state::load_mail_settings(&conn, &login)
            .map_err(|e| e.to_string())?
            .ok_or("No mail settings configured")?;

        let (name, email_addr): (String, String) = conn
            .query_row(
                "SELECT name, email FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;

        (settings, email_addr, name)
    };

    // Always queue first so there's a persisted record with a stable Message-ID.
    // If we crash between SMTP submission and the status update, reconcile_sending
    // can verify against the Sent folder on the next startup.
    let (outbox_id, message_id) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        smtp_client::queue_email(&conn, account_id, email, &from_email)
            .map_err(|e| e.to_string())?
    };
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE outbox_queue SET status = 'sending' WHERE id = ?1",
            rusqlite::params![outbox_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let raw_message = match smtp_client::send_email(&settings, &from_email, &from_name, email, Some(&message_id)).await {
        Ok(raw) => {
            let conn = db.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE outbox_queue SET status = 'sent' WHERE id = ?1",
                rusqlite::params![outbox_id],
            )
            .map_err(|e| e.to_string())?;
            raw
        }
        Err(e) => {
            log::error!("SMTP send failed for account {}: {}", account_id, e);
            let conn = db.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE outbox_queue SET status = 'pending', error = ?2, retry_count = retry_count + 1 WHERE id = ?1",
                rusqlite::params![outbox_id, e.to_string()],
            )
            .map_err(|e| e.to_string())?;
            let err_msg = format!("Send failed, queued for retry: {}", e);
            events::emit_mail_send_failed(app, account_id, &err_msg, seq);
            return Err(err_msg);
        }
    };

    // Save to Sent folder via IMAP APPEND. Best-effort: the message has been
    // SUBMITTED — whatever happens below must not leave the send unresolved.
    match imap_client::connect(&settings).await {
        Ok(mut session) => {
            let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();
            for (name, mapped) in &remote_folders {
                log::info!("send_email: remote folder '{}' → mapped '{}'", name, mapped);
            }
            let sent_imap_name = remote_folders.iter()
                .find(|(_, mapped)| mapped == "sent")
                .map(|(name, _)| name.clone());

            // If no Sent folder found, create one
            let sent_folder = match sent_imap_name {
                Some(name) => name,
                None => {
                    let folder_name = "Sent";
                    log::info!("No Sent folder found — creating '{}'", folder_name);
                    if let Err(e) = session.create(folder_name).await {
                        log::error!("Failed to create Sent folder: {}", e);
                    }
                    folder_name.to_string()
                }
            };

            if let Err(e) = imap_client::append_to_mailbox(&mut session, &sent_folder, &raw_message, "(\\Seen)").await {
                log::error!("Failed to save sent message to '{}': {}", sent_folder, e);
            } else {
                log::info!("Sent message saved to IMAP folder '{}'", sent_folder);
            }
            let _ = session.logout().await;
        }
        Err(e) => {
            log::error!("IMAP connect failed when saving to Sent for account {}: {}", account_id, e);
        }
    }

    // Emitted unconditionally once SMTP has accepted the message. This event
    // is what stops the compose spinner and clears the optimistic Sent
    // placeholder — gating it on the append succeeding (as before) meant an
    // IMAP hiccup here left the UI spinning forever over a mail that had in
    // fact been sent.
    let _ = app.emit("mail:sent", account_id);

    Ok(())
}

/// Push a locally saved draft to the server's Drafts mailbox so it can be
/// picked up from other devices (Outlook-style cross-device drafts).
///
/// APPENDs a fresh copy stamped with a new `<uuid@domain>` Message-ID (written
/// to the local row first, so the sync dedup maps the server copy back onto
/// it), records the new UID, then removes the previous server copy. The local
/// DB is already saved when this runs — a failure here only means the server
/// copy is stale, and the next save retries. Accounts without mail settings
/// (local/demo) are a silent no-op.
pub async fn upload_draft(
    db: &Arc<Mutex<Connection>>,
    account_id: &str,
    email_id: &str,
) -> Result<(), String> {
    use futures::TryStreamExt;

    let (settings, from_name, from_email, old_mid, draft_json, old_uids) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let login = crate::login::resolve(&conn, account_id).map_err(|e| e.to_string())?;
        let Some(settings) = sync_state::load_mail_settings(&conn, &login).map_err(|e| e.to_string())? else {
            return Ok(());
        };
        let (from_name, from_email, old_mid, draft_json): (String, String, String, String) = conn
            .query_row(
                "SELECT from_name, from_email, COALESCE(message_id, ''), draft_data
                 FROM emails
                 WHERE id = ?1 AND account_id = ?2 AND folder = 'drafts' AND draft_data IS NOT NULL",
                rusqlite::params![email_id, login.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|_| "Draft not found".to_string())?;
        let old_uids: Vec<u32> = {
            let mut stmt = conn
                .prepare("SELECT uid FROM imap_uid_map WHERE email_id = ?1 AND folder = 'drafts'")
                .map_err(|e| e.to_string())?;
            let collected: Vec<u32> = stmt
                .query_map(rusqlite::params![email_id], |row| row.get(0))
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            collected
        };
        (settings, from_name, from_email, old_mid, draft_json, old_uids)
    };

    // Assemble the outbound form from the stored composer state. Recipients
    // are display strings that may hold half-typed entries — those stay in
    // the local draft but are dropped from the MIME copy. Attachments are
    // local paths; missing files are skipped rather than failing the upload.
    let data: serde_json::Value = serde_json::from_str(&draft_json).map_err(|e| e.to_string())?;
    let field = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let attachments: Vec<smtp_client::AttachmentInfo> = data
        .get("attachments")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let name = a.get("name")?.as_str()?.to_string();
                    let path = a.get("path")?.as_str()?.to_string();
                    if std::path::Path::new(&path).exists() {
                        Some(smtp_client::AttachmentInfo { name, path })
                    } else {
                        log::warn!("upload_draft [{}]: attachment missing on disk, skipped: {}", email_id, path);
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let outbound = smtp_client::OutboundEmail {
        to: smtp_client::parse_recipient_list(&field("to")),
        cc: smtp_client::parse_recipient_list(&field("cc")),
        bcc: smtp_client::parse_recipient_list(&field("bcc")),
        subject: field("subject"),
        body_html: field("body"),
        attachments,
    };

    // Fresh Message-ID per save: the previous server copy is replaced, and
    // other devices see the replacement as a new message (their stale row
    // drops off when its UID disappears). Written to the local row BEFORE the
    // APPEND so a sync racing this upload still dedups onto our row.
    let domain = from_email.rsplit('@').next().filter(|d| !d.is_empty()).unwrap_or("localhost");
    let new_mid = format!("<{}@{}>", uuid::Uuid::new_v4(), domain);
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE emails SET message_id = ?1 WHERE id = ?2",
            rusqlite::params![new_mid, email_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let message = smtp_client::build_message(&from_email, &from_name, &outbound, Some(&new_mid))
        .await
        .map_err(|e| e.to_string())?;
    let raw = message.formatted();

    let mut session = imap_client::connect(&settings)
        .await
        .map_err(|e| format!("IMAP connect failed: {}", e))?;

    let result = async {
        let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();
        let drafts_folder = match remote_folders.iter().find(|(_, mapped)| mapped == "drafts") {
            Some((name, _)) => name.clone(),
            None => {
                log::info!("upload_draft: no Drafts folder found — creating 'Drafts'");
                if let Err(e) = session.create("Drafts").await {
                    log::error!("Failed to create Drafts folder: {}", e);
                }
                "Drafts".to_string()
            }
        };

        imap_client::append_to_mailbox(&mut session, &drafts_folder, &raw, "(\\Seen \\Draft)")
            .await
            .map_err(|e| e.to_string())?;

        // Learn the appended copy's UID so sync recognizes it as already ours.
        let new_uid = imap_client::find_uid_by_message_id(&mut session, &drafts_folder, &new_mid)
            .await
            .ok()
            .flatten();
        if let Some(uid) = new_uid {
            if let Ok(conn) = db.lock() {
                sync_state::insert_imap_uid(&conn, account_id, "drafts", uid, email_id).ok();
            }
        } else {
            log::warn!("upload_draft [{}]: appended copy's UID not found (sync will dedup by Message-ID)", email_id);
        }

        // Replace semantics: drop the previous server copy (tracked UIDs, plus
        // a Message-ID search in case a UID was never learned).
        let mut stale: Vec<u32> = old_uids.clone();
        if !old_mid.trim().is_empty() && old_mid != new_mid {
            if let Ok(Some(uid)) = imap_client::find_uid_by_message_id(&mut session, &drafts_folder, &old_mid).await {
                if !stale.contains(&uid) {
                    stale.push(uid);
                }
            }
        }
        stale.retain(|u| Some(*u) != new_uid);
        if !stale.is_empty() {
            session.select(&drafts_folder).await.map_err(|e| e.to_string())?;
            let uid_set = stale.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");
            session
                .uid_store(&uid_set, "+FLAGS (\\Deleted)")
                .await
                .map_err(|e| e.to_string())?
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| e.to_string())?;
            session.expunge().await.map_err(|e| e.to_string())?.try_collect::<Vec<_>>().await.ok();
            if let Ok(conn) = db.lock() {
                sync_state::delete_imap_uids(&conn, account_id, "drafts", &stale).ok();
            }
        }

        log::info!(
            "upload_draft [{}]: uploaded to '{}' (uid={:?}, replaced {} old copies)",
            email_id, drafts_folder, new_uid, stale.len()
        );
        Ok::<(), String>(())
    }
    .await;

    let _ = session.logout().await;
    result
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

/// Whether `email_id` is within `account_id`'s view — same scope as [`get`],
/// but against an already-held connection, so a caller holding the `db` lock
/// can check without deadlocking on it.
///
/// Guards the mutating operations. They look their row up by id alone, and ids
/// are UUIDs from a shared space: every account on one login draws from the
/// same `emails` table, so an id belonging to a sibling mailbox would otherwise
/// be perfectly usable.
fn is_visible(conn: &Connection, account_id: &str, email_id: &str) -> Result<bool, String> {
    let view = crate::login::view_of(conn, account_id).map_err(|e| e.to_string())?;
    let login_id = view.login.as_str().to_string();

    let sql = format!(
        "SELECT 1 FROM emails WHERE id = ?1 AND account_id = ?2{}",
        crate::login::mailbox_clause(view.mailbox.as_deref(), 3),
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&email_id, &login_id];
    if let Some(addr) = view.mailbox.as_ref() {
        params.push(addr);
    }
    Ok(conn
        .query_row(&sql, params.as_slice(), |_| Ok(()))
        .optional()
        .map_err(|e| e.to_string())?
        .is_some())
}

/// Search the emails **visible to an account** by a free-text query over
/// subject/sender/preview/body. Returns up to `limit` most-recent matches.
///
/// Scoped exactly like the account's own message list: the login owns the data,
/// and a mailbox account sees only mail for its address. External agents reach
/// this through MCP, so without the scope a mailbox's agent could read its
/// sibling's mail — and a mailbox account would find nothing at all, since its
/// mail is keyed to the login rather than to it.
#[allow(dead_code)]
pub fn search(
    db: &Arc<Mutex<Connection>>,
    account_id: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<EmailSummary>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let view = crate::login::view_of(&conn, account_id).map_err(|e| e.to_string())?;
    let login_id = view.login.as_str().to_string();
    let like = format!("%{}%", query.trim());

    let sql = format!(
        "SELECT id, from_name, from_email, subject, date, folder, substr(preview, 1, 200)
         FROM emails
         WHERE account_id = ?1
           AND (subject LIKE ?2 OR from_name LIKE ?2 OR from_email LIKE ?2 OR preview LIKE ?2 OR body LIKE ?2){}
         ORDER BY date DESC
         LIMIT ?3",
        crate::login::mailbox_clause(view.mailbox.as_deref(), 4),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&login_id, &like, &limit];
    if let Some(addr) = view.mailbox.as_ref() {
        params.push(addr);
    }
    let rows = stmt.query_map(params.as_slice(), |row| {
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

/// Load a single email's full content, if it is visible to this account.
///
/// Scoped like [`search`]: `None` for a message that belongs to a sibling
/// mailbox on the same login. Email ids are guessable UUIDs from a shared id
/// space, so this check is what stops an external agent reading across
/// mailboxes by id.
#[allow(dead_code)]
pub fn get(
    db: &Arc<Mutex<Connection>>,
    account_id: &str,
    email_id: &str,
) -> Result<Option<EmailFull>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let view = crate::login::view_of(&conn, account_id).map_err(|e| e.to_string())?;
    let login_id = view.login.as_str().to_string();

    let sql = format!(
        "SELECT id, from_name, from_email, subject, preview, body, folder, date, COALESCE(message_id,''), is_read
         FROM emails WHERE id = ?1 AND account_id = ?2{}",
        crate::login::mailbox_clause(view.mailbox.as_deref(), 3),
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&email_id, &login_id];
    if let Some(addr) = view.mailbox.as_ref() {
        params.push(addr);
    }
    conn.query_row(
        &sql,
        params.as_slice(),
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
