//! events.rs — Backend → frontend push channel for mail/calendar/contacts
//! state changes. Each event type has a dedicated channel name; the
//! frontend subscribes only to what it needs.
//!
//! Mail covers flag changes, folder changes, removals, sync, send, body and
//! preview fetches. Calendar covers event upsert/remove/sync. Contacts cover
//! contact upsert/remove/favorite, sync, and contact-list CRUD.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{Account, CalendarEvent, FullContact};
use crate::sync_state::{DavSettings, MailServerSettings};

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
pub const MAIL_DRAFT_GENERATED: &str = "mail:draft-generated";

pub const CALENDAR_EVENT_UPSERTED: &str = "calendar:event-upserted";
pub const CALENDAR_EVENT_REMOVED: &str = "calendar:event-removed";
pub const CALENDAR_SYNC_COMPLETED: &str = "calendar:sync-completed";

pub const CONTACTS_CONTACT_UPSERTED: &str = "contacts:contact-upserted";
pub const CONTACTS_CONTACT_REMOVED: &str = "contacts:contact-removed";
pub const CONTACTS_CONTACT_SAVE_FAILED: &str = "contacts:contact-save-failed";
pub const CONTACTS_FAVORITE_CHANGED: &str = "contacts:favorite-changed";
pub const CONTACTS_SYNC_COMPLETED: &str = "contacts:sync-completed";
pub const CONTACTS_LIST_UPSERTED: &str = "contacts:list-upserted";
pub const CONTACTS_LIST_REMOVED: &str = "contacts:list-removed";

pub const SETTINGS_ACCOUNT_UPSERTED: &str = "settings:account-upserted";
pub const SETTINGS_ACCOUNT_REMOVED: &str = "settings:account-removed";
pub const SETTINGS_ACCOUNTS_REORDERED: &str = "settings:accounts-reordered";
pub const SETTINGS_KV_CHANGED: &str = "settings:kv-changed";
pub const SETTINGS_STORAGE_QUOTA_CHANGED: &str = "settings:storage-quota-changed";
pub const SETTINGS_OFFLINE_DOWNLOAD_CHANGED: &str = "settings:offline-download-changed";
pub const SETTINGS_MAIL_SERVER_SAVED: &str = "settings:mail-server-saved";
pub const SETTINGS_CALDAV_SAVED: &str = "settings:caldav-saved";
pub const SETTINGS_CARDDAV_SAVED: &str = "settings:carddav-saved";
pub const SETTINGS_DAV_SERVER_CHANGED: &str = "settings:dav-server-changed";
pub const SETTINGS_DAV_USERS_CHANGED: &str = "settings:dav-users-changed";
pub const SETTINGS_AI_CONFIG_CHANGED: &str = "settings:ai-config-changed";
pub const SETTINGS_AI_FEATURES_CHANGED: &str = "settings:ai-features-changed";

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
    /// true: the message is still queued and will be retried -- inform, do not
    /// alarm. false: retries are exhausted and the message went back to Drafts.
    pub transient: bool,
    pub subject: String,
    pub seq: u64,
}

pub fn emit_mail_send_failed(
    app: &AppHandle,
    account_id: &str,
    error: &str,
    transient: bool,
    subject: &str,
    seq: u64,
) {
    let payload = MailSendFailedEvent {
        account_id: account_id.to_string(),
        error: error.to_string(),
        transient,
        subject: subject.to_string(),
        seq,
    };
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

/// An AI-generated reply draft is ready. The frontend inserts it as an
/// ephemeral Drafts entry and opens it in the composer. `in_reply_to` is the
/// seed email's id; `references` are the prior-mail ids folded into the prompt
/// (surfaced for transparency). Body is rendered HTML.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MailDraftGeneratedEvent {
    pub account_id: String,
    pub in_reply_to: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body_html: String,
    pub references: Vec<String>,
    pub seq: u64,
}

pub fn emit_mail_draft_generated(app: &AppHandle, payload: MailDraftGeneratedEvent) {
    if let Err(e) = app.emit(MAIL_DRAFT_GENERATED, payload) {
        log::error!("Failed to emit {}: {}", MAIL_DRAFT_GENERATED, e);
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

/// A contact save or delete did not reach the CardDAV server (or the local DB).
/// Contacts are applied optimistically in the UI, so this is the rollback
/// signal; without it the earlier 400/409 failures were invisible (2026-08-30).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactSaveFailedEvent {
    pub account_id: String,
    pub contact_id: String,
    /// "save" or "delete"
    pub op: String,
    pub error: String,
    pub seq: u64,
}

pub fn emit_contact_save_failed(app: &AppHandle, account_id: &str, contact_id: &str, op: &str, error: &str, seq: u64) {
    let payload = ContactSaveFailedEvent {
        account_id: account_id.to_string(), contact_id: contact_id.to_string(),
        op: op.to_string(), error: error.to_string(), seq,
    };
    if let Err(e) = app.emit(CONTACTS_CONTACT_SAVE_FAILED, payload) {
        log::error!("Failed to emit {}: {}", CONTACTS_CONTACT_SAVE_FAILED, e);
    }
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

// ── Settings ────────────────────────────────────────────

/// An account row was created or updated. The full Account is sent so the
/// frontend can replace its local entry without a separate fetch. `requestedId`
/// echoes the caller's id so a parent that holds a temp/new id can reconcile.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsAccountUpsertedEvent {
    pub requested_id: String,
    pub account: Account,
    pub seq: u64,
}

pub fn emit_settings_account_upserted(app: &AppHandle, payload: SettingsAccountUpsertedEvent) {
    if let Err(e) = app.emit(SETTINGS_ACCOUNT_UPSERTED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_ACCOUNT_UPSERTED, e);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsAccountRemovedEvent {
    pub account_id: String,
    pub seq: u64,
}

pub fn emit_settings_account_removed(app: &AppHandle, account_id: &str, seq: u64) {
    let payload = SettingsAccountRemovedEvent { account_id: account_id.to_string(), seq };
    if let Err(e) = app.emit(SETTINGS_ACCOUNT_REMOVED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_ACCOUNT_REMOVED, e);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsAccountsReorderedEvent {
    pub ordered_ids: Vec<String>,
    pub seq: u64,
}

pub fn emit_settings_accounts_reordered(app: &AppHandle, ordered_ids: Vec<String>, seq: u64) {
    let payload = SettingsAccountsReorderedEvent { ordered_ids, seq };
    if let Err(e) = app.emit(SETTINGS_ACCOUNTS_REORDERED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_ACCOUNTS_REORDERED, e);
    }
}

/// A simple key/value setting changed (theme, accentColor, locale, …).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsKvChangedEvent {
    pub key: String,
    pub value: String,
    pub seq: u64,
}

pub fn emit_settings_kv_changed(app: &AppHandle, key: &str, value: &str, seq: u64) {
    let payload = SettingsKvChangedEvent { key: key.to_string(), value: value.to_string(), seq };
    if let Err(e) = app.emit(SETTINGS_KV_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_KV_CHANGED, e);
    }
}

