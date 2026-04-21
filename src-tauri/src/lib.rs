mod db;
mod sync_state;
mod crypto;
mod identity;
mod imap_client;
mod smtp_client;
mod caldav_client;
mod carddav_client;
mod autodiscover;
pub mod dav_server;

use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use log::LevelFilter;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

// ── Serde models (match the TypeScript types) ───────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub name: String,
    pub email: String,
    pub initials: String,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContactEmailEntry {
    pub email: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContactPhoneEntry {
    pub number: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub subtypes: Vec<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContactAddressEntry {
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub postal_code: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FullContact {
    pub id: String,
    pub name: String,
    /// Primary email — mirrors `emails[default]` for compatibility with Contact-typed consumers.
    #[serde(default)]
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middle_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    #[serde(default)]
    pub emails: Vec<ContactEmailEntry>,
    #[serde(default)]
    pub phones: Vec<ContactPhoneEntry>,
    #[serde(default)]
    pub addresses: Vec<ContactAddressEntry>,
    pub initials: String,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birthday: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub is_favorite: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(default)]
    pub is_read_only: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentEntry {
    pub index: i64,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    pub id: String,
    pub from: Contact,
    pub to: Vec<Contact>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<Contact>,
    pub subject: String,
    pub preview: String,
    pub body: String,
    pub date: String,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_pinned: bool,
    pub is_focused: bool,
    pub is_replied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replied_at: Option<String>,
    pub has_attachment: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentEntry>,
    pub folder: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reply_to: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub auth_results: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub is_favorite: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EventAttendee {
    pub name: String,
    pub email: String,
    pub initials: String,
    pub color: String,
    pub role: String, // "required" or "optional"
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AttendeeParam {
    pub name: String,
    pub email: String,
    pub initials: String,
    pub color: String,
    pub role: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceParam {
    pub freq: String,              // "daily" | "weekly" | "monthly" | "yearly"
    pub interval: u32,
    pub end_date: Option<String>,  // "2026-12-31"
    pub by_day: Option<String>,    // monthly-by-weekday: e.g. "3TU", "-1MO"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exdates: Option<Vec<String>>, // exception dates (deleted single occurrences)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub is_all_day: bool,
    pub calendar_id: String,
    pub calendar_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendees: Option<Vec<EventAttendee>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<RecurrenceParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_online_meeting: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_minutes: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CalendarCategory {
    pub id: String,
    pub name: String,
    pub color: String,
    pub visible: bool,
    pub group: String, // "my" or "other"
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub initials: String,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub folders: Vec<Folder>,
    pub emails: Vec<Email>,
    pub calendar_events: Vec<CalendarEvent>,
    pub calendar_categories: Vec<CalendarCategory>,
    pub contacts: Vec<FullContact>,
}

// ── App state ───────────────────────────────────────────

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub dav_handle: Mutex<Option<dav_server::DavServerHandle>>,
}

// ── Helpers ─────────────────────────────────────────────

/// Prepend `https://` if the URL has no scheme, so users can omit it.
fn normalize_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{}", url)
    }
}

// ── Tauri commands ──────────────────────────────────────

#[tauri::command]
fn get_all_accounts(state: tauri::State<'_, AppState>) -> Result<Vec<Account>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT id, name, email, initials, color, avatar_url, alias FROM accounts ORDER BY position, rowid")
        .map_err(|e| e.to_string())?;

    let accounts: Vec<Account> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|(id, name, email, initials, color, avatar_url, alias_raw)| {
            let alias = alias_raw.filter(|s| !s.is_empty());
            let folders = query_folders(&conn, &id);
            let emails = query_emails(&conn, &id);
            let calendar_events = query_events(&conn, &id);
            let calendar_categories = query_cal_categories(&conn, &id);
            let contacts = query_contacts(&conn, &id);

            Account {
                id,
                name,
                email,
                initials,
                color,
                avatar_url,
                alias,
                folders,
                emails,
                calendar_events,
                calendar_categories,
                contacts,
            }
        })
        .collect();

    Ok(accounts)
}

#[tauri::command]
fn get_setting(state: tauri::State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let result = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get::<_, String>(0),
        )
        .ok();
    Ok(result)
}

#[tauri::command]
fn set_setting(state: tauri::State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn update_account(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    email: String,
    initials: String,
    color: String,
    avatar_url: Option<String>,
    alias: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE accounts SET name=?2, email=?3, initials=?4, color=?5, avatar_url=?6, alias=?7 WHERE id=?1",
        rusqlite::params![id, name, email, initials, color, avatar_url, alias.unwrap_or_default()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn add_account(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    email: String,
    initials: String,
    color: String,
    avatar_url: Option<String>,
    alias: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let next_position: i64 = conn
        .query_row("SELECT COALESCE(MAX(position), 0) + 1 FROM accounts", [], |r| r.get(0))
        .unwrap_or(1);
    conn.execute(
        "INSERT INTO accounts (id, name, email, initials, color, avatar_url, alias, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![id, name, email, initials, color, avatar_url, alias.unwrap_or_default(), next_position],
    )
    .map_err(|e| e.to_string())?;

    let default_folders = vec![
        ("inbox", "Inbox", "inbox", 1),
        ("sent", "Sent Items", "sent", 1),
        ("drafts", "Drafts", "drafts", 0),
        ("archive", "Archive", "archive", 0),
        ("deleted", "Deleted Items", "trash", 0),
        ("junk", "Junk Email", "junk", 0),
    ];
    for (fid, fname, ficon, ffav) in default_folders {
        conn.execute(
            "INSERT INTO folders (id, account_id, name, icon, is_favorite) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![fid, id, fname, ficon, ffav],
        )
        .map_err(|e| e.to_string())?;
    }

    // Every account gets the four default local calendars
    for (slug, name, color, group) in &[
        ("personal",  "Personal",      "#0078d4", "my"),
        ("work",      "Work",          "#498205", "my"),
        ("birthdays", "Birthdays",     "#e3008c", "other"),
        ("birthdays", "Anniversaries", "#ee7733", "other"),
        ("holidays",  "Holidays",      "#da3b01", "other"),
    ] {
        conn.execute(
            "INSERT OR IGNORE INTO caldav_collection_state (account_id, collection_url, ctag, display_name, color, visible, calendar_group) VALUES (?1, ?2, '', ?3, ?4, 1, ?5)",
            rusqlite::params![id, slug, name, color, group],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
fn delete_account(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys = ON;").map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM accounts WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM folders WHERE account_id = ?1", rusqlite::params![id]).ok();
    conn.execute("DELETE FROM contacts WHERE account_id = ?1", rusqlite::params![id]).ok();
    conn.execute("DELETE FROM emails WHERE account_id = ?1", rusqlite::params![id]).ok();
    conn.execute("DELETE FROM calendar_categories WHERE account_id = ?1", rusqlite::params![id]).ok();
    conn.execute("DELETE FROM calendar_events WHERE account_id = ?1", rusqlite::params![id]).ok();
    Ok(())
}

/// Persist a new ordering for accounts. `ordered_ids` is the full list of
/// account ids in the desired display order; positions are written as 1..N.
#[tauri::command]
fn set_account_positions(
    state: tauri::State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (idx, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE accounts SET position = ?1 WHERE id = ?2",
            rusqlite::params![(idx as i64) + 1, id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn update_email_read(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
    is_read: bool,
) -> Result<(), String> {
    // Update local DB first
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE emails SET is_read = ?2 WHERE id = ?1",
            rusqlite::params![email_id, is_read as i32],
        )
        .map_err(|e| e.to_string())?;
    }

    // Sync \Seen flag to IMAP server
    let (uid, folder, settings) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let uid_folder: Option<(u32, String)> = conn.query_row(
            "SELECT m.uid, m.folder FROM imap_uid_map m WHERE m.email_id = ?1",
            rusqlite::params![email_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok();
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?;
        (uid_folder.as_ref().map(|(u, _)| *u), uid_folder.map(|(_, f)| f), settings)
    };

    if let (Some(uid), Some(folder), Some(settings)) = (uid, folder, settings) {
        match imap_client::connect(&settings).await {
            Ok(mut session) => {
                let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();
                let imap_folder = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == folder)
                    .map(|(name, _)| name.clone());
                if let Some(imap_folder) = imap_folder {
                    let flags = &[async_imap::types::Flag::Seen];
                    if is_read {
                        if let Err(e) = imap_client::set_flags(&mut session, &imap_folder, &[uid], flags).await {
                            log::error!("IMAP set \\Seen failed for UID {}: {}", uid, e);
                        }
                    } else {
                        if let Err(e) = imap_client::remove_flags(&mut session, &imap_folder, &[uid], flags).await {
                            log::error!("IMAP remove \\Seen failed for UID {}: {}", uid, e);
                        }
                    }
                }
                let _ = session.logout().await;
            }
            Err(e) => log::error!("IMAP connect failed for read flag sync: {}", e),
        }
    }

    Ok(())
}

#[tauri::command]
async fn update_email_starred(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
    is_starred: bool,
) -> Result<(), String> {
    // Update local DB first
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE emails SET is_starred = ?2 WHERE id = ?1",
            rusqlite::params![email_id, is_starred as i32],
        )
        .map_err(|e| e.to_string())?;
    }

    // Sync \Flagged to IMAP server
    let (uid, folder, settings) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let uid_folder: Option<(u32, String)> = conn.query_row(
            "SELECT m.uid, m.folder FROM imap_uid_map m WHERE m.email_id = ?1",
            rusqlite::params![email_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok();
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?;
        (uid_folder.as_ref().map(|(u, _)| *u), uid_folder.map(|(_, f)| f), settings)
    };

    if let (Some(uid), Some(folder), Some(settings)) = (uid, folder, settings) {
        match imap_client::connect(&settings).await {
            Ok(mut session) => {
                let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();
                let imap_folder = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == folder)
                    .map(|(name, _)| name.clone());
                if let Some(imap_folder) = imap_folder {
                    let flags = &[async_imap::types::Flag::Flagged];
                    if is_starred {
                        if let Err(e) = imap_client::set_flags(&mut session, &imap_folder, &[uid], flags).await {
                            log::error!("IMAP set \\Flagged failed for UID {}: {}", uid, e);
                        }
                    } else {
                        if let Err(e) = imap_client::remove_flags(&mut session, &imap_folder, &[uid], flags).await {
                            log::error!("IMAP remove \\Flagged failed for UID {}: {}", uid, e);
                        }
                    }
                }
                let _ = session.logout().await;
            }
            Err(e) => log::error!("IMAP connect failed for starred flag sync: {}", e),
        }
    }

    Ok(())
}

#[tauri::command]
async fn update_email_replied(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
    is_replied: bool,
) -> Result<(), String> {
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE emails SET is_replied = ?2, replied_at = ?3 WHERE id = ?1",
            rusqlite::params![email_id, is_replied as i32, if is_replied { Some(now) } else { None }],
        )
        .map_err(|e| e.to_string())?;
    }

    // Sync \Answered flag to IMAP server
    let (uid, folder, settings) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let uid_folder: Option<(u32, String)> = conn.query_row(
            "SELECT m.uid, m.folder FROM imap_uid_map m WHERE m.email_id = ?1",
            rusqlite::params![email_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok();
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?;
        (uid_folder.as_ref().map(|(u, _)| *u), uid_folder.map(|(_, f)| f), settings)
    };

    if let (Some(uid), Some(folder), Some(settings)) = (uid, folder, settings) {
        match imap_client::connect(&settings).await {
            Ok(mut session) => {
                let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();
                let imap_folder = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == folder)
                    .map(|(name, _)| name.clone());
                if let Some(imap_folder) = imap_folder {
                    let flags = &[async_imap::types::Flag::Answered];
                    if is_replied {
                        if let Err(e) = imap_client::set_flags(&mut session, &imap_folder, &[uid], flags).await {
                            log::error!("IMAP set \\Answered failed for UID {}: {}", uid, e);
                        }
                    } else {
                        if let Err(e) = imap_client::remove_flags(&mut session, &imap_folder, &[uid], flags).await {
                            log::error!("IMAP remove \\Answered failed for UID {}: {}", uid, e);
                        }
                    }
                }
                let _ = session.logout().await;
            }
            Err(e) => log::error!("IMAP connect failed for replied flag sync: {}", e),
        }
    }

    Ok(())
}

#[tauri::command]
fn update_email_pinned(
    state: tauri::State<'_, AppState>,
    email_id: String,
    is_pinned: bool,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE emails SET is_pinned = ?2 WHERE id = ?1",
        rusqlite::params![email_id, is_pinned as i32],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn update_email_focused(
    state: tauri::State<'_, AppState>,
    email_id: String,
    is_focused: bool,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE emails SET is_focused = ?2, is_focused_manual = 1 WHERE id = ?1",
        rusqlite::params![email_id, is_focused as i32],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Server settings commands ────────────────────────────

#[tauri::command]
fn save_mail_settings(
    state: tauri::State<'_, AppState>,
    account_id: String,
    settings: sync_state::MailServerSettings,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    sync_state::save_mail_settings(&conn, &account_id, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_mail_settings(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Option<sync_state::MailServerSettings>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    sync_state::load_mail_settings(&conn, &account_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_caldav_settings(
    state: tauri::State<'_, AppState>,
    account_id: String,
    settings: sync_state::DavSettings,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    sync_state::save_caldav_settings(&conn, &account_id, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_caldav_settings(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Option<sync_state::DavSettings>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    sync_state::load_caldav_settings(&conn, &account_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_carddav_settings(
    state: tauri::State<'_, AppState>,
    account_id: String,
    settings: sync_state::DavSettings,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    sync_state::save_carddav_settings(&conn, &account_id, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_carddav_settings(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Option<sync_state::DavSettings>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    sync_state::load_carddav_settings(&conn, &account_id).map_err(|e| e.to_string())
}

// ── IMAP sync command ───────────────────────────────────

/// Sync result returned to the frontend.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct MailSyncResult {
    new_count: usize,
    deleted_count: usize,
    flag_updates: usize,
    errors: Vec<String>,
}

#[tauri::command]
async fn sync_mail(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<MailSyncResult, String> {
    // Load settings from DB (need to clone out because we can't hold the lock across await)
    let (settings, folders_to_sync) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No mail server settings configured for this account".to_string())?;

        // Determine which IMAP folders to sync
        let folders: Vec<(String, String)> = conn
            .prepare("SELECT id, name FROM folders WHERE account_id = ?1")
            .map_err(|e| e.to_string())?
            .query_map(rusqlite::params![account_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        (settings, folders)
    };

    log::info!(
        "sync_mail [{}]: incoming={}:{} security={}, local folders={}",
        account_id, settings.incoming_server, settings.incoming_port,
        settings.incoming_security, folders_to_sync.len()
    );
    for (fid, fname) in &folders_to_sync {
        log::info!("  local folder: id={} name={}", fid, fname);
    }

    // Connect to IMAP
    let mut session = imap_client::connect(&settings)
        .await
        .map_err(|e| {
            log::error!("IMAP connect failed for account {}: {}", account_id, e);
            format!("IMAP connect failed: {}", e)
        })?;

    // List remote folders to map names
    let remote_folders = imap_client::list_folders(&mut session)
        .await
        .map_err(|e| {
            log::error!("Failed to list IMAP folders for account {}: {}", account_id, e);
            format!("Failed to list IMAP folders: {}", e)
        })?;

    log::info!("sync_mail [{}]: {} remote IMAP folders found", account_id, remote_folders.len());
    for (rname, mapped) in &remote_folders {
        log::info!("  remote folder: \"{}\" → mapped=\"{}\"", rname, mapped);
    }

    let mut total_new = 0usize;
    let mut total_deleted = 0usize;
    let mut total_flags = 0usize;
    let errors = Vec::new();

    // Sync each folder
    for (local_id, _local_name) in &folders_to_sync {
        // Find matching IMAP folder
        let imap_name = remote_folders.iter()
            .find(|(_, mapped)| mapped == local_id)
            .map(|(name, _)| name.clone());

        let imap_name = match imap_name {
            Some(n) => n,
            None => {
                log::warn!("sync_mail [{}]: no remote folder matches local '{}'", account_id, local_id);
                continue;
            }
        };

        let (prev_state, local_uids) = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let prev = sync_state::load_imap_folder_state(&conn, &account_id, local_id)
                .map_err(|e| e.to_string())?;
            let uids = sync_state::load_imap_uids(&conn, &account_id, local_id)
                .unwrap_or_default();
            // If we have saved sync state but no local UIDs, a previous sync saved
            // the state but emails were lost — reset to force a full re-fetch.
            if prev.is_some() && uids.is_empty() {
                log::warn!(
                    "sync_mail [{}]: folder '{}' has saved state but no local UIDs — resetting for full fetch",
                    account_id, local_id
                );
                (None, uids)
            } else {
                (prev, uids)
            }
        };

        log::info!("sync_mail [{}]: syncing local '{}' ↔ remote '{}'", account_id, local_id, imap_name);

        let sync_result = imap_client::sync_folder(
            &mut session, prev_state, local_uids, &account_id, &imap_name, local_id,
        )
        .await
        .map_err(|e| {
            log::error!(
                "IMAP sync failed for account {} folder {} (remote {}): {}",
                account_id,
                local_id,
                imap_name,
                e
            );
            format!("Sync {} failed: {}", local_id, e)
        })?;

        log::info!(
            "sync_mail [{}]: folder '{}' result: {} new, {} deleted, {} flag changes, full_resync={}",
            account_id, local_id,
            sync_result.new_emails.len(), sync_result.deleted_uids.len(),
            sync_result.flag_changes.len(), sync_result.full_resync
        );

        // Apply results to local DB in a single transaction per folder
        {
            let mut conn = state.db.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;

            // Insert new emails
            for env in &sync_result.new_emails {

                // Dedup by Message-ID: if an email with the same non-empty
                // Message-ID already exists for this account, attach the new
                // UID mapping to the existing row and update its folder in
                // place instead of inserting a second copy. Guards against
                // the server moving messages (e.g. Inbox→Trash) between syncs
                // without us observing a clean UID transition.
                let existing_id: Option<String> = if env.message_id.trim().is_empty() {
                    None
                } else {
                    tx.query_row(
                        "SELECT id FROM emails WHERE account_id = ?1 AND message_id = ?2 LIMIT 1",
                        rusqlite::params![account_id, env.message_id],
                        |row| row.get(0),
                    ).ok()
                };

                if let Some(existing_id) = existing_id {
                    tx.execute(
                        "UPDATE emails SET folder = ?1, is_read = ?2, is_starred = ?3 WHERE id = ?4",
                        rusqlite::params![local_id, env.is_read as i32, env.is_starred as i32, existing_id],
                    ).ok();
                    sync_state::insert_imap_uid(&tx, &account_id, local_id, env.uid, &existing_id).ok();
                    continue;
                }

                let email_id = uuid::Uuid::new_v4().to_string();
                let initials = make_initials(&env.from_name, &env.from_email);
                let color = make_avatar_color(&env.from_email);

                tx.execute(
                    "INSERT OR IGNORE INTO emails
                     (id, account_id, from_name, from_email, from_initials, from_color,
                      subject, preview, body, date, is_read, is_starred, is_replied, has_attachment, folder, is_pinned,
                      reply_to, message_id, auth_results)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0,?16,?17,?18)",
                    rusqlite::params![
                        email_id, account_id,
                        env.from_name, env.from_email, initials, color,
                        env.subject, env.preview,
                        env.body,
                        env.date,
                        env.is_read as i32, env.is_starred as i32, env.is_replied as i32,
                        env.has_attachment as i32, local_id,
                        env.reply_to, env.message_id, env.auth_results,
                    ],
                ).ok();

                // Store To recipients
                for (name, email) in &env.to_list {
                    let ri = make_initials(name, email);
                    let rc = make_avatar_color(email);
                    tx.execute(
                        "INSERT INTO email_recipients (email_id, name, email, initials, color, type)
                         VALUES (?1,?2,?3,?4,?5,'to')",
                        rusqlite::params![email_id, name, email, ri, rc],
                    ).ok();
                }

                // Store CC recipients
                for (name, email) in &env.cc_list {
                    let ri = make_initials(name, email);
                    let rc = make_avatar_color(email);
                    tx.execute(
                        "INSERT INTO email_recipients (email_id, name, email, initials, color, type)
                         VALUES (?1,?2,?3,?4,?5,'cc')",
                        rusqlite::params![email_id, name, email, ri, rc],
                    ).ok();
                }

                // Store attachment metadata
                for (idx, meta) in env.attachments.iter().enumerate() {
                    tx.execute(
                        "INSERT OR IGNORE INTO email_attachments (email_id, idx, filename, mime_type, size) VALUES (?1,?2,?3,?4,?5)",
                        rusqlite::params![email_id, idx as i64, meta.filename, meta.mime_type, meta.size as i64],
                    ).ok();
                }

                // Auto-junk: move to junk if sender is blocked or auth hard-fails
                if local_id == "inbox" {
                    let blocked = db::is_sender_blocked(&tx, &env.from_email, &account_id);
                    let auth_fail = {
                        let ar = env.auth_results.to_lowercase();
                        ar.contains("spf=fail") || ar.contains("dkim=fail") || ar.contains("dmarc=fail")
                    };
                    if blocked || auth_fail {
                        tx.execute(
                            "UPDATE emails SET folder = 'junk' WHERE id = ?1",
                            rusqlite::params![email_id],
                        ).ok();
                    }
                }

                // Track UID→email_id mapping
                sync_state::insert_imap_uid(&tx, &account_id, local_id, env.uid, &email_id).ok();
                total_new += 1;
            }

            // Handle deletions + flag changes — hoist UID map once instead of per-row
            if !sync_result.deleted_uids.is_empty() || !sync_result.flag_changes.is_empty() {
                let uid_map = sync_state::load_imap_uids(&tx, &account_id, local_id)
                    .unwrap_or_default();

                for uid in &sync_result.deleted_uids {
                    if let Some((_, email_id)) = uid_map.iter().find(|(u, _)| *u == *uid) {
                        tx.execute("DELETE FROM emails WHERE id = ?1", rusqlite::params![email_id]).ok();
                    }
                    total_deleted += 1;
                }
                if !sync_result.deleted_uids.is_empty() {
                    sync_state::delete_imap_uids(&tx, &account_id, local_id, &sync_result.deleted_uids).ok();
                }

                for (uid, is_read, is_starred, is_replied) in &sync_result.flag_changes {
                    if let Some((_, email_id)) = uid_map.iter().find(|(u, _)| u == uid) {
                        tx.execute(
                            "UPDATE emails SET is_read = ?2, is_starred = ?3, is_replied = ?4 WHERE id = ?1",
                            rusqlite::params![email_id, *is_read as i32, *is_starred as i32, *is_replied as i32],
                        ).ok();
                        total_flags += 1;
                    }
                }
            }

            // Save updated sync state
            sync_state::save_imap_folder_state(&tx, &account_id, local_id, &sync_result.new_state).ok();

            tx.commit().map_err(|e| e.to_string())?;
        }
    }

    // Reclassify inbox emails now that all folders (incl. sent recipients) are in the DB
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE emails SET is_focused = CASE
               WHEN lower(from_email) GLOB 'noreply*'
                 OR lower(from_email) GLOB 'no-reply*'
                 OR lower(from_email) GLOB 'no_reply*'
                 OR lower(from_email) GLOB 'donotreply*'
                 OR lower(from_email) GLOB 'do-not-reply*'
                 OR lower(from_email) GLOB '*+noreply*'
               THEN 0
               WHEN EXISTS (
                 SELECT 1 FROM contacts c
                 WHERE c.account_id = emails.account_id
                   AND lower(c.email) = lower(emails.from_email)
               ) THEN 1
               WHEN EXISTS (
                 SELECT 1 FROM contact_emails ce
                 JOIN contacts c ON ce.contact_id = c.id
                 WHERE c.account_id = emails.account_id
                   AND lower(ce.address) = lower(emails.from_email)
               ) THEN 1
               WHEN EXISTS (
                 SELECT 1 FROM email_recipients er
                 JOIN emails sent ON er.email_id = sent.id
                 WHERE sent.account_id = emails.account_id
                   AND lower(er.email) = lower(emails.from_email)
               ) THEN 1
               ELSE 0
             END
             WHERE account_id = ?1 AND folder = 'inbox' AND is_focused_manual = 0",
            rusqlite::params![account_id],
        ).ok();
    }

    // Logout cleanly
    session.logout().await.ok();

    log::info!(
        "sync_mail [{}]: complete — {} new, {} deleted, {} flag updates, {} errors",
        account_id, total_new, total_deleted, total_flags, errors.len()
    );

    Ok(MailSyncResult {
        new_count: total_new,
        deleted_count: total_deleted,
        flag_updates: total_flags,
        errors,
    })
}

// ── Delete email ────────────────────────────────────────

#[tauri::command]
async fn delete_email(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
) -> Result<(), String> {
    let (folder, uid, message_id, settings) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;

        let (folder, message_id): (String, String) = conn
            .query_row(
                "SELECT folder, COALESCE(message_id, '') FROM emails WHERE id = ?1",
                rusqlite::params![email_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Email not found".to_string())?;

        // Look up the IMAP UID for this email
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

    // Sync deletion to IMAP server if we have UID and settings
    let mut new_trash_uid: Option<u32> = None;
    if let (Some(uid), Some(settings)) = (uid, settings) {
        match imap_client::connect(&settings).await {
            Ok(mut session) => {
                let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();

                // Find the IMAP name for the email's current folder
                let source_imap = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == folder)
                    .map(|(name, _)| name.clone());

                if let Some(source) = source_imap {
                    if folder == "deleted" {
                        // Permanently delete: flag as \Deleted + EXPUNGE
                        let del_result: Result<(), String> = async {
                            use futures::TryStreamExt;
                            session.select(&source).await.map_err(|e| e.to_string())?;
                            let uid_str = uid.to_string();
                            session.uid_store(&uid_str, "+FLAGS (\\Deleted)").await
                                .map_err(|e| e.to_string())?
                                .try_collect::<Vec<_>>().await.map_err(|e| e.to_string())?;
                            session.expunge().await.map_err(|e| e.to_string())?
                                .try_collect::<Vec<_>>().await.map_err(|e| e.to_string())?;
                            Ok(())
                        }.await;
                        if let Err(e) = del_result {
                            log::error!("IMAP permanent delete failed for UID {}: {}", uid, e);
                        }
                    } else {
                        // Move to Trash/Deleted folder
                        let trash_imap = remote_folders.iter()
                            .find(|(_, mapped)| mapped == "deleted")
                            .map(|(name, _)| name.clone());

                        if let Some(trash) = trash_imap {
                            match imap_client::move_message(&mut session, &source, &trash, uid, &message_id).await {
                                Ok(new_uid) => { new_trash_uid = new_uid; }
                                Err(e) => log::error!("IMAP move to trash failed for UID {}: {}", uid, e),
                            }
                        } else {
                            // No Trash folder — just flag as deleted
                            log::warn!("No Trash folder found on IMAP server, flagging as \\Deleted");
                            let del_result: Result<(), String> = async {
                                use futures::TryStreamExt;
                                session.select(&source).await.map_err(|e| e.to_string())?;
                                let uid_str = uid.to_string();
                                session.uid_store(&uid_str, "+FLAGS (\\Deleted)").await
                                    .map_err(|e| e.to_string())?
                                    .try_collect::<Vec<_>>().await.map_err(|e| e.to_string())?;
                                session.expunge().await.map_err(|e| e.to_string())?
                                    .try_collect::<Vec<_>>().await.map_err(|e| e.to_string())?;
                                Ok(())
                            }.await;
                            if let Err(e) = del_result {
                                log::error!("IMAP delete-flag failed for UID {}: {}", uid, e);
                            }
                        }
                    }
                }
                let _ = session.logout().await;
            }
            Err(e) => {
                log::error!("IMAP connect failed for delete: {}", e);
            }
        }
    }

    // Update local DB
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    if folder == "deleted" {
        conn.execute("DELETE FROM email_recipients WHERE email_id = ?1", rusqlite::params![email_id]).ok();
        conn.execute("DELETE FROM imap_uid_map WHERE email_id = ?1", rusqlite::params![email_id]).ok();
        conn.execute("DELETE FROM emails WHERE id = ?1", rusqlite::params![email_id]).ok();
    } else {
        conn.execute(
            "UPDATE emails SET folder = 'deleted' WHERE id = ?1",
            rusqlite::params![email_id],
        ).map_err(|e| e.to_string())?;

        // Refresh imap_uid_map: drop the stale source-folder row and insert the
        // new destination mapping if we resolved it. If the resolution failed,
        // drop the stale entry anyway so the next sync re-inserts cleanly (the
        // Message-ID dedup in apply_sync_results will attach to the same email).
        conn.execute("DELETE FROM imap_uid_map WHERE email_id = ?1", rusqlite::params![email_id]).ok();
        if let Some(new_uid) = new_trash_uid {
            sync_state::insert_imap_uid(&conn, &account_id, "deleted", new_uid, &email_id).ok();
        }
    }

    Ok(())
}

// ── Empty folder (permanently delete all emails in a folder) ────────

#[tauri::command]
async fn empty_folder(
    state: tauri::State<'_, AppState>,
    account_id: String,
    folder: String,
) -> Result<u64, String> {
    // Collect all UIDs for IMAP deletion
    let (uids, settings) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;

        let mut stmt = conn.prepare(
            "SELECT m.uid FROM imap_uid_map m JOIN emails e ON e.id = m.email_id
             WHERE e.account_id = ?1 AND e.folder = ?2",
        ).map_err(|e| e.to_string())?;

        let uids: Vec<u32> = stmt
            .query_map(rusqlite::params![account_id, folder], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?;

        (uids, settings)
    };

    // Delete from IMAP server
    if let Some(settings) = settings {
        if !uids.is_empty() {
            match imap_client::connect(&settings).await {
                Ok(mut session) => {
                    let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();
                    let imap_folder = remote_folders.iter()
                        .find(|(_, mapped)| *mapped == folder)
                        .map(|(name, _)| name.clone());

                    if let Some(imap_name) = imap_folder {
                        let del_result: Result<(), String> = async {
                            use futures::TryStreamExt;
                            session.select(&imap_name).await.map_err(|e| e.to_string())?;
                            let uid_set = uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");
                            session.uid_store(&uid_set, "+FLAGS (\\Deleted)").await
                                .map_err(|e| e.to_string())?
                                .try_collect::<Vec<_>>().await.map_err(|e| e.to_string())?;
                            session.expunge().await.map_err(|e| e.to_string())?
                                .try_collect::<Vec<_>>().await.map_err(|e| e.to_string())?;
                            Ok(())
                        }.await;
                        if let Err(e) = &del_result {
                            log::error!("IMAP empty folder failed for {}: {}", folder, e);
                        }
                    }
                    let _ = session.logout().await;
                }
                Err(e) => {
                    log::error!("IMAP connect failed for empty_folder: {}", e);
                }
            }
        }
    }

    // Delete from local DB
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM imap_uid_map WHERE email_id IN (SELECT id FROM emails WHERE account_id = ?1 AND folder = ?2)",
        rusqlite::params![account_id, folder],
    ).ok();
    conn.execute(
        "DELETE FROM email_recipients WHERE email_id IN (SELECT id FROM emails WHERE account_id = ?1 AND folder = ?2)",
        rusqlite::params![account_id, folder],
    ).ok();
    let deleted = conn.execute(
        "DELETE FROM emails WHERE account_id = ?1 AND folder = ?2",
        rusqlite::params![account_id, folder],
    ).map_err(|e| e.to_string())? as u64;

    log::info!("empty_folder: deleted {} emails from {} for account {}", deleted, folder, account_id);
    Ok(deleted)
}

// ── Move email to folder ────────────────────────────────

#[tauri::command]
async fn move_email(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
    target_folder: String,
) -> Result<(), String> {
    let (current_folder, uid, message_id, settings) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;

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

    // Sync move to IMAP server
    let mut new_target_uid: Option<u32> = None;
    if let (Some(uid), Some(settings)) = (uid, settings) {
        match imap_client::connect(&settings).await {
            Ok(mut session) => {
                let remote_folders = imap_client::list_folders(&mut session).await.unwrap_or_default();

                let source_imap = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == current_folder)
                    .map(|(name, _)| name.clone());

                let target_imap = remote_folders.iter()
                    .find(|(_, mapped)| *mapped == target_folder)
                    .map(|(name, _)| name.clone());

                if let (Some(source), Some(target)) = (source_imap, target_imap) {
                    match imap_client::move_message(&mut session, &source, &target, uid, &message_id).await {
                        Ok(new_uid) => { new_target_uid = new_uid; }
                        Err(e) => log::error!("IMAP move failed for UID {} from {} to {}: {}", uid, source, target, e),
                    }
                } else {
                    log::warn!("Could not find IMAP folder mapping for move: {} -> {}", current_folder, target_folder);
                }
                let _ = session.logout().await;
            }
            Err(e) => {
                log::error!("IMAP connect failed for move: {}", e);
            }
        }
    }

    // Update local DB
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE emails SET folder = ?1 WHERE id = ?2",
        rusqlite::params![target_folder, email_id],
    ).map_err(|e| e.to_string())?;

    // Refresh imap_uid_map so the next sync doesn't treat the destination copy
    // as a new message. See comment in delete_email for details.
    conn.execute("DELETE FROM imap_uid_map WHERE email_id = ?1", rusqlite::params![email_id]).ok();
    if let Some(new_uid) = new_target_uid {
        sync_state::insert_imap_uid(&conn, &account_id, &target_folder, new_uid, &email_id).ok();
    }

    Ok(())
}

// ── Fetch email body on demand ──────────────────────────

#[tauri::command]
async fn fetch_email_body(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
) -> Result<serde_json::Value, String> {
    // First check if body is already cached
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let cached: Option<String> = conn
            .query_row(
                "SELECT body FROM emails WHERE id = ?1 AND body != ''",
                rusqlite::params![email_id],
                |row| row.get(0),
            )
            .ok();
        if let Some(body) = cached {
            if !body.is_empty() {
                let (preview, auth_results, has_attach): (String, String, bool) = conn
                    .query_row(
                        "SELECT preview, COALESCE(auth_results, ''), has_attachment FROM emails WHERE id = ?1",
                        rusqlite::params![email_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i32>(2)? != 0)),
                    )
                    .unwrap_or_default();
                // Load cached attachment metadata
                let attachments: Vec<serde_json::Value> = {
                    let mut stmt = conn.prepare(
                        "SELECT idx, filename, mime_type, size FROM email_attachments WHERE email_id = ?1 ORDER BY idx"
                    ).unwrap();
                    stmt.query_map(rusqlite::params![email_id], |row| {
                        Ok(serde_json::json!({
                            "index": row.get::<_, i64>(0)?,
                            "filename": row.get::<_, String>(1)?,
                            "mimeType": row.get::<_, String>(2)?,
                            "size": row.get::<_, i64>(3)?
                        }))
                    }).unwrap().filter_map(|r| r.ok()).collect()
                };
                // If has_attachment is set but email_attachments was never populated
                // (email synced before attachment extraction was added), fall through
                // to the IMAP fetch path so metadata gets extracted and stored.
                if has_attach && attachments.is_empty() {
                    // fall through
                } else {
                    return Ok(serde_json::json!({
                        "body": body,
                        "preview": preview,
                        "authResults": auth_results,
                        "attachments": attachments
                    }));
                }
            }
        }
    }

    // Need to fetch from IMAP
    let (settings, uid, imap_folder) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No mail settings")?;

        // Find the UID for this email
        let row: Option<(u32, String)> = conn
            .query_row(
                "SELECT m.uid, e.folder FROM imap_uid_map m JOIN emails e ON e.id = m.email_id
                 WHERE m.email_id = ?1 AND m.account_id = ?2",
                rusqlite::params![email_id, account_id],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .ok();

        let (uid, folder) = row.ok_or("Email UID not found")?;
        (settings, uid, folder)
    };

    // Connect and fetch
    let mut session = imap_client::connect(&settings)
        .await
        .map_err(|e| {
            log::error!("IMAP connect failed while fetching body for email {}: {}", email_id, e);
            format!("IMAP connect failed: {}", e)
        })?;

    // Map folder ID back to IMAP name
    let remote_folders = imap_client::list_folders(&mut session)
        .await
        .map_err(|e| {
            log::error!("Failed to list IMAP folders while fetching body for email {}: {}", email_id, e);
            e.to_string()
        })?;
    let imap_name = remote_folders.iter()
        .find(|(_, mapped)| *mapped == imap_folder)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| imap_folder.clone());

    let (body, preview, auth_results, has_attachment, attachment_meta) =
        imap_client::fetch_body(&mut session, &imap_name, uid)
        .await
        .map_err(|e| {
            log::error!(
                "Failed to fetch IMAP body for email {} uid {} folder {}: {}",
                email_id,
                uid,
                imap_name,
                e
            );
            format!("Body fetch failed: {}", e)
        })?;

    session.logout().await.ok();

    // Cache body, preview, auth results, attachment flag, and per-attachment metadata
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE emails SET body = ?2, preview = ?3, auth_results = ?4, has_attachment = ?5 WHERE id = ?1",
            rusqlite::params![email_id, body, preview, auth_results, has_attachment as i32],
        ).ok();
        // Replace attachment rows for this email
        conn.execute("DELETE FROM email_attachments WHERE email_id = ?1", rusqlite::params![email_id]).ok();
        for (idx, meta) in attachment_meta.iter().enumerate() {
            conn.execute(
                "INSERT INTO email_attachments (email_id, idx, filename, mime_type, size) VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![email_id, idx as i64, meta.filename, meta.mime_type, meta.size as i64],
            ).ok();
        }
    }

    let attachments: Vec<serde_json::Value> = attachment_meta.iter().enumerate().map(|(idx, m)| {
        serde_json::json!({
            "index": idx,
            "filename": m.filename,
            "mimeType": m.mime_type,
            "size": m.size
        })
    }).collect();

    Ok(serde_json::json!({
        "body": body,
        "preview": preview,
        "authResults": auth_results,
        "hasAttachment": has_attachment,
        "attachments": attachments
    }))
}

// ── Fetch previews around a viewport anchor ─────────────

/// Fill previews for messages near a given anchor in the same folder, in one
/// IMAP round trip. Called by the frontend when a blank-preview row scrolls
/// into view: `ahead` rows older than the anchor, `behind` rows newer, plus
/// the anchor itself. Only rows whose preview is currently empty are fetched.
#[tauri::command]
async fn fetch_previews_around(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
    ahead: u32,
    behind: u32,
) -> Result<serde_json::Value, String> {
    let (settings, anchor_folder, anchor_date, targets) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;

        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No mail settings")?;

        let (anchor_folder, anchor_date): (String, String) = conn
            .query_row(
                "SELECT folder, date FROM emails WHERE id = ?1 AND account_id = ?2",
                rusqlite::params![email_id, account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Anchor email not found".to_string())?;

        // Collect rows in the window: anchor + `ahead` older + `behind` newer,
        // all with empty preview and a known IMAP UID in the same folder.
        let mut targets: Vec<(String, u32)> = Vec::new();

        // Anchor itself (only if still blank)
        if let Ok((id, uid)) = conn.query_row(
            "SELECT e.id, m.uid
               FROM emails e
               JOIN imap_uid_map m ON m.email_id = e.id AND m.folder = e.folder
              WHERE e.id = ?1 AND e.preview = ''",
            rusqlite::params![email_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
        ) {
            targets.push((id, uid));
        }

        // Older than anchor (date DESC — closest first)
        let mut older_stmt = conn.prepare(
            "SELECT e.id, m.uid
               FROM emails e
               JOIN imap_uid_map m ON m.email_id = e.id AND m.folder = e.folder
              WHERE e.account_id = ?1
                AND e.folder     = ?2
                AND e.preview    = ''
                AND e.date       < ?3
              ORDER BY e.date DESC
              LIMIT ?4"
        ).map_err(|e| e.to_string())?;
        if let Ok(rows) = older_stmt.query_map(
            rusqlite::params![account_id, anchor_folder, anchor_date, ahead as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
        ) {
            for r in rows.flatten() { targets.push(r); }
        }

        // Newer than anchor (date ASC — closest first)
        let mut newer_stmt = conn.prepare(
            "SELECT e.id, m.uid
               FROM emails e
               JOIN imap_uid_map m ON m.email_id = e.id AND m.folder = e.folder
              WHERE e.account_id = ?1
                AND e.folder     = ?2
                AND e.preview    = ''
                AND e.date       > ?3
              ORDER BY e.date ASC
              LIMIT ?4"
        ).map_err(|e| e.to_string())?;
        if let Ok(rows) = newer_stmt.query_map(
            rusqlite::params![account_id, anchor_folder, anchor_date, behind as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
        ) {
            for r in rows.flatten() { targets.push(r); }
        }

        (settings, anchor_folder, anchor_date, targets)
    };
    let _ = anchor_date; // retained above for the query; not needed further

    if targets.is_empty() {
        return Ok(serde_json::json!({ "updates": [] }));
    }

    let uids: Vec<u32> = targets.iter().map(|(_, u)| *u).collect();
    let uid_to_email_id: std::collections::HashMap<u32, String> = targets
        .into_iter()
        .map(|(id, uid)| (uid, id))
        .collect();

    let mut session = imap_client::connect(&settings)
        .await
        .map_err(|e| format!("IMAP connect failed: {}", e))?;

    let remote_folders = imap_client::list_folders(&mut session)
        .await
        .map_err(|e| e.to_string())?;
    let imap_name = remote_folders.iter()
        .find(|(_, mapped)| *mapped == anchor_folder)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| anchor_folder.clone());

    let fetched = imap_client::fetch_bodies_full_batch(&mut session, &imap_name, &uids)
        .await
        .unwrap_or_default();

    session.logout().await.ok();

    // Persist and build the response payload.
    let mut updates = Vec::with_capacity(fetched.len());
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for (uid, html, preview, auth, has_attach, attach_meta) in &fetched {
            let Some(id) = uid_to_email_id.get(uid) else { continue };
            tx.execute(
                "UPDATE emails SET body = ?2, preview = ?3, auth_results = ?4, has_attachment = ?5 WHERE id = ?1",
                rusqlite::params![id, html, preview, auth, *has_attach as i32],
            ).ok();
            tx.execute("DELETE FROM email_attachments WHERE email_id = ?1", rusqlite::params![id]).ok();
            for (idx, meta) in attach_meta.iter().enumerate() {
                tx.execute(
                    "INSERT INTO email_attachments (email_id, idx, filename, mime_type, size) VALUES (?1,?2,?3,?4,?5)",
                    rusqlite::params![id, idx as i64, meta.filename, meta.mime_type, meta.size as i64],
                ).ok();
            }
            let attachments: Vec<serde_json::Value> = attach_meta.iter().enumerate().map(|(idx, m)| {
                serde_json::json!({
                    "index": idx,
                    "filename": m.filename,
                    "mimeType": m.mime_type,
                    "size": m.size
                })
            }).collect();
            updates.push(serde_json::json!({
                "id": id,
                "body": html,
                "preview": preview,
                "authResults": auth,
                "hasAttachment": *has_attach,
                "attachments": attachments,
            }));
        }
        tx.commit().ok();
    }

    Ok(serde_json::json!({ "updates": updates }))
}

// ── Open attachment ─────────────────────────────────────

#[tauri::command]
async fn open_attachment(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
    attachment_index: usize,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    // Resolve UID + IMAP folder from DB
    let (settings, uid, imap_folder) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No mail settings")?;
        let row: Option<(u32, String)> = conn
            .query_row(
                "SELECT m.uid, e.folder FROM imap_uid_map m JOIN emails e ON e.id = m.email_id
                 WHERE m.email_id = ?1 AND m.account_id = ?2",
                rusqlite::params![email_id, account_id],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .ok();
        let (uid, folder) = row.ok_or("Email UID not found")?;
        (settings, uid, folder)
    };

    // Connect and fetch raw MIME
    let mut session = imap_client::connect(&settings)
        .await
        .map_err(|e| format!("IMAP connect failed: {}", e))?;

    let remote_folders = imap_client::list_folders(&mut session)
        .await
        .map_err(|e| e.to_string())?;
    let imap_name = remote_folders.iter()
        .find(|(_, mapped)| *mapped == imap_folder)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| imap_folder.clone());

    let raw = imap_client::fetch_raw(&mut session, &imap_name, uid)
        .await
        .map_err(|e| format!("Body fetch failed: {}", e))?;
    session.logout().await.ok();

    // Extract the requested attachment bytes
    let (filename, bytes) = imap_client::extract_attachment_bytes(&raw, attachment_index)
        .ok_or_else(|| format!("Attachment index {} not found", attachment_index))?;

    // Write to a per-email temp directory so different attachments don't collide
    let tmp_dir = std::env::temp_dir().join("mail_attachments").join(&email_id);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let tmp_path = tmp_dir.join(&filename);
    std::fs::write(&tmp_path, &bytes).map_err(|e| e.to_string())?;

    app.opener()
        .open_path(tmp_path.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Save an attachment to a user-chosen path (the frontend provides save_path via a dialog).
#[tauri::command]
async fn save_attachment(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email_id: String,
    attachment_index: usize,
    save_path: String,
) -> Result<(), String> {
    // Resolve UID + IMAP folder from DB
    let (settings, uid, imap_folder) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No mail settings")?;
        let row: Option<(u32, String)> = conn
            .query_row(
                "SELECT m.uid, e.folder FROM imap_uid_map m JOIN emails e ON e.id = m.email_id
                 WHERE m.email_id = ?1 AND m.account_id = ?2",
                rusqlite::params![email_id, account_id],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .ok();
        let (uid, folder) = row.ok_or("Email UID not found")?;
        (settings, uid, folder)
    };

    // Connect and fetch raw MIME
    let mut session = imap_client::connect(&settings)
        .await
        .map_err(|e| format!("IMAP connect failed: {}", e))?;

    let remote_folders = imap_client::list_folders(&mut session)
        .await
        .map_err(|e| e.to_string())?;
    let imap_name = remote_folders.iter()
        .find(|(_, mapped)| *mapped == imap_folder)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| imap_folder.clone());

    let raw = imap_client::fetch_raw(&mut session, &imap_name, uid)
        .await
        .map_err(|e| format!("Body fetch failed: {}", e))?;
    session.logout().await.ok();

    // Extract the requested attachment bytes
    let (_filename, bytes) = imap_client::extract_attachment_bytes(&raw, attachment_index)
        .ok_or_else(|| format!("Attachment index {} not found", attachment_index))?;

    // Write to the user-specified path
    std::fs::write(&save_path, &bytes).map_err(|e| e.to_string())?;

    Ok(())
}

// ── SMTP send command ───────────────────────────────────

#[tauri::command]
async fn send_email(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    account_id: String,
    email: smtp_client::OutboundEmail,
) -> Result<(), String> {
    let (settings, from_email, from_name) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = sync_state::load_mail_settings(&conn, &account_id)
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
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        smtp_client::queue_email(&conn, &account_id, &email, &from_email)
            .map_err(|e| e.to_string())?
    };
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE outbox_queue SET status = 'sending' WHERE id = ?1",
            rusqlite::params![outbox_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let raw_message = match smtp_client::send_email(&settings, &from_email, &from_name, &email, Some(&message_id)).await {
        Ok(raw) => {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE outbox_queue SET status = 'sent' WHERE id = ?1",
                rusqlite::params![outbox_id],
            )
            .map_err(|e| e.to_string())?;
            raw
        }
        Err(e) => {
            log::error!("SMTP send failed for account {}: {}", account_id, e);
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE outbox_queue SET status = 'pending', error = ?2, retry_count = retry_count + 1 WHERE id = ?1",
                rusqlite::params![outbox_id, e.to_string()],
            )
            .map_err(|e| e.to_string())?;
            return Err(format!("Send failed, queued for retry: {}", e));
        }
    };

    // Save to Sent folder via IMAP APPEND
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

            if let Err(e) = imap_client::append_to_mailbox(&mut session, &sent_folder, &raw_message).await {
                log::error!("Failed to save sent message to '{}': {}", sent_folder, e);
            } else {
                log::info!("Sent message saved to IMAP folder '{}'", sent_folder);
                // Notify the frontend to sync immediately
                let _ = app.emit("mail:sent", &account_id);
            }
            let _ = session.logout().await;
        }
        Err(e) => {
            log::error!("IMAP connect failed when saving to Sent for account {}: {}", account_id, e);
        }
    }

    Ok(())
}

/// Flush the outbox queue (retry pending sends).
#[tauri::command]
async fn flush_outbox(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<usize, String> {
    let (settings, from_email, from_name) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = sync_state::load_mail_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No mail settings")?;
        let (name, email_addr): (String, String) = conn
            .query_row(
                "SELECT name, email FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        (settings, email_addr, name)
    };

    // First, reconcile any rows stuck in 'sending' (e.g. app was killed mid-send).
    if let Err(e) = smtp_client::reconcile_sending(state.db.clone(), &account_id, &settings).await {
        log::warn!("reconcile_sending error for {}: {}", account_id, e);
    }

    // Read pending queue entries (after reconcile, these include anything we
    // flipped back from 'sending').
    let conn_data: Vec<(String, String, String, String, String, String, String, String)> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id, to_addrs, cc_addrs, bcc_addrs, subject, body_html, attachments, message_id
             FROM outbox_queue WHERE account_id = ?1 AND status = 'pending' ORDER BY created_at"
        ).map_err(|e| e.to_string())?;
        let out: Vec<(String, String, String, String, String, String, String, String)> = stmt
            .query_map(rusqlite::params![account_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        out
    };

    let mut sent_count = 0usize;

    for (id, to_json, cc_json, bcc_json, subject, body_html, att_json, message_id) in conn_data {
        let email = smtp_client::OutboundEmail {
            to: serde_json::from_str(&to_json).unwrap_or_default(),
            cc: serde_json::from_str(&cc_json).unwrap_or_default(),
            bcc: serde_json::from_str(&bcc_json).unwrap_or_default(),
            subject,
            body_html,
            attachments: serde_json::from_str(&att_json).unwrap_or_default(),
        };

        // Mark as sending before the SMTP call so a crash here is recoverable
        // via reconcile_sending on the next flush.
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            conn.execute("UPDATE outbox_queue SET status = 'sending' WHERE id = ?1", rusqlite::params![id]).ok();
        }

        let mid = if message_id.is_empty() { None } else { Some(message_id.as_str()) };
        match smtp_client::send_email(&settings, &from_email, &from_name, &email, mid).await {
            Ok(_raw) => {
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                conn.execute("UPDATE outbox_queue SET status = 'sent' WHERE id = ?1", rusqlite::params![id]).ok();
                sent_count += 1;
            }
            Err(e) => {
                log::error!("Outbox retry failed for account {} queue entry {}: {}", account_id, id, e);
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE outbox_queue SET status = 'pending', error = ?2, retry_count = retry_count + 1 WHERE id = ?1",
                    rusqlite::params![id, e.to_string()],
                ).ok();
            }
        }
    }

    Ok(sent_count)
}

// ── Auto-discovery command ───────────────────────────────

#[tauri::command]
async fn discover_mail_settings(email: String) -> Result<autodiscover::DiscoveredConfig, String> {
    autodiscover::discover(&email)
        .await
        .map_err(|e| {
            log::error!("Mail auto-discovery failed for {}: {}", email, e);
            format!("Auto-discovery failed: {}", e)
        })
}

// ── Connection test commands ────────────────────────────

#[tauri::command]
async fn test_imap_connection(settings: sync_state::MailServerSettings) -> Result<(), String> {
    let mut session = imap_client::connect(&settings)
        .await
        .map_err(|e| {
            log::error!("IMAP connection test failed for server {}:{}: {}", settings.incoming_server, settings.incoming_port, e);
            format!("IMAP connection failed: {}", e)
        })?;
    session.logout().await.ok();
    Ok(())
}

#[tauri::command]
async fn test_smtp_connection(settings: sync_state::MailServerSettings) -> Result<(), String> {
    smtp_client::test_connection(&settings)
        .await
        .map_err(|e| {
            log::error!("SMTP connection test failed for server {}:{}: {}", settings.smtp_server, settings.smtp_port, e);
            format!("SMTP connection failed: {}", e)
        })
}

#[tauri::command]
async fn test_caldav_connection(url: String, username: String, password: String) -> Result<(), String> {
    caldav_client::test_connection(&url, &username, &password)
        .await
        .map_err(|e| {
            log::error!("CalDAV connection test failed for {}: {}", url, e);
            format!("CalDAV connection failed: {}", e)
        })
}

#[tauri::command]
async fn test_carddav_connection(url: String, username: String, password: String) -> Result<(), String> {
    carddav_client::test_connection(&url, &username, &password)
        .await
        .map_err(|e| {
            log::error!("CardDAV connection test failed for {}: {}", url, e);
            format!("CardDAV connection failed: {}", e)
        })
}

// ── CalDAV sync command ─────────────────────────────────

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct CalSyncResult {
    new_count: usize,
    updated_count: usize,
    deleted_count: usize,
}

#[tauri::command]
async fn sync_calendars(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<CalSyncResult, String> {
    log::info!("sync_calendars: starting for account {}", account_id);
    let dav_settings = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        sync_state::load_caldav_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No CalDAV settings configured")?
    };

    // Discover calendars
    let caldav_url = normalize_url(&dav_settings.url);
    let (home_set_url, calendars, default_cal_url) = caldav_client::discover_calendars(
        &caldav_url, &dav_settings.username, &dav_settings.password,
    )
    .await
    .map_err(|e| {
        log::error!("CalDAV discovery failed for account {}: {}", account_id, e);
        format!("CalDAV discovery failed: {}", e)
    })?;

    // ── Clean up phantom collection rows ──────────────────────────────────
    // These are http-URL rows with empty ctag that were created by a previous MKCALENDAR
    // run before we switched to using the canonical server URL.  They have no events or
    // resource-state entries, so they are safe to delete.
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM caldav_collection_state
             WHERE account_id = ?1
               AND collection_url LIKE 'http%'
               AND (ctag = '' OR ctag IS NULL)
               AND NOT EXISTS (
                   SELECT 1 FROM calendar_events e
                   WHERE e.account_id = caldav_collection_state.account_id
                     AND e.calendar_id = caldav_collection_state.collection_url
               )",
            rusqlite::params![account_id],
        ).ok();
    }

    // ── Push local-only collections to the server ─────────────────────────
    // Local collections have placeholder URLs (not http/https).
    // For each one, check if the server already has a matching name; if not, create it.
    // Then update the local collection_url and push all local events.
    {
        let local_collections: Vec<(String, String, String)> = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(
                "SELECT collection_url, display_name, color FROM caldav_collection_state
                 WHERE account_id = ?1 AND collection_url NOT LIKE 'http%'"
            ).map_err(|e| e.to_string())?;
            let results: Vec<(String, String, String)> = stmt.query_map(rusqlite::params![account_id], |row| {
                Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
            results
        };

        for (local_url, display_name, color) in local_collections {
            // Check if server already has a calendar with the same name (case-insensitive)
            let server_match = calendars.iter().find(|c| {
                c.display_name.to_lowercase() == display_name.to_lowercase()
            });

            let real_url = match server_match {
                Some(c) => {
                    log::info!("CalDAV: local collection \"{}\" matched server calendar {}", display_name, c.url);
                    c.url.clone()
                }
                None => {
                    // Create it on the server
                    match caldav_client::create_calendar(
                        &home_set_url, &display_name, &color,
                        &dav_settings.username, &dav_settings.password,
                    ).await {
                        Ok(_) => {
                            // Servers typically assign their own canonical URL regardless of
                            // the path we chose (e.g. /caldav/Y2FsOi8vMC82MQ/ instead of
                            // /caldav/personal/).  Re-list the home-set to find the real URL.
                            match caldav_client::list_calendar_collections(
                                &home_set_url, &dav_settings.username, &dav_settings.password,
                            ).await {
                                Ok(refreshed) => {
                                    match refreshed.into_iter().find(|c| {
                                        c.display_name.to_lowercase() == display_name.to_lowercase()
                                    }) {
                                        Some(c) => {
                                            log::info!("CalDAV: created calendar \"{}\" — canonical URL = {}", display_name, c.url);
                                            c.url
                                        }
                                        None => {
                                            log::warn!("CalDAV: created \"{}\" but could not find it in re-discovery — skipping", display_name);
                                            continue;
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!("CalDAV: post-create re-discovery failed: {} — skipping", e);
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("CalDAV: could not create calendar \"{}\": {} — keeping local", display_name, e);
                            continue;
                        }
                    }
                }
            };

            // Re-home events/resource-state to the server URL, then delete the local
            // placeholder row.  Server wins on any collision — the regular sync loop
            // below will upsert the server row with authoritative ctag/color/name.
            {
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE calendar_events SET calendar_id = ?2 WHERE account_id = ?1 AND calendar_id = ?3",
                    rusqlite::params![account_id, real_url, local_url],
                ).ok();
                conn.execute(
                    "UPDATE caldav_resource_state SET collection_url = ?2, resource_url = REPLACE(resource_url, 'local://', ?2)
                     WHERE account_id = ?1 AND collection_url = ?3",
                    rusqlite::params![account_id, real_url, local_url],
                ).ok();
                // DELETE instead of UPDATE — avoids unique-constraint conflict when the
                // server row already exists.  The server row is authoritative.
                conn.execute(
                    "DELETE FROM caldav_collection_state WHERE account_id = ?1 AND collection_url = ?2",
                    rusqlite::params![account_id, local_url],
                ).ok();
            } // conn dropped here

            // Query events to push — separate scope so conn is freed before the for loop's .await
            let events_to_push: Vec<(String, String, String, String, Option<String>, Option<String>, bool)> = {
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                let mut stmt = conn.prepare(
                    "SELECT e.id, e.title, e.start, e.end, e.location, e.description, e.is_all_day
                     FROM calendar_events e
                     WHERE e.account_id = ?1 AND e.calendar_id = ?2"
                ).map_err(|e| e.to_string())?;
                let results: Vec<(String, String, String, String, Option<String>, Option<String>, bool)> = stmt.query_map(rusqlite::params![account_id, real_url], |row| {
                    Ok((
                        row.get::<_,String>(0)?, row.get::<_,String>(1)?,
                        row.get::<_,String>(2)?, row.get::<_,String>(3)?,
                        row.get::<_,Option<String>>(4)?, row.get::<_,Option<String>>(5)?,
                        row.get::<_,bool>(6)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
                results
            }; // conn dropped here

            for (eid, title, start, end, location, description, is_all_day) in events_to_push {
                let parsed = caldav_client::ParsedEvent {
                    uid: eid.clone(),
                    summary: title,
                    dtstart: start,
                    dtend: end,
                    location,
                    description,
                    is_all_day,
                    organizer: None,
                    attendees: vec![],
                    rrule: None,
                    exdates: None,
                    alert_minutes: None,
                    resource_url: String::new(), // new resource
                    etag: String::new(),
                };
                match caldav_client::put_event(
                    &real_url, &dav_settings.username, &dav_settings.password, &parsed,
                ).await {
                    Ok(new_etag) => {
                        let new_res_url = format!("{}/{}.ics", real_url.trim_end_matches('/'), eid);
                        let conn = state.db.lock().map_err(|e| e.to_string())?;
                        conn.execute(
                            "UPDATE caldav_resource_state SET resource_url = ?3, etag = ?4, uid = ?5
                             WHERE account_id = ?1 AND event_id = ?2",
                            rusqlite::params![account_id, eid, new_res_url, new_etag, eid],
                        ).ok();
                    }
                    Err(e) => log::warn!("CalDAV: push of event {} failed: {}", eid, e),
                }
            }
        }
    }

    // Classify each calendar into "my" or "other" group.
    // "other" = system/auto-generated calendars (Birthdays, Holidays).
    // "my"    = user-created calendars + the provider's default calendar.
    // The provider's default calendar (identified via RFC 6638
    // schedule-default-calendar-URL) is renamed to "Calendar" since providers
    // often name it after the account holder (e.g. "Nakamoto, Takeshi").
    let well_known_other: std::collections::HashSet<&str> =
        ["birthdays", "holidays", "anniversaries"].iter().copied().collect();

    // Rename the provider's default calendar to "Calendar".
    let well_known_names: std::collections::HashSet<&str> =
        ["personal", "work", "birthdays", "holidays", "anniversaries", "calendar"].iter().copied().collect();

    let mut calendars = calendars;
    if let Some(ref default_url) = default_cal_url {
        // RFC 6638: use the server-advertised default calendar URL
        for cal in calendars.iter_mut() {
            if cal.url == *default_url {
                log::info!("CalDAV: renaming default calendar \"{}\" → \"Calendar\" (via schedule-default-calendar-URL)", cal.display_name);
                cal.display_name = "Calendar".to_string();
            }
        }
    } else {
        // Fallback: if no schedule-default-calendar-URL, the provider's default
        // is typically the one with a name that doesn't match any well-known
        // calendar name (providers name it after the account holder).
        // Only apply when exactly one calendar matches — multiple candidates are
        // ambiguous and collapsing them all to "Calendar" produces duplicates.
        let candidate_count = calendars.iter().filter(|cal| {
            let lower = cal.display_name.to_lowercase();
            !well_known_names.contains(lower.as_str()) && !well_known_other.contains(lower.as_str())
        }).count();
        if candidate_count == 1 {
            for cal in calendars.iter_mut() {
                let lower = cal.display_name.to_lowercase();
                if !well_known_names.contains(lower.as_str()) && !well_known_other.contains(lower.as_str()) {
                    log::info!("CalDAV: renaming likely default calendar \"{}\" → \"Calendar\" (name heuristic)", cal.display_name);
                    cal.display_name = "Calendar".to_string();
                }
            }
        } else if candidate_count > 1 {
            log::info!("CalDAV: skipping default-calendar rename heuristic — {} ambiguous candidates", candidate_count);
        }
    }

    let cal_groups: std::collections::HashMap<String, &str> = calendars.iter().map(|cal| {
        let lower_name = cal.display_name.to_lowercase();
        let group = if well_known_other.contains(lower_name.as_str()) {
            "other"
        } else {
            "my"
        };
        (cal.url.clone(), group)
    }).collect();

    let mut total_new = 0usize;
    let mut total_updated = 0usize;
    let mut total_deleted = 0usize;

    for cal in &calendars {
        let calendar_group = cal_groups.get(&cal.url).copied().unwrap_or("my");
        // Get known ctag
        let known_ctag = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT ctag FROM caldav_collection_state WHERE account_id = ?1 AND collection_url = ?2",
                rusqlite::params![account_id, cal.url],
                |row| row.get::<_, String>(0),
            ).unwrap_or_default()
        };

        let known_etags: Vec<(String, String, String)> = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(
                "SELECT resource_url, etag, event_id FROM caldav_resource_state
                 WHERE account_id = ?1 AND collection_url = ?2"
            ).map_err(|e| e.to_string())?;
            let results: Vec<(String, String, String)> = stmt.query_map(rusqlite::params![account_id, cal.url], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
            results
        };

        let (new_events, updated_events, deleted_urls) = caldav_client::sync_calendar(
            &cal.url, &dav_settings.username, &dav_settings.password,
            &known_etags, &known_ctag, &cal.ctag,
        )
        .await
        .map_err(|e| {
            log::error!(
                "CalDAV sync failed for account {} calendar {} ({}): {}",
                account_id,
                cal.display_name,
                cal.url,
                e
            );
            format!("CalDAV sync failed for {}: {}", cal.display_name, e)
        })?;

        // Apply to local DB
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;

            // Ensure collection entry exists; always update group and display_name
            // but preserve user's visible flag via ON CONFLICT
            conn.execute(
                "INSERT INTO caldav_collection_state (account_id, collection_url, ctag, display_name, color, visible, calendar_group)
                 VALUES (?1, ?2, '', ?3, ?4, 1, ?5)
                 ON CONFLICT(account_id, collection_url) DO UPDATE SET
                     calendar_group=excluded.calendar_group,
                     display_name=excluded.display_name",
                rusqlite::params![account_id, cal.url, cal.display_name, cal.color, calendar_group],
            ).ok();

            // Insert new events
            for evt in &new_events {
                let event_id = uuid::Uuid::new_v4().to_string();
                let exdates_csv = evt.exdates.as_ref().map(|v| v.join(","));
                conn.execute(
                    "INSERT OR IGNORE INTO calendar_events
                     (id, account_id, title, start, end, color, location, description, is_all_day, calendar_id, is_online_meeting, rrule, exdates, alert_minutes)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,?11,?12,?13)",
                    rusqlite::params![
                        event_id, account_id, evt.summary, evt.dtstart, evt.dtend,
                        cal.color, evt.location, evt.description, evt.is_all_day as i32, cal.url, evt.rrule,
                        exdates_csv, evt.alert_minutes,
                    ],
                ).ok();

                // Track etag + original CalDAV UID (needed to PUT updates with the correct UID)
                conn.execute(
                    "INSERT OR REPLACE INTO caldav_resource_state (account_id, collection_url, resource_url, etag, event_id, uid)
                     VALUES (?1,?2,?3,?4,?5,?6)",
                    rusqlite::params![account_id, cal.url, evt.resource_url, evt.etag, event_id, evt.uid],
                ).ok();

                // Store attendees
                for (name, email, role) in &evt.attendees {
                    let initials = make_initials(name, email);
                    let color = make_avatar_color(email);
                    conn.execute(
                        "INSERT INTO event_attendees (event_id, name, email, initials, color, role) VALUES (?1,?2,?3,?4,?5,?6)",
                        rusqlite::params![event_id, name, email, initials, color, role],
                    ).ok();
                }
                total_new += 1;
            }

            // Update existing events
            for evt in &updated_events {
                let event_id: Option<String> = conn.query_row(
                    "SELECT event_id FROM caldav_resource_state WHERE account_id = ?1 AND resource_url = ?2",
                    rusqlite::params![account_id, evt.resource_url],
                    |row| row.get(0),
                ).ok();

                if let Some(event_id) = event_id {
                    let exdates_csv = evt.exdates.as_ref().map(|v| v.join(","));
                    conn.execute(
                        "UPDATE calendar_events SET title=?2, start=?3, end=?4, location=?5, description=?6, is_all_day=?7, rrule=?8, exdates=?9, alert_minutes=?10 WHERE id=?1",
                        rusqlite::params![event_id, evt.summary, evt.dtstart, evt.dtend, evt.location, evt.description, evt.is_all_day as i32, evt.rrule, exdates_csv, evt.alert_minutes],
                    ).ok();

                    conn.execute(
                        "UPDATE caldav_resource_state SET etag = ?3 WHERE account_id = ?1 AND resource_url = ?2",
                        rusqlite::params![account_id, evt.resource_url, evt.etag],
                    ).ok();

                    // Refresh attendees
                    conn.execute("DELETE FROM event_attendees WHERE event_id = ?1", rusqlite::params![event_id]).ok();
                    for (name, email, role) in &evt.attendees {
                        let initials = make_initials(name, email);
                        let color = make_avatar_color(email);
                        conn.execute(
                            "INSERT INTO event_attendees (event_id, name, email, initials, color, role) VALUES (?1,?2,?3,?4,?5,?6)",
                            rusqlite::params![event_id, name, email, initials, color, role],
                        ).ok();
                    }
                    total_updated += 1;
                }
            }

            // Delete removed events
            for url in &deleted_urls {
                let event_id: Option<String> = conn.query_row(
                    "SELECT event_id FROM caldav_resource_state WHERE account_id = ?1 AND resource_url = ?2",
                    rusqlite::params![account_id, url],
                    |row| row.get(0),
                ).ok();
                if let Some(event_id) = event_id {
                    conn.execute("DELETE FROM calendar_events WHERE id = ?1", rusqlite::params![event_id]).ok();
                }
                conn.execute(
                    "DELETE FROM caldav_resource_state WHERE account_id = ?1 AND resource_url = ?2",
                    rusqlite::params![account_id, url],
                ).ok();
                total_deleted += 1;
            }

            // Update sync state; ON CONFLICT DO UPDATE preserves the user's visible flag
            conn.execute(
                "INSERT INTO caldav_collection_state (account_id, collection_url, ctag, display_name, color, last_sync, visible, calendar_group)
                 VALUES (?1,?2,?3,?4,?5,datetime('now'),1,?6)
                 ON CONFLICT(account_id, collection_url) DO UPDATE SET
                     ctag=excluded.ctag, display_name=excluded.display_name,
                     color=excluded.color, last_sync=excluded.last_sync,
                     calendar_group=excluded.calendar_group",
                rusqlite::params![account_id, cal.url, cal.ctag, cal.display_name, cal.color, calendar_group],
            ).ok();
        }
    }

    log::info!("sync_calendars: done for account {} — {} new, {} updated, {} deleted", account_id, total_new, total_updated, total_deleted);
    Ok(CalSyncResult { new_count: total_new, updated_count: total_updated, deleted_count: total_deleted })
}

// ── CardDAV sync command ────────────────────────────────

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct ContactSyncResult {
    new_count: usize,
    updated_count: usize,
    deleted_count: usize,
}

#[tauri::command]
async fn sync_contacts(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<ContactSyncResult, String> {
    log::info!("sync_contacts: starting for account {}", account_id);

    // ── Cleanup: fix stale sync state and duplicates ──────────────────────
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;

        // If we have stored ctags but no contacts, the sync state is stale —
        // reset it so a fresh pull happens.
        let contact_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM contacts WHERE account_id = ?1",
            rusqlite::params![account_id],
            |row| row.get(0),
        ).unwrap_or(0);

        let ctag_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM carddav_collection_state WHERE account_id = ?1 AND ctag != ''",
            rusqlite::params![account_id],
            |row| row.get(0),
        ).unwrap_or(0);

        if contact_count == 0 && ctag_count > 0 {
            log::info!("sync_contacts: stale sync state detected (0 contacts but {} ctags) — resetting for account {}", ctag_count, account_id);
            conn.execute(
                "UPDATE carddav_collection_state SET ctag = '' WHERE account_id = ?1",
                rusqlite::params![account_id],
            ).ok();
            conn.execute(
                "DELETE FROM carddav_resource_state WHERE account_id = ?1",
                rusqlite::params![account_id],
            ).ok();
        }

        // Reset ctag for collections that have a ctag but no resource entries —
        // the ctag was stored but contacts were never tracked (e.g. deduped away).
        conn.execute(
            "UPDATE carddav_collection_state SET ctag = ''
             WHERE account_id = ?1 AND ctag != ''
               AND collection_url NOT IN (
                   SELECT DISTINCT collection_url FROM carddav_resource_state WHERE account_id = ?1
               )",
            rusqlite::params![account_id],
        ).ok();

        if contact_count > 0 {
            // Remove duplicate contacts (same name+email) created before dedup was added
            let deleted_dupes: usize = conn.execute(
                "DELETE FROM contacts WHERE id IN (
                    SELECT c.id FROM contacts c
                    WHERE c.account_id = ?1
                      AND c.id NOT IN (
                        SELECT MIN(c2.id) FROM contacts c2
                        WHERE c2.account_id = ?1
                        GROUP BY c2.name, c2.email
                      )
                )",
                rusqlite::params![account_id],
            ).unwrap_or(0);
            if deleted_dupes > 0 {
                log::info!("sync_contacts: cleaned up {} duplicate contacts for account {}", deleted_dupes, account_id);
                conn.execute(
                    "UPDATE carddav_collection_state SET ctag = '' WHERE account_id = ?1",
                    rusqlite::params![account_id],
                ).ok();
                conn.execute(
                    "DELETE FROM carddav_resource_state WHERE account_id = ?1 AND contact_id NOT IN (SELECT id FROM contacts)",
                    rusqlite::params![account_id],
                ).ok();
            }
        }
    }

    let dav_settings = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        sync_state::load_carddav_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No CardDAV settings configured")?
    };

    let carddav_url = normalize_url(&dav_settings.url);
    let (ab_home_set_url, books) = carddav_client::discover_address_books(
        &carddav_url, &dav_settings.username, &dav_settings.password,
    )
    .await
    .map_err(|e| {
        log::error!("CardDAV discovery failed for account {}: {}", account_id, e);
        format!("CardDAV discovery failed: {}", e)
    })?;

    // ── Push local-only address books to the server ───────────────────────
    {
        let local_books: Vec<(String, String)> = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(
                "SELECT collection_url, display_name FROM carddav_collection_state
                 WHERE account_id = ?1 AND collection_url NOT LIKE 'http%'"
            ).map_err(|e| e.to_string())?;
            let results: Vec<(String, String)> = stmt.query_map(rusqlite::params![account_id], |row| {
                Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
            results
        };

        for (local_url, display_name) in local_books {
            let server_match = books.iter().find(|b| {
                b.display_name.to_lowercase() == display_name.to_lowercase()
            });

            let real_url = match server_match {
                Some(b) => {
                    log::info!("CardDAV: local address book \"{}\" matched server book {}", display_name, b.url);
                    b.url.clone()
                }
                None => {
                    match carddav_client::create_address_book(
                        &ab_home_set_url, &display_name,
                        &dav_settings.username, &dav_settings.password,
                    ).await {
                        Ok(url) => {
                            log::info!("CardDAV: created server address book \"{}\" at {}", display_name, url);
                            url
                        }
                        Err(e) => {
                            log::warn!("CardDAV: could not create address book \"{}\": {} — keeping local", display_name, e);
                            continue;
                        }
                    }
                }
            };

            let conn = state.db.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE carddav_resource_state SET collection_url = ?2
                 WHERE account_id = ?1 AND collection_url = ?3",
                rusqlite::params![account_id, real_url, local_url],
            ).ok();
            conn.execute(
                "UPDATE carddav_collection_state SET collection_url = ?2
                 WHERE account_id = ?1 AND collection_url = ?3",
                rusqlite::params![account_id, real_url, local_url],
            ).ok();
            // (contacts themselves don't store collection_url, only resource_state does)
        }
    }

    let mut total_new = 0usize;
    let mut total_updated = 0usize;
    let mut total_deleted = 0usize;

    // Track vCard UIDs we've already inserted in this sync run so that the same
    // contact appearing in multiple address books (Global, Collected, Contacts)
    // is only stored once.  Maps vCard UID → local contact_id.
    let mut synced_uids: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for book in &books {
        let known_ctag = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT ctag FROM carddav_collection_state WHERE account_id = ?1 AND collection_url = ?2",
                rusqlite::params![account_id, book.url],
                |row| row.get::<_, String>(0),
            ).unwrap_or_default()
        };

        let known_etags: Vec<(String, String, String)> = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(
                "SELECT resource_url, etag, contact_id FROM carddav_resource_state
                 WHERE account_id = ?1 AND collection_url = ?2"
            ).map_err(|e| e.to_string())?;
            let results: Vec<(String, String, String)> = stmt.query_map(rusqlite::params![account_id, book.url], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
            results
        };

        let (new_contacts, updated_contacts, deleted_urls) = carddav_client::sync_address_book(
            &book.url, &dav_settings.username, &dav_settings.password,
            &known_etags, &known_ctag, &book.ctag,
        )
        .await
        .map_err(|e| {
            log::error!(
                "CardDAV sync failed for account {} address book {} ({}): {}",
                account_id,
                book.display_name,
                book.url,
                e
            );
            format!("CardDAV sync failed for {}: {}", book.display_name, e)
        })?;

        // Download remote photo URLs as data URIs so they're cached locally
        let mut new_contacts = new_contacts;
        let mut updated_contacts = updated_contacts;
        for contact in new_contacts.iter_mut().chain(updated_contacts.iter_mut()) {
            if let Some(ref url) = contact.photo_url {
                if url.starts_with("http://") || url.starts_with("https://") {
                    contact.photo_url = carddav_client::download_photo_as_data_uri(url).await;
                }
            }
        }

        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;

            // Insert new contacts — deduplicate by vCard UID across address books
            for contact in &new_contacts {
                let contact_email = contact.emails.first().map(|e| e.email.to_lowercase()).unwrap_or_default();

                // If we already synced a contact with this vCard UID from another
                // address book, reuse the existing contact_id and skip the insert.
                if let Some(existing_id) = synced_uids.get(&contact.uid) {
                    log::info!(
                        "CardDAV: skipping duplicate contact \"{}\" (UID {}) from book \"{}\", already synced as {}",
                        contact.full_name, contact.uid, book.display_name, existing_id
                    );
                    // Still track the resource_state so etag sync works for this book
                    conn.execute(
                        "INSERT OR REPLACE INTO carddav_resource_state (account_id, collection_url, resource_url, etag, contact_id)
                         VALUES (?1,?2,?3,?4,?5)",
                        rusqlite::params![account_id, book.url, contact.resource_url, contact.etag, existing_id],
                    ).ok();
                    continue;
                }

                let contact_id = uuid::Uuid::new_v4().to_string();
                let email = contact.emails.first().map(|e| e.email.as_str()).unwrap_or("");
                let initials = make_initials(&contact.full_name, email);
                let color = make_avatar_color(email);
                let phone = contact.phones.iter().find(|p| p.label == "work" || p.label == "other")
                    .map(|p| p.number.clone());
                let mobile = contact.phones.iter().find(|p| p.label == "cell" || p.label == "mobile")
                    .map(|p| p.number.clone());
                let address_scalar = contact.addresses.first().map(|a| {
                    [&a.street, &a.city, &a.region, &a.postal_code, &a.country]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                });

                conn.execute(
                    "INSERT OR IGNORE INTO contacts
                     (id, account_id, name, first_name, last_name, middle_name, prefix, suffix, email, initials, color, phone, mobile, job_title, department, organization, address, birthday, photo_url, is_favorite, rev)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,0,?20)",
                    rusqlite::params![
                        contact_id, account_id, contact.full_name,
                        contact.first_name, contact.last_name, contact.middle_name,
                        contact.prefix, contact.suffix,
                        email, initials, color, phone, mobile,
                        if contact.title.is_empty() { None } else { Some(&contact.title) },
                        contact.department,
                        if contact.organization.is_empty() { None } else { Some(&contact.organization) },
                        address_scalar,
                        contact.birthday,
                        contact.photo_url,
                        contact.rev,
                    ],
                ).ok();

                write_contact_children(&conn, &contact_id, &contact.emails, &contact.phones, &contact.addresses);

                conn.execute(
                    "INSERT OR REPLACE INTO carddav_resource_state (account_id, collection_url, resource_url, etag, contact_id)
                     VALUES (?1,?2,?3,?4,?5)",
                    rusqlite::params![account_id, book.url, contact.resource_url, contact.etag, contact_id],
                ).ok();

                // Remember this UID so subsequent address books won't create duplicates
                if !contact.uid.is_empty() {
                    synced_uids.insert(contact.uid.clone(), contact_id.clone());
                }
                total_new += 1;
            }

            // Update existing contacts — also deduplicate by vCard UID
            for contact in &updated_contacts {
                // If this UID was already handled from another address book, just update resource_state
                if !contact.uid.is_empty() {
                    if let Some(existing_id) = synced_uids.get(&contact.uid) {
                        conn.execute(
                            "INSERT OR REPLACE INTO carddav_resource_state (account_id, collection_url, resource_url, etag, contact_id)
                             VALUES (?1,?2,?3,?4,?5)",
                            rusqlite::params![account_id, book.url, contact.resource_url, contact.etag, existing_id],
                        ).ok();
                        continue;
                    }
                }

                let contact_id: Option<String> = conn.query_row(
                    "SELECT contact_id FROM carddav_resource_state WHERE account_id = ?1 AND resource_url = ?2",
                    rusqlite::params![account_id, contact.resource_url],
                    |row| row.get(0),
                ).ok();

                if let Some(contact_id) = contact_id {
                    let email = contact.emails.first().map(|e| e.email.as_str()).unwrap_or("");
                    let phone = contact.phones.iter().find(|p| p.label == "work" || p.label == "other")
                        .map(|p| p.number.clone());
                    let mobile = contact.phones.iter().find(|p| p.label == "cell" || p.label == "mobile")
                        .map(|p| p.number.clone());
                    let address_scalar = contact.addresses.first().map(|a| {
                        [&a.street, &a.city, &a.region, &a.postal_code, &a.country]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    });

                    conn.execute(
                        "UPDATE contacts SET name=?2, email=?3, first_name=?4, last_name=?5, middle_name=?6, prefix=?7, suffix=?8, phone=?9, mobile=?10, job_title=?11, department=?12, organization=?13, address=?14, birthday=?15, photo_url=?16, rev=?17 WHERE id=?1",
                        rusqlite::params![
                            contact_id, contact.full_name, email,
                            contact.first_name, contact.last_name, contact.middle_name,
                            contact.prefix, contact.suffix,
                            phone, mobile,
                            if contact.title.is_empty() { None } else { Some(&contact.title) },
                            contact.department,
                            if contact.organization.is_empty() { None } else { Some(&contact.organization) },
                            address_scalar,
                            contact.birthday,
                            contact.photo_url,
                            contact.rev,
                        ],
                    ).ok();

                    write_contact_children(&conn, &contact_id, &contact.emails, &contact.phones, &contact.addresses);

                    conn.execute(
                        "UPDATE carddav_resource_state SET etag = ?3 WHERE account_id = ?1 AND resource_url = ?2",
                        rusqlite::params![account_id, contact.resource_url, contact.etag],
                    ).ok();

                    if !contact.uid.is_empty() {
                        synced_uids.insert(contact.uid.clone(), contact_id.clone());
                    }
                    total_updated += 1;
                }
            }

            // Delete removed contacts
            for url in &deleted_urls {
                let contact_id: Option<String> = conn.query_row(
                    "SELECT contact_id FROM carddav_resource_state WHERE account_id = ?1 AND resource_url = ?2",
                    rusqlite::params![account_id, url],
                    |row| row.get(0),
                ).ok();
                if let Some(contact_id) = contact_id {
                    conn.execute("DELETE FROM contacts WHERE id = ?1", rusqlite::params![contact_id]).ok();
                }
                conn.execute(
                    "DELETE FROM carddav_resource_state WHERE account_id = ?1 AND resource_url = ?2",
                    rusqlite::params![account_id, url],
                ).ok();
                total_deleted += 1;
            }

            // Update collection ctag and read_only flag
            conn.execute(
                "INSERT INTO carddav_collection_state (account_id, collection_url, ctag, display_name, read_only, last_sync)
                 VALUES (?1,?2,?3,?4,?5,datetime('now'))
                 ON CONFLICT(account_id, collection_url) DO UPDATE SET ctag=excluded.ctag, display_name=excluded.display_name, read_only=excluded.read_only, last_sync=excluded.last_sync",
                rusqlite::params![account_id, book.url, book.ctag, book.display_name, book.read_only as i32],
            ).ok();
        }
    }

    // If the account has no avatar, try to use a matching contact's photo
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let has_avatar: bool = conn.query_row(
            "SELECT avatar_url IS NOT NULL AND avatar_url != '' FROM accounts WHERE id = ?1",
            rusqlite::params![account_id],
            |row| row.get(0),
        ).unwrap_or(false);

        if !has_avatar {
            let account_email: String = conn.query_row(
                "SELECT email FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
                |row| row.get(0),
            ).unwrap_or_default();

            if !account_email.is_empty() {
                let photo: Option<String> = conn.query_row(
                    "SELECT photo_url FROM contacts c
                     WHERE c.account_id = ?1
                       AND c.photo_url IS NOT NULL AND c.photo_url != ''
                       AND (LOWER(c.email) = LOWER(?2)
                            OR EXISTS (SELECT 1 FROM contact_emails ce
                                       WHERE ce.contact_id = c.id
                                         AND LOWER(ce.address) = LOWER(?2)))
                     LIMIT 1",
                    rusqlite::params![account_id, account_email],
                    |row| row.get(0),
                ).ok();

                if let Some(photo_url) = photo {
                    conn.execute(
                        "UPDATE accounts SET avatar_url = ?2 WHERE id = ?1",
                        rusqlite::params![account_id, photo_url],
                    ).ok();
                    log::info!("sync_contacts: set account {} avatar from matching contact photo", account_id);
                }
            }
        }
    }

    log::info!("sync_contacts: done for account {} — {} new, {} updated, {} deleted", account_id, total_new, total_updated, total_deleted);
    Ok(ContactSyncResult { new_count: total_new, updated_count: total_updated, deleted_count: total_deleted })
}

