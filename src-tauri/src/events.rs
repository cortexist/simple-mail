//! events.rs — Backend → frontend push channel for mail/calendar/contacts
//! state changes. Each event type has a dedicated channel name; the
//! frontend subscribes only to what it needs.
//!
//! Mail covers flag changes, folder changes, removals, sync, send, body and
//! preview fetches. Calendar covers event upsert/remove/sync. Contacts cover
//! contact upsert/remove/favorite, sync, and contact-list CRUD.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{CalendarEvent, FullContact};

// ── Channel names ───────────────────────────────────────
// Keep these as constants so the frontend `listen(...)` calls and the
// backend `emit(...)` calls cannot drift.

pub const MAIL_EMAIL_FLAG_CHANGED: &str = "mail:email-flag-changed";
pub const MAIL_EMAIL_FOLDER_CHANGED: &str = "mail:email-folder-changed";
pub const MAIL_EMAIL_REMOVED: &str = "mail:email-removed";
pub const MAIL_SYNC_COMPLETED: &str = "mail:sync-completed";
pub const MAIL_SEND_FAILED: &str = "mail:send-failed";
pub const MAIL_EMAIL_BODY_FETCHED: &str = "mail:email-body-fetched";
pub const MAIL_EMAIL_PREVIEWS_FETCHED: &str = "mail:email-previews-fetched";

pub const CALENDAR_EVENT_UPSERTED: &str = "calendar:event-upserted";
pub const CALENDAR_EVENT_REMOVED: &str = "calendar:event-removed";
pub const CALENDAR_SYNC_COMPLETED: &str = "calendar:sync-completed";

pub const CONTACTS_CONTACT_UPSERTED: &str = "contacts:contact-upserted";
pub const CONTACTS_CONTACT_REMOVED: &str = "contacts:contact-removed";
pub const CONTACTS_FAVORITE_CHANGED: &str = "contacts:favorite-changed";
pub const CONTACTS_SYNC_COMPLETED: &str = "contacts:sync-completed";
pub const CONTACTS_LIST_UPSERTED: &str = "contacts:list-upserted";
pub const CONTACTS_LIST_REMOVED: &str = "contacts:list-removed";

// ── Payloads ────────────────────────────────────────────

/// A flag-style field on a single email changed on the backend. `seq`
/// is a monotonic counter so the UI can drop echoes that arrive out of
/// order (e.g. when the user clicks star → unstar quickly and the two
/// IMAP round trips complete in reverse).
///
/// `field` is one of: `"read"`, `"starred"`, `"replied"`, `"pinned"`,
/// `"focused"`. On rollback after a network failure, `value` carries
/// the restored pre-click value rather than the requested one.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmailFlagChangedEvent {
    pub account_id: String,
    pub email_id: String,
    pub field: &'static str,
    pub value: bool,
    pub seq: u64,
}

pub fn emit_email_flag_changed(
    app: &AppHandle,
    account_id: &str,
    email_id: &str,
    field: &'static str,
    value: bool,
    seq: u64,
) {
    let payload = EmailFlagChangedEvent {
        account_id: account_id.to_string(),
        email_id: email_id.to_string(),
        field,
        value,
        seq,
    };
    if let Err(e) = app.emit(MAIL_EMAIL_FLAG_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_EMAIL_FLAG_CHANGED, e);
    }
}

/// The folder of an email changed (move, archive, junk, soft-delete).
/// On IMAP failure the backend rolls back the local DB and emits the
/// restored folder, so the UI's "folder" view stays in sync.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmailFolderChangedEvent {
    pub account_id: String,
    pub email_id: String,
    pub folder: String,
    pub seq: u64,
}

pub fn emit_email_folder_changed(
    app: &AppHandle,
    account_id: &str,
    email_id: &str,
    folder: &str,
    seq: u64,
) {
    let payload = EmailFolderChangedEvent {
        account_id: account_id.to_string(),
        email_id: email_id.to_string(),
        folder: folder.to_string(),
        seq,
    };
    if let Err(e) = app.emit(MAIL_EMAIL_FOLDER_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_EMAIL_FOLDER_CHANGED, e);
    }
}