/// Storage quota was set (`quotaBytes != null`) or cleared (`quotaBytes == null`).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsStorageQuotaChangedEvent {
    pub quota_bytes: Option<u64>,
    pub seq: u64,
}

pub fn emit_settings_storage_quota_changed(app: &AppHandle, quota_bytes: Option<u64>, seq: u64) {
    let payload = SettingsStorageQuotaChangedEvent { quota_bytes, seq };
    if let Err(e) = app.emit(SETTINGS_STORAGE_QUOTA_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_STORAGE_QUOTA_CHANGED, e);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsOfflineDownloadChangedEvent {
    pub account_id: String,
    pub enabled: bool,
    pub seq: u64,
}

pub fn emit_settings_offline_download_changed(
    app: &AppHandle,
    account_id: &str,
    enabled: bool,
    seq: u64,
) {
    let payload = SettingsOfflineDownloadChangedEvent {
        account_id: account_id.to_string(),
        enabled,
        seq,
    };
    if let Err(e) = app.emit(SETTINGS_OFFLINE_DOWNLOAD_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_OFFLINE_DOWNLOAD_CHANGED, e);
    }
}

/// The global AI config changed. Carries the non-secret fields plus `hasKey`
/// (never the API key itself); the frontend replaces its cached config.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsAiConfigChangedEvent {
    pub enabled: bool,
    pub base_url: String,
    pub has_key: bool,
    pub classify_model: String,
    pub draft_model: String,
    pub max_tokens: i32,
    pub temperature: f64,
    pub seq: u64,
}

pub fn emit_settings_ai_config_changed(app: &AppHandle, payload: SettingsAiConfigChangedEvent) {
    if let Err(e) = app.emit(SETTINGS_AI_CONFIG_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_AI_CONFIG_CHANGED, e);
    }
}

/// A per-account AI feature toggle set changed.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsAiFeaturesChangedEvent {
    pub account_id: String,
    pub junk_filter: bool,
    pub priority_detect: bool,
    pub auto_draft: bool,
    pub external_access: bool,
    pub seq: u64,
}

pub fn emit_settings_ai_features_changed(app: &AppHandle, payload: SettingsAiFeaturesChangedEvent) {
    if let Err(e) = app.emit(SETTINGS_AI_FEATURES_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_AI_FEATURES_CHANGED, e);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsMailServerSavedEvent {
    pub account_id: String,
    pub settings: MailServerSettings,
    pub seq: u64,
}

pub fn emit_settings_mail_server_saved(app: &AppHandle, payload: SettingsMailServerSavedEvent) {
    if let Err(e) = app.emit(SETTINGS_MAIL_SERVER_SAVED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_MAIL_SERVER_SAVED, e);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDavSavedEvent {
    pub account_id: String,
    pub settings: DavSettings,
    pub seq: u64,
}

pub fn emit_settings_caldav_saved(app: &AppHandle, payload: SettingsDavSavedEvent) {
    if let Err(e) = app.emit(SETTINGS_CALDAV_SAVED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_CALDAV_SAVED, e);
    }
}

pub fn emit_settings_carddav_saved(app: &AppHandle, payload: SettingsDavSavedEvent) {
    if let Err(e) = app.emit(SETTINGS_CARDDAV_SAVED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_CARDDAV_SAVED, e);
    }
}

/// Local DAV server started (`addr` is the bound address) or stopped (`addr` is null).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDavServerChangedEvent {
    pub addr: Option<String>,
    pub seq: u64,
}

pub fn emit_settings_dav_server_changed(app: &AppHandle, addr: Option<String>, seq: u64) {
    let payload = SettingsDavServerChangedEvent { addr, seq };
    if let Err(e) = app.emit(SETTINGS_DAV_SERVER_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_DAV_SERVER_CHANGED, e);
    }
}

/// DAV-server user list changed (add/remove). Carries the full current
/// `[email, accountId]` pairs so the frontend can replace its list wholesale.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDavUsersChangedEvent {
    pub users: Vec<(String, String)>,
    pub seq: u64,
}

pub fn emit_settings_dav_users_changed(app: &AppHandle, users: Vec<(String, String)>, seq: u64) {
    let payload = SettingsDavUsersChangedEvent { users, seq };
    if let Err(e) = app.emit(SETTINGS_DAV_USERS_CHANGED, payload) {
        log::error!("Failed to emit {}: {}", SETTINGS_DAV_USERS_CHANGED, e);
    }
}