// ── CalDAV CRUD commands ────────────────────────────────

#[tauri::command]
async fn save_calendar_event(
    state: tauri::State<'_, AppState>,
    account_id: String,
    event_id: String,
    title: String,
    start: String,
    end: String,
    location: Option<String>,
    description: Option<String>,
    is_all_day: bool,
    calendar_id: String,
    attendees: Vec<AttendeeParam>,
    recurrence: Option<RecurrenceParam>,
    is_online_meeting: bool,
    meeting_url: Option<String>,
    alert_minutes: Option<i32>,
) -> Result<String, String> {
    // Load CalDAV settings (optional — events can be saved locally without CalDAV)
    let dav_settings = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        sync_state::load_caldav_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
    };

    // Look up existing resource_url + etag + uid (if updating an existing event)
    let existing: Option<(String, String, String)> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT rs.resource_url, rs.etag, rs.uid FROM caldav_resource_state rs WHERE rs.account_id = ?1 AND rs.event_id = ?2",
            rusqlite::params![account_id, event_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok()
    };

    let is_new = existing.is_none();
    // For updates, use the original CalDAV UID stored at sync time (may differ from our local event_id).
    // For new events, generate a fresh UUID that becomes both the local id and the CalDAV UID.
    let uid = match &existing {
        Some((_, _, stored_uid)) if !stored_uid.is_empty() => stored_uid.clone(),
        _ => if is_new { uuid::Uuid::new_v4().to_string() } else { event_id.clone() },
    };

    // If CalDAV is configured AND the calendar is a real server collection, push to the server.
    // A PUT failure is non-fatal: log and fall back to local-only storage.
    let rrule_str: Option<String> = recurrence.as_ref().map(recurrence_to_rrule);
    let exdates_str: Option<String> = recurrence.as_ref()
        .and_then(|r| r.exdates.as_ref())
        .map(|v| v.join(","));
    let exdates_vec: Option<Vec<String>> = recurrence.as_ref()
        .and_then(|r| r.exdates.clone());
    let parsed_attendees: Vec<(String, String, String)> = attendees.iter()
        .map(|a| (a.name.clone(), a.email.clone(), a.role.clone()))
        .collect();

    // Look up account name/email for ORGANIZER property (needed when attendees exist)
    let organizer_info: Option<(String, String)> = if !attendees.is_empty() {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let info: Option<(String, String)> = conn.query_row(
            "SELECT name, email FROM accounts WHERE id = ?1",
            rusqlite::params![account_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok();
        info
    } else {
        None
    };

    let is_server_calendar = calendar_id.starts_with("http://") || calendar_id.starts_with("https://");
    let caldav_etag: Option<String> = if let Some(dav) = dav_settings {
        if is_server_calendar {
            let (resource_url, etag, _) = existing.clone().unwrap_or_default();
            let parsed = caldav_client::ParsedEvent {
                uid: uid.clone(),
                summary: title.clone(),
                dtstart: start.clone(),
                dtend: end.clone(),
                location: location.clone(),
                description: description.clone(),
                is_all_day,
                organizer: organizer_info.clone(),
                attendees: parsed_attendees.clone(),
                rrule: rrule_str.clone(),
                exdates: exdates_vec.clone(),
                alert_minutes,
                resource_url,
                etag,
            };
            match caldav_client::put_event(&calendar_id, &dav.username, &dav.password, &parsed).await {
                Ok(etag) => Some(etag),
                Err(e) => {
                    log::warn!("CalDAV PUT failed (saving locally only): {}", e);
                    None
                }
            }
        } else {
            None // local-only calendar (e.g. "personal")
        }
    } else {
        None
    };

    // Persist to local DB
    let local_id = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        if is_new {
            let local_id = uid.clone(); // same UUID as CalDAV UID — keeps local id and server UID in sync
            // Look up calendar color from caldav_collection_state (single source of truth)
            let color: String = conn.query_row(
                "SELECT color FROM caldav_collection_state WHERE account_id = ?1 AND collection_url = ?2",
                rusqlite::params![account_id, calendar_id],
                |row| row.get(0),
            ).unwrap_or_else(|_| "#0078d4".to_string());

            conn.execute(
                "INSERT INTO calendar_events (id, account_id, title, start, end, color, location, description, is_all_day, calendar_id, is_online_meeting, meeting_url, rrule, exdates, alert_minutes)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params![local_id, account_id, title, start, end, color, location, description, is_all_day as i32, calendar_id, is_online_meeting as i32, meeting_url, rrule_str, exdates_str, alert_minutes],
            ).map_err(|e| e.to_string())?;

            // Insert attendees
            for att in &attendees {
                let initials = make_initials(&att.name, &att.email);
                let att_color = make_avatar_color(&att.email);
                conn.execute(
                    "INSERT INTO event_attendees (event_id, name, email, initials, color, role) VALUES (?1,?2,?3,?4,?5,?6)",
                    rusqlite::params![local_id, att.name, att.email, initials, att_color, att.role],
                ).ok();
            }

            // Always track in caldav_resource_state — local events use a local:// URL with empty etag
            // so they can be identified and pushed when CalDAV is later configured
            let res_url = match &caldav_etag {
                Some(_) => format!("{}/{}.ics", calendar_id.trim_end_matches('/'), uid),
                None    => format!("local://{}.ics", uid),
            };
            let etag_val = caldav_etag.as_deref().unwrap_or("");
            conn.execute(
                "INSERT OR REPLACE INTO caldav_resource_state (account_id, collection_url, resource_url, etag, event_id, uid)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![account_id, calendar_id, res_url, etag_val, local_id, uid],
            ).map_err(|e| e.to_string())?;

            local_id
        } else {
            conn.execute(
                "UPDATE calendar_events SET title=?2, start=?3, end=?4, location=?5, description=?6, is_all_day=?7, calendar_id=?8, is_online_meeting=?9, meeting_url=?10, rrule=?11, exdates=?12, alert_minutes=?13 WHERE id=?1",
                rusqlite::params![event_id, title, start, end, location, description, is_all_day as i32, calendar_id, is_online_meeting as i32, meeting_url, rrule_str, exdates_str, alert_minutes],
            ).map_err(|e| e.to_string())?;

            // Refresh attendees
            conn.execute("DELETE FROM event_attendees WHERE event_id = ?1", rusqlite::params![event_id]).ok();
            for att in &attendees {
                let initials = make_initials(&att.name, &att.email);
                let att_color = make_avatar_color(&att.email);
                conn.execute(
                    "INSERT INTO event_attendees (event_id, name, email, initials, color, role) VALUES (?1,?2,?3,?4,?5,?6)",
                    rusqlite::params![event_id, att.name, att.email, initials, att_color, att.role],
                ).ok();
            }

            if let Some(etag) = caldav_etag {
                conn.execute(
                    "UPDATE caldav_resource_state SET etag = ?3 WHERE account_id = ?1 AND event_id = ?2",
                    rusqlite::params![account_id, event_id, etag],
                ).map_err(|e| e.to_string())?;
            }
            event_id.clone()
        }
    };

    // Send calendar invitations via SMTP if there are attendees
    if !attendees.is_empty() {
        let smtp_info = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let settings = sync_state::load_mail_settings(&conn, &account_id).ok().flatten();
            let from_email: Option<String> = conn.query_row(
                "SELECT email FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
                |row| row.get(0),
            ).ok();
            let from_name: Option<String> = conn.query_row(
                "SELECT name FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
                |row| row.get(0),
            ).ok();
            settings.and_then(|s| from_email.zip(from_name).map(|(e, n)| (s, e, n)))
        };
        if let Some((smtp_settings, from_email, from_name)) = smtp_info {
            // Build a ParsedEvent for the iMIP invitation ICS
            let ics_event = caldav_client::ParsedEvent {
                uid: uid.clone(),
                summary: title.clone(),
                dtstart: start.clone(),
                dtend: end.clone(),
                location: location.clone(),
                description: description.clone(),
                is_all_day,
                organizer: Some((from_name.clone(), from_email.clone())),
                attendees: parsed_attendees.clone(),
                rrule: rrule_str.clone(),
                exdates: exdates_vec.clone(),
                alert_minutes,
                resource_url: String::new(),
                etag: String::new(),
            };
            let ics = caldav_client::build_imip_invitation(&ics_event);
            let to_list: Vec<String> = attendees.iter().map(|a| {
                if a.name.is_empty() { a.email.clone() } else { format!("{} <{}>", a.name, a.email) }
            }).collect();
            let html_body = format!(
                "<p>You are invited to: <strong>{}</strong></p><p>Start: {}</p><p>End: {}</p>{}",
                title, start, end,
                location.as_deref().map(|l| format!("<p>Location: {}</p>", l)).unwrap_or_default()
            );
            if let Err(e) = smtp_client::send_invitation(
                &smtp_settings, &from_email, &from_name,
                &to_list, &format!("Invitation: {}", title), &html_body, &ics,
            ).await {
                log::warn!("Failed to send calendar invitations: {}", e);
            }
        }
    }

    Ok(local_id)
}