/// One or more emails were permanently removed from the local DB (and
/// IMAP, on success). `empty_folder` and a permanent `delete_email`
/// both emit this. `email_ids` is empty only if the operation found
/// nothing to delete.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmailRemovedEvent {
    pub account_id: String,
    pub email_ids: Vec<String>,
    pub seq: u64,
}

pub fn emit_email_removed(
    app: &AppHandle,
    account_id: &str,
    email_ids: Vec<String>,
    seq: u64,
) {
    let payload = EmailRemovedEvent { account_id: account_id.to_string(), email_ids, seq };
    if let Err(e) = app.emit(MAIL_EMAIL_REMOVED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_EMAIL_REMOVED, e);
    }
}

/// Result of a `sync_mail` IMAP sync. Frontend uses this to refresh local
/// state and surface per-account error toasts.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MailSyncCompletedEvent {
    pub account_id: String,
    pub ok: bool,
    pub new_count: usize,
    pub deleted_count: usize,
    pub flag_updates: usize,
    pub errors: Vec<String>,
    pub seq: u64,
}

pub fn emit_mail_sync_completed(app: &AppHandle, payload: MailSyncCompletedEvent) {
    if let Err(e) = app.emit(MAIL_SYNC_COMPLETED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_SYNC_COMPLETED, e);
    }
}

/// A `send_email` operation failed. Used by the frontend to surface
/// error feedback when the corresponding `mail:sent` echo will never come.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MailSendFailedEvent {
    pub account_id: String,
    pub error: String,
    pub seq: u64,
}

pub fn emit_mail_send_failed(
    app: &AppHandle,
    account_id: &str,
    error: &str,
    seq: u64,
) {
    let payload = MailSendFailedEvent { account_id: account_id.to_string(), error: error.to_string(), seq };
    if let Err(e) = app.emit(MAIL_SEND_FAILED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_SEND_FAILED, e);
    }
}

/// `fetch_email_body` completed. The body is the rendered HTML, preview is the
/// plain-text snippet, and attachments lists the per-part metadata. Frontend
/// uses this to patch the email row in place.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmailBodyFetchedEvent {
    pub account_id: String,
    pub email_id: String,
    pub body: String,
    pub preview: String,
    pub auth_results: String,
    pub has_attachment: bool,
    pub attachments: Vec<serde_json::Value>,
    pub seq: u64,
}

pub fn emit_email_body_fetched(
    app: &AppHandle,
    payload: EmailBodyFetchedEvent,
) {
    if let Err(e) = app.emit(MAIL_EMAIL_BODY_FETCHED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_EMAIL_BODY_FETCHED, e);
    }
}

/// `fetch_previews_around` completed. Carries the batch of preview updates
/// that the IMAP round trip produced.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmailPreviewsFetchedEvent {
    pub account_id: String,
    pub updates: Vec<serde_json::Value>,
    pub seq: u64,
}

pub fn emit_email_previews_fetched(
    app: &AppHandle,
    payload: EmailPreviewsFetchedEvent,
) {
    if let Err(e) = app.emit(MAIL_EMAIL_PREVIEWS_FETCHED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_EMAIL_PREVIEWS_FETCHED, e);
    }
}

// ── Calendar ────────────────────────────────────────────

/// An event was created or updated. `requested_id` is the id the caller
/// passed in (a frontend temp id for new events); `event.id` is the canonical
/// id assigned by the backend — frontend reconciles the two if they differ.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventUpsertedEvent {
    pub account_id: String,
    pub requested_id: String,
    pub event: CalendarEvent,
    pub seq: u64,
}

pub fn emit_calendar_event_upserted(app: &AppHandle, payload: CalendarEventUpsertedEvent) {
    if let Err(e) = app.emit(CALENDAR_EVENT_UPSERTED, payload) {
        log::error!("Failed to emit {}: {}", CALENDAR_EVENT_UPSERTED, e);
    }
}

/// A calendar event was removed (CalDAV delete + local row drop).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventRemovedEvent {
    pub account_id: String,
    pub event_id: String,
    pub seq: u64,
}

pub fn emit_calendar_event_removed(app: &AppHandle, account_id: &str, event_id: &str, seq: u64) {
    let payload = CalendarEventRemovedEvent {
        account_id: account_id.to_string(),
        event_id: event_id.to_string(),
        seq,
    };
    if let Err(e) = app.emit(CALENDAR_EVENT_REMOVED, payload) {
        log::error!("Failed to emit {}: {}", CALENDAR_EVENT_REMOVED, e);
    }
}