#[tauri::command]
async fn delete_calendar_event(
    state: tauri::State<'_, AppState>,
    account_id: String,
    event_id: String,
) -> Result<(), String> {
    // Attempt CalDAV server delete if settings + resource state are available (non-fatal)
    let caldav_info: Option<(sync_state::DavSettings, String, String)> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = sync_state::load_caldav_settings(&conn, &account_id)
            .ok()
            .flatten();
        let resource = settings.as_ref().and_then(|_| {
            conn.query_row(
                "SELECT resource_url, etag FROM caldav_resource_state WHERE account_id = ?1 AND event_id = ?2",
                rusqlite::params![account_id, event_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            ).ok()
        });
        match (settings, resource) {
            (Some(s), Some((url, etag))) => Some((s, url, etag)),
            _ => None,
        }
    };

    if let Some((dav_settings, resource_url, etag)) = caldav_info {
        if let Err(e) = caldav_client::delete_event(
            &resource_url, &dav_settings.username, &dav_settings.password, &etag,
        )
        .await
        {
            eprintln!("CalDAV DELETE failed (continuing with local delete): {}", e);
        }
    }

    // Always remove from local DB
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM calendar_events WHERE id = ?1", rusqlite::params![event_id]).ok();
        conn.execute(
            "DELETE FROM caldav_resource_state WHERE account_id = ?1 AND event_id = ?2",
            rusqlite::params![account_id, event_id],
        ).ok();
    }

    Ok(())
}

// ── CardDAV CRUD commands ───────────────────────────────

#[tauri::command]
async fn save_contact_entry(
    state: tauri::State<'_, AppState>,
    account_id: String,
    contact_id: String,
    name: String,
    first_name: Option<String>,
    last_name: Option<String>,
    middle_name: Option<String>,
    prefix: Option<String>,
    suffix: Option<String>,
    emails: Vec<ContactEmailEntry>,
    phones: Vec<ContactPhoneEntry>,
    addresses: Vec<ContactAddressEntry>,
    job_title: Option<String>,
    department: Option<String>,
    organization: Option<String>,
    birthday: Option<String>,
    notes: Option<String>,
    is_favorite: bool,
    photo_url: Option<String>,
) -> Result<String, String> {
    let dav_settings = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        sync_state::load_carddav_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No CardDAV settings configured")?
    };

    // Look up existing resource_url + etag + collection_url
    let existing: Option<(String, String, String)> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT rs.resource_url, rs.etag, rs.collection_url FROM carddav_resource_state rs WHERE rs.account_id = ?1 AND rs.contact_id = ?2",
            rusqlite::params![account_id, contact_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok()
    };

    let is_new = existing.is_none();
    let uid = if is_new { uuid::Uuid::new_v4().to_string() } else { contact_id.clone() };

    // For new contacts, pick the first address book
    let book_url = if let Some((_, _, ref coll)) = existing {
        coll.clone()
    } else {
        let normalized = normalize_url(&dav_settings.url);
        let (_home, books) = carddav_client::discover_address_books(
            &normalized, &dav_settings.username, &dav_settings.password,
        )
        .await
        .map_err(|e| format!("CardDAV discovery failed: {}", e))?;
        books.into_iter().next().map(|b| b.url).ok_or("No address books found")?
    };

    let (resource_url, etag) = existing.map(|(r, e, _)| (r, e)).unwrap_or_default();

    let fn_first = first_name.clone().unwrap_or_default();
    let fn_last = last_name.clone().unwrap_or_default();
    let fn_middle = middle_name.clone().unwrap_or_default();
    let fn_prefix = prefix.clone().unwrap_or_default();
    let fn_suffix = suffix.clone().unwrap_or_default();

    // Normalize inputs: trim + ensure exactly one default per list.
    let emails = normalize_contact_emails(emails);
    let phones = normalize_contact_phones(phones);
    let addresses = normalize_contact_addresses(addresses);

    let primary_email = emails.iter().find(|e| e.is_default).or_else(|| emails.first())
        .map(|e| e.email.clone()).unwrap_or_default();
    let primary_phone = phones.iter().find(|p| p.label == "work" || p.label == "home" || p.label == "other")
        .map(|p| p.number.clone());
    let primary_mobile = phones.iter().find(|p| p.label == "cell")
        .map(|p| p.number.clone());
    let address_scalar = addresses.iter().find(|a| a.is_default).or_else(|| addresses.first()).map(|a| {
        [&a.street, &a.city, &a.region, &a.postal_code, &a.country]
            .iter().filter(|s| !s.is_empty()).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
    });

    let dav_emails: Vec<carddav_client::ContactEmail> = emails.iter().map(|e| carddav_client::ContactEmail {
        email: e.email.clone(), label: e.label.clone(), is_default: e.is_default,
    }).collect();
    let dav_phones: Vec<carddav_client::ContactPhone> = phones.iter().map(|p| carddav_client::ContactPhone {
        number: p.number.clone(), label: p.label.clone(), subtypes: p.subtypes.clone(), is_default: p.is_default,
    }).collect();
    let dav_addresses: Vec<carddav_client::ContactAddress> = addresses.iter().map(|a| carddav_client::ContactAddress {
        street: a.street.clone(), city: a.city.clone(), region: a.region.clone(),
        postal_code: a.postal_code.clone(), country: a.country.clone(),
        label: a.label.clone(), is_default: a.is_default,
    }).collect();

    let new_rev = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    let parsed = carddav_client::ParsedContact {
        uid: uid.clone(),
        full_name: name.clone(),
        first_name: fn_first,
        last_name: fn_last,
        middle_name: fn_middle,
        prefix: fn_prefix,
        suffix: fn_suffix,
        emails: dav_emails,
        phones: dav_phones,
        addresses: dav_addresses,
        organization: organization.clone().unwrap_or_default(),
        department: department.clone(),
        title: job_title.clone().unwrap_or_default(),
        birthday: birthday.clone(),
        photo_url: photo_url.clone(),
        rev: Some(new_rev.clone()),
        resource_url,
        etag,
    };

    let new_etag = carddav_client::put_contact(
        &book_url, &dav_settings.username, &dav_settings.password, &parsed,
    )
    .await
    .map_err(|e| format!("CardDAV PUT failed: {}", e))?;

    // Persist to local DB
    let local_id = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        if is_new {
            let local_id = uuid::Uuid::new_v4().to_string();
            let initials = make_initials(&name, &primary_email);
            let color = make_avatar_color(&primary_email);
            conn.execute(
                "INSERT INTO contacts (id, account_id, name, first_name, last_name, middle_name, prefix, suffix, email, initials, color, phone, mobile, job_title, department, organization, address, birthday, notes, is_favorite, photo_url, rev)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
                rusqlite::params![local_id, account_id, name, first_name, last_name, middle_name, prefix, suffix, primary_email, initials, color, primary_phone, primary_mobile, job_title, department, organization, address_scalar, birthday, notes, is_favorite as i32, photo_url, new_rev],
            ).map_err(|e| e.to_string())?;

            write_contact_children_local(&conn, &local_id, &emails, &phones, &addresses);

            let res_url = format!("{}/{}.vcf", book_url.trim_end_matches('/'), uid);
            conn.execute(
                "INSERT OR REPLACE INTO carddav_resource_state (account_id, collection_url, resource_url, etag, contact_id)
                 VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![account_id, book_url, res_url, new_etag, local_id],
            ).map_err(|e| e.to_string())?;
            local_id
        } else {
            let initials = make_initials(&name, &primary_email);
            conn.execute(
                "UPDATE contacts SET name=?2, email=?3, initials=?4, first_name=?5, last_name=?6, middle_name=?7, prefix=?8, suffix=?9, phone=?10, mobile=?11, job_title=?12, department=?13, organization=?14, address=?15, birthday=?16, notes=?17, is_favorite=?18, photo_url=?19, rev=?20 WHERE id=?1",
                rusqlite::params![contact_id, name, primary_email, initials, first_name, last_name, middle_name, prefix, suffix, primary_phone, primary_mobile, job_title, department, organization, address_scalar, birthday, notes, is_favorite as i32, photo_url, new_rev],
            ).map_err(|e| e.to_string())?;

            write_contact_children_local(&conn, &contact_id, &emails, &phones, &addresses);

            conn.execute(
                "UPDATE carddav_resource_state SET etag = ?3 WHERE account_id = ?1 AND contact_id = ?2",
                rusqlite::params![account_id, contact_id, new_etag],
            ).map_err(|e| e.to_string())?;
            contact_id.clone()
        }
    };

    Ok(local_id)
}