/// Result of a `sync_calendars` run. Mirrors `MailSyncCompletedEvent`.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSyncCompletedEvent {
    pub account_id: String,
    pub ok: bool,
    pub new_count: usize,
    pub updated_count: usize,
    pub deleted_count: usize,
    pub error: Option<String>,
    pub seq: u64,
}

pub fn emit_calendar_sync_completed(app: &AppHandle, payload: CalendarSyncCompletedEvent) {
    if let Err(e) = app.emit(CALENDAR_SYNC_COMPLETED, payload) {
        log::error!("Failed to emit {}: {}", CALENDAR_SYNC_COMPLETED, e);
    }
}

// ── Contacts ────────────────────────────────────────────

/// A contact was created or updated. `requested_id` allows the frontend to
/// reconcile its temp-id optimistic entry with the canonical id.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactUpsertedEvent {
    pub account_id: String,
    pub requested_id: String,
    pub contact: FullContact,
    pub seq: u64,
}

pub fn emit_contact_upserted(app: &AppHandle, payload: ContactUpsertedEvent) {
    if let Err(e) = app.emit(CONTACTS_CONTACT_UPSERTED, payload) {
        log::error!("Failed to emit {}: {}", CONTACTS_CONTACT_UPSERTED, e);
    }
}

/// A contact was removed (CardDAV delete + local row drop).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactRemovedEvent {
    pub account_id: String,
    pub contact_id: String,
    pub seq: u64,
}

pub fn emit_contact_removed(app: &AppHandle, account_id: &str, contact_id: &str, seq: u64) {
    let payload = ContactRemovedEvent {
        account_id: account_id.to_string(),
        contact_id: contact_id.to_string(),
        seq,
    };
    if let Err(e) = app.emit(CONTACTS_CONTACT_REMOVED, payload) {
        log::error!("Failed to emit {}: {}", CONTACTS_CONTACT_REMOVED, e);
    }
}

/// The favorite flag toggled on a contact (local-only DB state).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactFavoriteChangedEvent {
    pub contact_id: String,
    pub is_favorite: bool,
    pub seq: u64,
}

pub fn emit_contact_favorite_changed(app: &AppHandle, contact_id: &str, is_favorite: bool, seq: u64) {
    let payload = ContactFavoriteChangedEvent {
        contact_id: contact_id.to_string(),
        is_favorite,
        seq,
    };
    if let Err(e) = app.emit(CONTACTS_FAVORITE_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", CONTACTS_FAVORITE_CHANGED, e);
    }
}

/// Result of a `sync_contacts` run.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactsSyncCompletedEvent {
    pub account_id: String,
    pub ok: bool,
    pub new_count: usize,
    pub updated_count: usize,
    pub deleted_count: usize,
    pub error: Option<String>,
    pub seq: u64,
}

pub fn emit_contacts_sync_completed(app: &AppHandle, payload: ContactsSyncCompletedEvent) {
    if let Err(e) = app.emit(CONTACTS_SYNC_COMPLETED, payload) {
        log::error!("Failed to emit {}: {}", CONTACTS_SYNC_COMPLETED, e);
    }
}

/// A contact list was created or updated.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactListUpsertedEvent {
    pub account_id: String,
    pub requested_id: String,
    pub list: serde_json::Value, // ContactList shape — keep JSON to avoid extra coupling
    pub seq: u64,
}

pub fn emit_contact_list_upserted(app: &AppHandle, payload: ContactListUpsertedEvent) {
    if let Err(e) = app.emit(CONTACTS_LIST_UPSERTED, payload) {
        log::error!("Failed to emit {}: {}", CONTACTS_LIST_UPSERTED, e);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactListRemovedEvent {
    pub list_id: String,
    pub seq: u64,
}

pub fn emit_contact_list_removed(app: &AppHandle, list_id: &str, seq: u64) {
    let payload = ContactListRemovedEvent { list_id: list_id.to_string(), seq };
    if let Err(e) = app.emit(CONTACTS_LIST_REMOVED, payload) {
        log::error!("Failed to emit {}: {}", CONTACTS_LIST_REMOVED, e);
    }
}