fn normalize_contact_emails(mut v: Vec<ContactEmailEntry>) -> Vec<ContactEmailEntry> {
    v.retain_mut(|e| { e.email = e.email.trim().to_string(); !e.email.is_empty() });
    for e in &mut v {
        if e.label.is_empty() { e.label = "work".to_string(); }
    }
    ensure_single_default(&mut v, |e| e.is_default, |e, b| e.is_default = b);
    v
}

fn normalize_contact_phones(mut v: Vec<ContactPhoneEntry>) -> Vec<ContactPhoneEntry> {
    v.retain_mut(|p| { p.number = p.number.trim().to_string(); !p.number.is_empty() });
    for p in &mut v {
        if p.label.is_empty() { p.label = "work".to_string(); }
        p.subtypes.retain(|s| matches!(s.as_str(), "voice" | "text" | "fax" | "video" | "pager"));
    }
    ensure_single_default(&mut v, |p| p.is_default, |p, b| p.is_default = b);
    v
}

fn normalize_contact_addresses(mut v: Vec<ContactAddressEntry>) -> Vec<ContactAddressEntry> {
    v.retain_mut(|a| {
        a.street = a.street.trim().to_string();
        a.city = a.city.trim().to_string();
        a.region = a.region.trim().to_string();
        a.postal_code = a.postal_code.trim().to_string();
        a.country = a.country.trim().to_string();
        !(a.street.is_empty() && a.city.is_empty() && a.region.is_empty() && a.postal_code.is_empty() && a.country.is_empty())
    });
    for a in &mut v {
        if a.label.is_empty() { a.label = "home".to_string(); }
    }
    ensure_single_default(&mut v, |a| a.is_default, |a, b| a.is_default = b);
    v
}

fn ensure_single_default<T, G: Fn(&T) -> bool, S: Fn(&mut T, bool)>(items: &mut [T], get: G, set: S) {
    if items.is_empty() { return; }
    let first_default = items.iter().position(|i| get(i));
    match first_default {
        Some(idx) => {
            for (i, it) in items.iter_mut().enumerate() {
                if i != idx && get(it) { set(it, false); }
            }
        }
        None => set(&mut items[0], true),
    }
}

fn write_contact_children_local(
    conn: &rusqlite::Connection,
    contact_id: &str,
    emails: &[ContactEmailEntry],
    phones: &[ContactPhoneEntry],
    addresses: &[ContactAddressEntry],
) {
    conn.execute("DELETE FROM contact_emails WHERE contact_id = ?1", rusqlite::params![contact_id]).ok();
    conn.execute("DELETE FROM contact_phones WHERE contact_id = ?1", rusqlite::params![contact_id]).ok();
    conn.execute("DELETE FROM contact_addresses WHERE contact_id = ?1", rusqlite::params![contact_id]).ok();
    for (i, e) in emails.iter().enumerate() {
        conn.execute(
            "INSERT INTO contact_emails (id, contact_id, address, label, is_default, position) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![format!("{}-e{}", contact_id, i), contact_id, e.email, e.label, e.is_default as i32, i as i64],
        ).ok();
    }
    for (i, p) in phones.iter().enumerate() {
        conn.execute(
            "INSERT INTO contact_phones (id, contact_id, number, label, subtypes, is_default, position) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![format!("{}-p{}", contact_id, i), contact_id, p.number, p.label, p.subtypes.join(","), p.is_default as i32, i as i64],
        ).ok();
    }
    for (i, a) in addresses.iter().enumerate() {
        conn.execute(
            "INSERT INTO contact_addresses (id, contact_id, street, city, region, postal_code, country, label, is_default, position) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![format!("{}-a{}", contact_id, i), contact_id, a.street, a.city, a.region, a.postal_code, a.country, a.label, a.is_default as i32, i as i64],
        ).ok();
    }
}

/// Version used during sync that takes carddav_client types.
pub(crate) fn write_contact_children(
    conn: &rusqlite::Connection,
    contact_id: &str,
    emails: &[carddav_client::ContactEmail],
    phones: &[carddav_client::ContactPhone],
    addresses: &[carddav_client::ContactAddress],
) {
    conn.execute("DELETE FROM contact_emails WHERE contact_id = ?1", rusqlite::params![contact_id]).ok();
    conn.execute("DELETE FROM contact_phones WHERE contact_id = ?1", rusqlite::params![contact_id]).ok();
    conn.execute("DELETE FROM contact_addresses WHERE contact_id = ?1", rusqlite::params![contact_id]).ok();
    for (i, e) in emails.iter().enumerate() {
        conn.execute(
            "INSERT INTO contact_emails (id, contact_id, address, label, is_default, position) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![format!("{}-e{}", contact_id, i), contact_id, e.email, e.label, e.is_default as i32, i as i64],
        ).ok();
    }
    for (i, p) in phones.iter().enumerate() {
        conn.execute(
            "INSERT INTO contact_phones (id, contact_id, number, label, subtypes, is_default, position) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![format!("{}-p{}", contact_id, i), contact_id, p.number, p.label, p.subtypes.join(","), p.is_default as i32, i as i64],
        ).ok();
    }
    for (i, a) in addresses.iter().enumerate() {
        conn.execute(
            "INSERT INTO contact_addresses (id, contact_id, street, city, region, postal_code, country, label, is_default, position) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![format!("{}-a{}", contact_id, i), contact_id, a.street, a.city, a.region, a.postal_code, a.country, a.label, a.is_default as i32, i as i64],
        ).ok();
    }
}

#[tauri::command]
async fn delete_contact_entry(
    state: tauri::State<'_, AppState>,
    account_id: String,
    contact_id: String,
) -> Result<(), String> {
    let dav_settings = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        sync_state::load_carddav_settings(&conn, &account_id)
            .map_err(|e| e.to_string())?
            .ok_or("No CardDAV settings configured")?
    };

    let (resource_url, etag): (String, String) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT resource_url, etag FROM carddav_resource_state WHERE account_id = ?1 AND contact_id = ?2",
            rusqlite::params![account_id, contact_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|e| format!("Contact not found in CardDAV state: {}", e))?
    };

    carddav_client::delete_contact(
        &resource_url, &dav_settings.username, &dav_settings.password, &etag,
    )
    .await
    .map_err(|e| format!("CardDAV DELETE failed: {}", e))?;

    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM contacts WHERE id = ?1", rusqlite::params![contact_id]).ok();
        conn.execute(
            "DELETE FROM carddav_resource_state WHERE account_id = ?1 AND contact_id = ?2",
            rusqlite::params![account_id, contact_id],
        ).ok();
    }

    Ok(())
}

// ── Toggle Contact Favorite (local-only) ─────────────────

#[tauri::command]
async fn toggle_contact_favorite(
    state: tauri::State<'_, AppState>,
    contact_id: String,
    is_favorite: bool,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE contacts SET is_favorite = ?2 WHERE id = ?1",
        rusqlite::params![contact_id, is_favorite as i32],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Ignored (Muted) Addresses ────────────────────────────

#[tauri::command]
async fn add_ignored_address(
    state: tauri::State<'_, AppState>,
    email: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR IGNORE INTO ignored_addresses (email) VALUES (?1)",
        rusqlite::params![email.to_lowercase()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn remove_ignored_address(
    state: tauri::State<'_, AppState>,
    email: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM ignored_addresses WHERE email = ?1",
        rusqlite::params![email.to_lowercase()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_ignored_addresses(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT email FROM ignored_addresses")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for r in rows {
        result.push(r.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

// ── Sender Blocklist ────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SenderBlockEntry {
    pub email: String,
    pub account_id: String,
    pub list_type: String,
    pub created_at: String,
}

#[tauri::command]
async fn add_to_sender_blocklist(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email: String,
    list_type: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let now = chrono::Local::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO sender_blocklist (email, account_id, list_type, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![email.to_lowercase(), account_id, list_type, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn remove_from_sender_blocklist(
    state: tauri::State<'_, AppState>,
    account_id: String,
    email: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM sender_blocklist WHERE email = ?1 AND account_id = ?2",
        rusqlite::params![email.to_lowercase(), account_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_sender_blocklist(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Vec<SenderBlockEntry>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT email, account_id, list_type, created_at FROM sender_blocklist WHERE account_id = ?1 ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params![account_id], |row| {
        Ok(SenderBlockEntry {
            email: row.get(0)?,
            account_id: row.get(1)?,
            list_type: row.get(2)?,
            created_at: row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for r in rows {
        result.push(r.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

// ── Contact Lists CRUD ──────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactListMember {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactListWithMembers {
    pub id: String,
    pub name: String,
    pub members: Vec<ContactListMember>,
}

#[tauri::command]
fn get_contact_lists(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Vec<ContactListWithMembers>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    let mut list_stmt = conn
        .prepare("SELECT id, name FROM contact_lists WHERE account_id = ?1 ORDER BY name")
        .map_err(|e| e.to_string())?;
    let lists: Vec<(String, String)> = list_stmt
        .query_map(rusqlite::params![account_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut member_stmt = conn
        .prepare("SELECT name, email FROM contact_list_members WHERE list_id = ?1 ORDER BY name, email")
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for (id, name) in lists {
        let members: Vec<ContactListMember> = member_stmt
            .query_map(rusqlite::params![id], |row| {
                Ok(ContactListMember {
                    name: row.get(0)?,
                    email: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        result.push(ContactListWithMembers { id, name, members });
    }

    Ok(result)
}

#[tauri::command]
fn save_contact_list(
    state: tauri::State<'_, AppState>,
    account_id: String,
    id: String,
    name: String,
    members: Vec<ContactListMember>,
) -> Result<String, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO contact_lists (id, account_id, name) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name",
        rusqlite::params![id, account_id, name],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "DELETE FROM contact_list_members WHERE list_id = ?1",
        rusqlite::params![id],
    ).map_err(|e| e.to_string())?;

    for m in &members {
        conn.execute(
            "INSERT OR IGNORE INTO contact_list_members (list_id, name, email) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, m.name, m.email],
        ).map_err(|e| e.to_string())?;
    }

    Ok(id)
}

#[tauri::command]
fn delete_contact_list(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM contact_lists WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Helper utilities ────────────────────────────────────

fn make_initials(name: &str, email: &str) -> String {
    if !name.is_empty() {
        name.split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
    } else if !email.is_empty() {
        email.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
    } else {
        "?".to_string()
    }
}

fn make_avatar_color(email: &str) -> String {
    let colors = ["#0078d4", "#e74856", "#00cc6a", "#f7630c", "#8764b8", "#00b7c3", "#ff8c00", "#e81123"];
    let hash: usize = email.bytes().map(|b| b as usize).sum();
    colors[hash % colors.len()].to_string()
}

// ── Query helpers ───────────────────────────────────────

fn query_folders(conn: &Connection, account_id: &str) -> Vec<Folder> {
    let mut stmt = conn
        .prepare("SELECT id, name, icon, is_favorite FROM folders WHERE account_id = ?1 ORDER BY rowid")
        .unwrap();
    stmt.query_map(rusqlite::params![account_id], |row| {
        Ok(Folder {
            id: row.get(0)?,
            name: row.get(1)?,
            icon: row.get(2)?,
            is_favorite: row.get::<_, i32>(3)? != 0,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn query_emails(conn: &Connection, account_id: &str) -> Vec<Email> {
    // Build a lookup of contact photos by email address (lowercased).
    // Includes both the primary contacts.email and every entry in contact_emails,
    // so a sender matching any of a contact's addresses resolves the photo.
    let contact_photos: std::collections::HashMap<String, String> = {
        let mut stmt = conn
            .prepare(
                "SELECT lower(email), photo_url FROM contacts
                 WHERE account_id = ?1 AND photo_url IS NOT NULL AND photo_url != ''
                 UNION
                 SELECT lower(ce.address), c.photo_url FROM contact_emails ce
                 JOIN contacts c ON ce.contact_id = c.id
                 WHERE c.account_id = ?1 AND c.photo_url IS NOT NULL AND c.photo_url != ''",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    let mut stmt = conn
        .prepare(
            "SELECT id, from_name, from_email, from_initials, from_color, subject, preview, body, date,
                    is_read, is_starred, has_attachment, folder, is_pinned, is_focused,
                    COALESCE(reply_to, ''), COALESCE(message_id, ''), COALESCE(auth_results, ''),
                    is_replied, replied_at
             FROM emails WHERE account_id = ?1 ORDER BY is_pinned DESC, date DESC",
        )
        .unwrap();

    stmt.query_map(rusqlite::params![account_id], |row| {
        let email_id: String = row.get(0)?;
        Ok((email_id, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
            row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?,
            row.get::<_, i32>(9)?, row.get::<_, i32>(10)?, row.get::<_, i32>(11)?,
            row.get(12)?, row.get::<_, i32>(13)?, row.get::<_, i32>(14)?,
            row.get(15)?, row.get(16)?, row.get(17)?, row.get::<_, i32>(18)?,
            row.get::<_, Option<String>>(19)?))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .map(|(id, fn_, fe, fi, fc, subj, prev, body, date, read, star, att, folder, pinned, focused, reply_to, message_id, auth_results, replied, replied_at): (String, String, String, String, String, String, String, String, String, i32, i32, i32, String, i32, i32, String, String, String, i32, Option<String>)| {
        let (to_recipients, cc_recipients) = query_email_recipients(conn, &id);
        let photo_url = contact_photos.get(&fe.to_lowercase()).cloned();
        let attachments = query_email_attachments(conn, &id);
        Email {
            id,
            from: Contact { name: imap_client::decode_mime_header(&fn_), email: fe, initials: fi, color: fc, photo_url },
            to: to_recipients,
            cc: cc_recipients,
            subject: imap_client::decode_mime_header(&subj),
            preview: prev,
            body,
            date,
            is_read: read != 0,
            is_starred: star != 0,
            is_pinned: pinned != 0,
            is_focused: focused != 0,
            is_replied: replied != 0,
            replied_at,
            has_attachment: att != 0,
            attachments,
            folder,
            reply_to,
            message_id,
            auth_results,
        }
    })
    .collect()
}

fn query_email_attachments(conn: &Connection, email_id: &str) -> Vec<AttachmentEntry> {
    let mut stmt = conn
        .prepare("SELECT idx, filename, mime_type, size FROM email_attachments WHERE email_id = ?1 ORDER BY idx")
        .unwrap();
    stmt.query_map(rusqlite::params![email_id], |row| {
        Ok(AttachmentEntry {
            index: row.get(0)?,
            filename: row.get(1)?,
            mime_type: row.get(2)?,
            size: row.get(3)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn query_email_recipients(conn: &Connection, email_id: &str) -> (Vec<Contact>, Vec<Contact>) {
    let mut stmt = conn
        .prepare("SELECT name, email, initials, color, type FROM email_recipients WHERE email_id = ?1")
        .unwrap();
    let rows: Vec<(Contact, String)> = stmt.query_map(rusqlite::params![email_id], |row| {
        Ok((Contact {
            name: row.get(0)?,
            photo_url: None,
            email: row.get(1)?,
            initials: row.get(2)?,
            color: row.get(3)?,
        }, row.get::<_, String>(4)?))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect();

    let to_list = rows.iter().filter(|(_, t)| t == "to").map(|(c, _)| c.clone()).collect();
    let cc_list = rows.iter().filter(|(_, t)| t == "cc").map(|(c, _)| c.clone()).collect();
    (to_list, cc_list)
}

/// Lightweight recipient suggestion for the compose autocomplete.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientSuggestion {
    pub name: String,
    pub email: String,
    pub initials: String,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo_url: Option<String>,
    pub is_favorite: bool,
    /// "contact", "sender", or "list"
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_id: Option<String>,
}

#[tauri::command]
fn search_recipients(
    state: tauri::State<'_, AppState>,
    query: String,
    account_id: Option<String>,
) -> Result<Vec<RecipientSuggestion>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let q = query.trim().to_lowercase();
    let pattern = format!("%{}%", q);

    let mut results: Vec<RecipientSuggestion> = Vec::new();
    let mut seen_emails: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Helper: check if an email address is a noreply-like address
    fn is_noreply(email: &str) -> bool {
        let local = email.split('@').next().unwrap_or("");
        let stripped: String = local.chars().filter(|c| c.is_ascii_alphabetic()).collect();
        let s = stripped.to_lowercase();
        s.starts_with("noreply") || s.ends_with("noreply")
            || s.starts_with("donotreply") || s.ends_with("donotreply")
            || s.starts_with("noanswer") || s.ends_with("noanswer")
    }

    // 1. Favorite contacts
    {
        let mut stmt = conn
            .prepare(
                "SELECT c.name, c.email, c.initials, c.color, c.photo_url
                 FROM contacts c
                 WHERE c.is_favorite = 1
                   AND (LOWER(c.name) LIKE ?1 OR LOWER(c.email) LIKE ?1)
                   AND LOWER(c.email) NOT IN (SELECT email FROM ignored_addresses)
                 ORDER BY c.name
                 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![pattern], |row| {
                Ok(RecipientSuggestion {
                    name: row.get(0)?,
                    email: row.get(1)?,
                    initials: row.get(2)?,
                    color: row.get(3)?,
                    photo_url: row.get(4)?,
                    is_favorite: true,
                    source: "contact".to_string(),
                    list_id: None,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let key = row.email.to_lowercase();
            if seen_emails.insert(key) {
                results.push(row);
            }
        }
    }

    // 2. Rest of contacts (non-favorites)
    {
        let mut stmt = conn
            .prepare(
                "SELECT c.name, c.email, c.initials, c.color, c.photo_url
                 FROM contacts c
                 WHERE c.is_favorite = 0
                   AND (LOWER(c.name) LIKE ?1 OR LOWER(c.email) LIKE ?1)
                   AND LOWER(c.email) NOT IN (SELECT email FROM ignored_addresses)
                 ORDER BY c.name
                 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![pattern], |row| {
                Ok(RecipientSuggestion {
                    name: row.get(0)?,
                    email: row.get(1)?,
                    initials: row.get(2)?,
                    color: row.get(3)?,
                    photo_url: row.get(4)?,
                    is_favorite: false,
                    source: "contact".to_string(),
                    list_id: None,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let key = row.email.to_lowercase();
            if seen_emails.insert(key) {
                results.push(row);
            }
        }
    }

    // 3. Contact lists (scoped to account)
    if let Some(ref acct_id) = account_id {
        let mut stmt = conn
            .prepare(
                "SELECT cl.id, cl.name, COUNT(clm.email) as member_count
                 FROM contact_lists cl
                 LEFT JOIN contact_list_members clm ON clm.list_id = cl.id
                 WHERE LOWER(cl.name) LIKE ?1 AND cl.account_id = ?2
                 GROUP BY cl.id
                 ORDER BY cl.name
                 LIMIT 10",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![pattern, acct_id], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let count: i64 = row.get(2)?;
                let initials = make_initials(&name, "");
                let color = make_avatar_color(&name);
                Ok(RecipientSuggestion {
                    name,
                    email: format!("{} member{}", count, if count == 1 { "" } else { "s" }),
                    initials,
                    color,
                    photo_url: None,
                    is_favorite: false,
                    source: "list".to_string(),
                    list_id: Some(id),
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            results.push(row);
        }
    }

    // 4. Previous recipients from sent emails
    {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT r.name, r.email, r.initials, r.color
                 FROM email_recipients r
                 INNER JOIN emails e ON e.id = r.email_id
                 WHERE e.folder = 'sent'
                   AND (LOWER(r.name) LIKE ?1 OR LOWER(r.email) LIKE ?1)
                   AND LOWER(r.email) NOT IN (SELECT email FROM ignored_addresses)
                 ORDER BY r.name
                 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![pattern], |row| {
                Ok(RecipientSuggestion {
                    name: row.get(0)?,
                    email: row.get(1)?,
                    initials: row.get(2)?,
                    color: row.get(3)?,
                    photo_url: None,
                    is_favorite: false,
                    source: "sender".to_string(),
                    list_id: None,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let key = row.email.to_lowercase();
            if seen_emails.insert(key) {
                results.push(row);
            }
        }
    }

    // 5. Replyable senders (exclude noreply-like addresses)
    {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT e.from_name, e.from_email, e.from_initials, e.from_color
                 FROM emails e
                 WHERE (LOWER(e.from_name) LIKE ?1 OR LOWER(e.from_email) LIKE ?1)
                 ORDER BY e.from_name
                 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![pattern], |row| {
                Ok(RecipientSuggestion {
                    name: row.get(0)?,
                    email: row.get(1)?,
                    initials: row.get(2)?,
                    color: row.get(3)?,
                    photo_url: None,
                    is_favorite: false,
                    source: "sender".to_string(),
                    list_id: None,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            if is_noreply(&row.email) {
                continue;
            }
            let key = row.email.to_lowercase();
            if seen_emails.insert(key) {
                results.push(row);
            }
        }
    }

    Ok(results)
}

pub fn query_contacts(conn: &Connection, account_id: &str) -> Vec<FullContact> {
    let mut stmt = match conn.prepare(
        "SELECT c.id, c.name, c.email, c.initials, c.color, c.phone, c.mobile,
                c.job_title, c.department, c.organization, c.address, c.birthday, c.notes,
                c.is_favorite, c.photo_url,
                COALESCE(cs.read_only, 0),
                c.first_name, c.last_name, c.middle_name, c.prefix, c.suffix, c.rev
         FROM contacts c
         LEFT JOIN carddav_resource_state rs ON rs.contact_id = c.id AND rs.account_id = c.account_id
         LEFT JOIN carddav_collection_state cs ON cs.collection_url = rs.collection_url AND cs.account_id = c.account_id
         WHERE c.account_id = ?1
         ORDER BY c.name",
    ) { Ok(s) => s, Err(_) => return Vec::new() };

    struct Row {
        id: String,
        name: String,
        legacy_email: String,
        initials: String,
        color: String,
        legacy_phone: Option<String>,
        legacy_mobile: Option<String>,
        legacy_address: Option<String>,
        job_title: Option<String>,
        department: Option<String>,
        organization: Option<String>,
        birthday: Option<String>,
        notes: Option<String>,
        is_favorite: bool,
        photo_url: Option<String>,
        is_read_only: bool,
        first_name: Option<String>,
        last_name: Option<String>,
        middle_name: Option<String>,
        prefix: Option<String>,
        suffix: Option<String>,
        rev: Option<String>,
    }

    let rows: Vec<Row> = stmt.query_map(rusqlite::params![account_id], |row| {
        Ok(Row {
            id: row.get(0)?,
            name: row.get(1)?,
            legacy_email: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            initials: row.get(3)?,
            color: row.get(4)?,
            legacy_phone: row.get(5)?,
            legacy_mobile: row.get(6)?,
            job_title: row.get(7)?,
            department: row.get(8)?,
            organization: row.get(9)?,
            legacy_address: row.get(10)?,
            birthday: row.get(11)?,
            notes: row.get(12)?,
            is_favorite: row.get::<_, i32>(13)? != 0,
            photo_url: row.get(14)?,
            is_read_only: row.get::<_, i32>(15).unwrap_or(0) != 0,
            first_name: row.get::<_, Option<String>>(16).ok().flatten().filter(|s| !s.is_empty()),
            last_name: row.get::<_, Option<String>>(17).ok().flatten().filter(|s| !s.is_empty()),
            middle_name: row.get::<_, Option<String>>(18).ok().flatten().filter(|s| !s.is_empty()),
            prefix: row.get::<_, Option<String>>(19).ok().flatten().filter(|s| !s.is_empty()),
            suffix: row.get::<_, Option<String>>(20).ok().flatten().filter(|s| !s.is_empty()),
            rev: row.get::<_, Option<String>>(21).ok().flatten(),
        })
    }).ok()
    .into_iter()
    .flat_map(|it| it.flatten())
    .collect();

    let mut out: Vec<FullContact> = Vec::with_capacity(rows.len());
    for r in rows {
        let emails = query_contact_emails(conn, &r.id, &r.legacy_email);
        let phones = query_contact_phones(conn, &r.id, &r.legacy_phone, &r.legacy_mobile);
        let addresses = query_contact_addresses(conn, &r.id, &r.legacy_address);
        let primary_email = emails.iter().find(|e| e.is_default).or_else(|| emails.first())
            .map(|e| e.email.clone()).unwrap_or_default();

        out.push(FullContact {
            id: r.id,
            name: r.name,
            email: primary_email,
            first_name: r.first_name,
            last_name: r.last_name,
            middle_name: r.middle_name,
            prefix: r.prefix,
            suffix: r.suffix,
            emails,
            phones,
            addresses,
            initials: r.initials,
            color: r.color,
            organization: r.organization,
            job_title: r.job_title,
            department: r.department,
            birthday: r.birthday,
            notes: r.notes,
            is_favorite: r.is_favorite,
            photo_url: r.photo_url,
            rev: r.rev,
            is_read_only: r.is_read_only,
        });
    }
    out
}

fn query_contact_emails(conn: &Connection, contact_id: &str, legacy: &str) -> Vec<ContactEmailEntry> {
    let rows: Vec<ContactEmailEntry> = conn
        .prepare("SELECT address, label, is_default FROM contact_emails WHERE contact_id = ?1 ORDER BY position, rowid")
        .ok()
        .and_then(|mut s| {
            s.query_map(rusqlite::params![contact_id], |row| Ok(ContactEmailEntry {
                email: row.get(0)?,
                label: row.get::<_, Option<String>>(1)?.unwrap_or_else(|| "work".into()),
                is_default: row.get::<_, i32>(2)? != 0,
            })).ok().map(|it| it.flatten().collect())
        })
        .unwrap_or_default();
    if !rows.is_empty() {
        return rows;
    }
    if !legacy.is_empty() {
        vec![ContactEmailEntry { email: legacy.to_string(), label: "work".into(), is_default: true }]
    } else {
        Vec::new()
    }
}

fn query_contact_phones(conn: &Connection, contact_id: &str, legacy_phone: &Option<String>, legacy_mobile: &Option<String>) -> Vec<ContactPhoneEntry> {
    let rows: Vec<ContactPhoneEntry> = conn
        .prepare("SELECT number, label, subtypes, is_default FROM contact_phones WHERE contact_id = ?1 ORDER BY position, rowid")
        .ok()
        .and_then(|mut s| {
            s.query_map(rusqlite::params![contact_id], |row| Ok(ContactPhoneEntry {
                number: row.get(0)?,
                label: row.get::<_, Option<String>>(1)?.unwrap_or_else(|| "work".into()),
                subtypes: row.get::<_, Option<String>>(2)?.unwrap_or_default()
                    .split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                is_default: row.get::<_, i32>(3)? != 0,
            })).ok().map(|it| it.flatten().collect())
        })
        .unwrap_or_default();
    if !rows.is_empty() {
        return rows;
    }
    let mut out = Vec::new();
    if let Some(p) = legacy_phone.as_ref().filter(|s| !s.is_empty()) {
        out.push(ContactPhoneEntry { number: p.clone(), label: "work".into(), subtypes: vec!["voice".into()], is_default: true });
    }
    if let Some(m) = legacy_mobile.as_ref().filter(|s| !s.is_empty()) {
        let is_default = out.is_empty();
        out.push(ContactPhoneEntry { number: m.clone(), label: "cell".into(), subtypes: vec!["voice".into()], is_default });
    }
    out
}

fn query_contact_addresses(conn: &Connection, contact_id: &str, legacy: &Option<String>) -> Vec<ContactAddressEntry> {
    let rows: Vec<ContactAddressEntry> = conn
        .prepare("SELECT street, city, region, postal_code, country, label, is_default FROM contact_addresses WHERE contact_id = ?1 ORDER BY position, rowid")
        .ok()
        .and_then(|mut s| {
            s.query_map(rusqlite::params![contact_id], |row| Ok(ContactAddressEntry {
                street: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                city: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                region: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                postal_code: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                country: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                label: row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "home".into()),
                is_default: row.get::<_, i32>(6)? != 0,
            })).ok().map(|it| it.flatten().collect())
        })
        .unwrap_or_default();
    if !rows.is_empty() {
        return rows;
    }
    if let Some(a) = legacy.as_ref().filter(|s| !s.is_empty()) {
        vec![ContactAddressEntry { street: a.clone(), label: "home".into(), is_default: true, ..Default::default() }]
    } else {
        Vec::new()
    }
}

fn query_cal_categories(conn: &Connection, account_id: &str) -> Vec<CalendarCategory> {
    let mut stmt = conn
        .prepare(
            "SELECT cs.collection_url, cs.display_name, cs.color, cs.visible, cs.calendar_group
             FROM caldav_collection_state cs
             WHERE cs.account_id = ?1
             ORDER BY
               CASE cs.calendar_group WHEN 'my' THEN 0 ELSE 1 END,
               cs.rowid"
        )
        .unwrap();
    stmt.query_map(rusqlite::params![account_id], |row| {
        Ok(CalendarCategory {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            visible: row.get::<_, i32>(3)? != 0,
            group: row.get(4)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn query_events(conn: &Connection, account_id: &str) -> Vec<CalendarEvent> {
    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.title, e.start, e.end,
                    COALESCE(cs.color, e.color) AS color,
                    e.location, e.description,
                    e.is_all_day, e.calendar_id, e.is_online_meeting,
                    COALESCE(cs.display_name, e.calendar_id) AS calendar_name,
                    e.rrule, e.exdates, e.alert_minutes, e.meeting_url
             FROM calendar_events e
             LEFT JOIN caldav_collection_state cs
               ON cs.account_id = e.account_id AND cs.collection_url = e.calendar_id
             WHERE e.account_id = ?1 ORDER BY e.start",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![account_id], |row| {
        let event_id: String = row.get(0)?;
        Ok((event_id, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
            row.get(5)?, row.get(6)?, row.get::<_, i32>(7)?, row.get(8)?,
            row.get::<_, i32>(9)?, row.get::<_, String>(10)?, row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?, row.get::<_, Option<i32>>(13)?, row.get::<_, Option<String>>(14)?))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .map(|(id, title, start, end, color, location, description, all_day, cal_id, online, cal_name, rrule, exdates_raw, alert_mins, meeting_url):
         (String, String, String, String, String, Option<String>, Option<String>, i32, String, i32, String, Option<String>, Option<String>, Option<i32>, Option<String>)| {
        let attendees_list = query_event_attendees(conn, &id);
        let attendees = if attendees_list.is_empty() { None } else { Some(attendees_list) };
        let is_online = if online != 0 { Some(true) } else { None };
        let mut recurrence = rrule.as_deref().and_then(parse_rrule_to_recurrence);
        if let Some(ref mut rec) = recurrence {
            if let Some(ref raw) = exdates_raw {
                let dates: Vec<String> = raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                if !dates.is_empty() { rec.exdates = Some(dates); }
            }
        }
        CalendarEvent {
            id,
            title,
            start,
            end,
            color,
            location,
            description,
            is_all_day: all_day != 0,
            calendar_id: cal_id,
            calendar_name: cal_name,
            attendees,
            recurrence,
            is_online_meeting: is_online,
            meeting_url,
            alert_minutes: alert_mins,
        }
    })
    .collect()
}

fn query_event_attendees(conn: &Connection, event_id: &str) -> Vec<EventAttendee> {
    let mut stmt = conn
        .prepare("SELECT name, email, initials, color, role FROM event_attendees WHERE event_id = ?1")
        .unwrap();
    stmt.query_map(rusqlite::params![event_id], |row| {
        Ok(EventAttendee {
            name: row.get(0)?,
            email: row.get(1)?,
            initials: row.get(2)?,
            color: row.get(3)?,
            role: row.get::<_, String>(4).unwrap_or_else(|_| "required".to_string()),
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Parse a raw RRULE string like "FREQ=WEEKLY;INTERVAL=2;UNTIL=20261231T235959Z"
/// into a RecurrenceParam.
fn parse_rrule_to_recurrence(rrule: &str) -> Option<RecurrenceParam> {
    let mut freq: Option<String> = None;
    let mut interval = 1u32;
    let mut end_date: Option<String> = None;
    let mut by_day: Option<String> = None;
    for part in rrule.split(';') {
        if let Some((k, v)) = part.split_once('=') {
            match k.to_uppercase().as_str() {
                "FREQ" => {
                    freq = Some(match v.to_uppercase().as_str() {
                        "DAILY" => "daily",
                        "WEEKLY" => "weekly",
                        "MONTHLY" => "monthly",
                        "YEARLY" => "yearly",
                        _ => return None,
                    }.to_string());
                }
                "INTERVAL" => {
                    interval = v.parse().unwrap_or(1);
                }
                "UNTIL" => {
                    // "20261231T235959Z" → "2026-12-31"
                    if v.len() >= 8 {
                        end_date = Some(format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8]));
                    }
                }
                "BYDAY" => {
                    by_day = Some(v.to_string());
                }
                _ => {}
            }
        }
    }
    freq.map(|f| RecurrenceParam { freq: f, interval, end_date, by_day, exdates: None })
}

/// Build a raw RRULE string from a RecurrenceParam.
fn recurrence_to_rrule(r: &RecurrenceParam) -> String {
    let freq = match r.freq.as_str() {
        "daily" => "DAILY",
        "weekly" => "WEEKLY",
        "monthly" => "MONTHLY",
        "yearly" => "YEARLY",
        _ => "WEEKLY",
    };
    let mut rrule = format!("FREQ={};INTERVAL={}", freq, r.interval);
    if let Some(ref bd) = r.by_day {
        rrule.push_str(&format!(";BYDAY={}", bd));
    }
    if let Some(ref end) = r.end_date {
        // "2026-12-31" → "20261231T235959Z"
        let compact = end.replace('-', "");
        rrule.push_str(&format!(";UNTIL={}T235959Z", compact));
    }
    rrule
}

fn resolve_storage_dir<R: tauri::Runtime>(app: &tauri::App<R>) -> PathBuf {
    let home_dir = app.path().home_dir().expect("failed to get user home directory");
    home_dir.join(".cortexist").join("mail")
}

fn migrate_legacy_database<R: tauri::Runtime>(app: &tauri::App<R>, storage_dir: &PathBuf) {
    let legacy_dir = app.path().app_data_dir().expect("failed to get legacy app data dir");
    if legacy_dir == *storage_dir {
        return;
    }

    let db_name = "mail.db";
    let new_db_path = storage_dir.join(db_name);
    let legacy_db_path = legacy_dir.join(db_name);

    if new_db_path.exists() || !legacy_db_path.exists() {
        return;
    }

    std::fs::create_dir_all(storage_dir).expect("failed to create storage dir for migration");

    for suffix in ["", "-wal", "-shm"] {
        let legacy_path = legacy_dir.join(format!("{db_name}{suffix}"));
        let new_path = storage_dir.join(format!("{db_name}{suffix}"));
        if legacy_path.exists() {
            std::fs::copy(&legacy_path, &new_path).unwrap_or_else(|_| {
                panic!("failed to migrate database file {}", legacy_path.display())
            });
        }
    }
}

// ── DAV server commands ─────────────────────────────────

#[tauri::command]
async fn start_dav_server(
    state: tauri::State<'_, AppState>,
    bind_addr: String,
) -> Result<String, String> {
    let addr: std::net::SocketAddr = bind_addr.parse().map_err(|e| format!("Invalid address: {}", e))?;
    let db = state.db.clone();

    // Stop existing server if running
    {
        let mut handle = state.dav_handle.lock().map_err(|e| e.to_string())?;
        if let Some(h) = handle.take() {
            h.stop();
        }
    }

    let server_handle = dav_server::start_server(db, addr).await?;
    let actual_addr = server_handle.addr().to_string();

    {
        let mut handle = state.dav_handle.lock().map_err(|e| e.to_string())?;
        *handle = Some(server_handle);
    }

    Ok(actual_addr)
}

#[tauri::command]
fn stop_dav_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut handle = state.dav_handle.lock().map_err(|e| e.to_string())?;
    if let Some(h) = handle.take() {
        h.stop();
    }
    Ok(())
}

#[tauri::command]
fn get_dav_server_status(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let handle = state.dav_handle.lock().map_err(|e| e.to_string())?;
    Ok(handle.as_ref().map(|h| h.addr().to_string()))
}

#[tauri::command]
fn add_dav_user(
    state: tauri::State<'_, AppState>,
    email: String,
    password: String,
    account_id: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS dav_server_users (
            email      TEXT PRIMARY KEY,
            password   TEXT NOT NULL,
            account_id TEXT NOT NULL,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );"
    ).map_err(|e| e.to_string())?;

    let enc_pw = crypto::encrypt_password(&password).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO dav_server_users (email, password, account_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![email.to_lowercase(), enc_pw, account_id],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn remove_dav_user(
    state: tauri::State<'_, AppState>,
    email: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    conn.execute(
        "DELETE FROM dav_server_users WHERE email = ?1",
        rusqlite::params![email.to_lowercase()],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn list_dav_users(state: tauri::State<'_, AppState>) -> Result<Vec<(String, String)>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS dav_server_users (
            email      TEXT PRIMARY KEY,
            password   TEXT NOT NULL,
            account_id TEXT NOT NULL,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );"
    ).map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT email, account_id FROM dav_server_users ORDER BY email")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

// ── Entry point ─────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the existing window when a second instance is launched
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_log::Builder::new().level(LevelFilter::Info).build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let storage_dir = resolve_storage_dir(app);
            migrate_legacy_database(app, &storage_dir);
            std::fs::create_dir_all(&storage_dir).expect("failed to create storage dir");
            let db_path = storage_dir.join("mail.db");

            let conn = Connection::open(&db_path).expect("failed to open database");
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").ok();

            db::init_db(&conn).expect("failed to initialize database schema");
            sync_state::init_sync_tables(&conn).expect("failed to initialize sync tables");
            db::run_migrations(&conn).expect("failed to run database migrations");

            // Only seed mock data in debug builds (tauri dev).
            // Release builds (tauri build) start with an empty database.
            #[cfg(debug_assertions)]
            if !db::is_seeded(&conn).unwrap_or(false) {
                db::seed(&conn).expect("failed to seed database");
            }

            let db = Arc::new(Mutex::new(conn));
            app.manage(AppState { db, dav_handle: Mutex::new(None) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_all_accounts,
            get_setting,
            set_setting,
            update_account,
            add_account,
            delete_account,
            set_account_positions,
            update_email_read,
            update_email_starred,
            update_email_replied,
            update_email_pinned,
            update_email_focused,
            delete_email,
            empty_folder,
            move_email,
            save_mail_settings,
            load_mail_settings,
            save_caldav_settings,
            load_caldav_settings,
            save_carddav_settings,
            load_carddav_settings,
            sync_mail,
            fetch_email_body,
            fetch_previews_around,
            open_attachment,
            save_attachment,
            send_email,
            flush_outbox,
            discover_mail_settings,
            test_imap_connection,
            test_smtp_connection,
            test_caldav_connection,
            test_carddav_connection,
            sync_calendars,
            sync_contacts,
            save_calendar_event,
            delete_calendar_event,
            save_contact_entry,
            delete_contact_entry,
            start_dav_server,
            stop_dav_server,
            get_dav_server_status,
            add_dav_user,
            remove_dav_user,
            list_dav_users,
            search_recipients,
            get_contact_lists,
            save_contact_list,
            delete_contact_list,
            toggle_contact_favorite,
            add_ignored_address,
            remove_ignored_address,
            get_ignored_addresses,
            add_to_sender_blocklist,
            remove_from_sender_blocklist,
            get_sender_blocklist,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
