/**
 * dataService.ts — Bridge between the frontend and the SQLite database via Tauri IPC.
 * Falls back to in-memory mockData when running outside Tauri (e.g. `npm run dev` in browser).
 */

import type { Account, Email, Theme } from '$lib/types';
import { mockAccounts } from '$lib/data/mockData';
import { incNetwork, decNetwork } from '$lib/networkActivity.svelte';
import { buildSearchText } from '$lib/searchQuery';

// ── Tauri detection ─────────────────────────────────────

let invoke: ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>) | null = null;

async function getInvoke() {
  if (invoke) return invoke;
  try {
    const mod = await import('@tauri-apps/api/core');
    const real = mod.invoke;
    invoke = ((cmd: string, args?: Record<string, unknown>) => {
      incNetwork();
      return (real(cmd, args) as Promise<unknown>).finally(() => decNetwork());
    }) as typeof real;
    return invoke;
  } catch {
    return null;
  }
}

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

// ── Date parsing helper ─────────────────────────────────

/** Convert ISO date strings in account data back to Date objects */
function ensureFocusedFlag(email: Email): Email {
  if (typeof email.isFocused === 'boolean') return email;
  return {
    ...email,
    // Until backend classification lands, default inbox to Focused and non-inbox as Focused.
    isFocused: true,
  };
}

function hydrateAccountDates(account: Account): Account {
  return {
    ...account,
    emails: account.emails.map((e) => ({
      ...ensureFocusedFlag(e),
      date: new Date(e.date),
      searchText: e.body ? buildSearchText(e.body) : undefined,
    })),
    calendarEvents: account.calendarEvents.map((ev) => ({
      ...ev,
      start: new Date(ev.start),
      end: new Date(ev.end),
    })),
  } as unknown as Account;
}

async function hydratePersistedAccountSettings(account: Account): Promise<Account> {
  const [serverSettings, calDavSettings, cardDavSettings] = await Promise.all([
    loadMailSettings(account.id),
    loadCalDavSettings(account.id),
    loadCardDavSettings(account.id),
  ]);

  return {
    ...account,
    serverSettings: serverSettings ?? undefined,
    calDavSettings: calDavSettings ?? undefined,
    cardDavSettings: cardDavSettings ?? undefined,
  };
}

// ── Public API ──────────────────────────────────────────

/** Load all accounts with their full nested data. */
export async function loadAccounts(): Promise<Account[]> {
  if (!isTauri()) {
    // Web-only fallback — use mock data
    return mockAccounts.map(hydrateAccountDates);
  }
  const inv = await getInvoke();
  if (!inv) return mockAccounts.map(hydrateAccountDates);

  const raw = (await inv('get_all_accounts')) as Account[];
  const hydrated = raw.map(hydrateAccountDates);
  return Promise.all(hydrated.map(hydratePersistedAccountSettings));
}

/** Load a setting by key. Returns null if not found. */
export async function loadSetting(key: string): Promise<string | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('get_setting', { key })) as string | null;
}

/** Persist a setting key/value pair. */
export async function saveSetting(key: string, value: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('set_setting', { key, value });
}

/** Persist theme preference. */
export async function saveTheme(theme: Theme): Promise<void> {
  await saveSetting('theme', theme);
}

/** Load persisted theme preference. */
export async function loadTheme(): Promise<Theme> {
  const val = await loadSetting('theme');
  if (val === 'dark') return 'dark';
  if (val === 'light') return 'light';
  return 'system';
}

/** Persist accent color preference. */
export async function saveAccentColor(colorId: string): Promise<void> {
  await saveSetting('accentColor', colorId);
}

/** Load persisted accent color preference. */
export async function loadAccentColor(): Promise<string> {
  const val = await loadSetting('accentColor');
  return val || 'blue';
}

/** Persist language/locale preference. */
export async function saveLocale(code: string): Promise<void> {
  await saveSetting('locale', code);
}

/** Load persisted locale, or null to auto-detect. */
export async function loadLocale(): Promise<string | null> {
  return loadSetting('locale');
}

// ── Storage quota ──────────────────────────────────────

export interface StorageInfo {
  /** Bytes free on the volume where the DB lives. */
  freeBytes: number;
  /** Body bytes stored across all emails (with a small overhead pad). */
  usedBytes: number;
  /** Configured app-wide quota; null when no limit is set. */
  quotaBytes: number | null;
  /** Floor on any quota value: 100 MB. */
  minQuotaBytes: number;
  /** Ceiling on any quota value: 50% of freeBytes (re-read at call time). */
  maxQuotaBytes: number;
  /** True iff maxQuotaBytes >= minQuotaBytes (i.e. at least 200 MB free). */
  canEnable: boolean;
}

const WEB_FALLBACK_STORAGE: StorageInfo = {
  freeBytes: 0,
  usedBytes: 0,
  quotaBytes: null,
  minQuotaBytes: 100 * 1024 * 1024,
  maxQuotaBytes: 0,
  canEnable: false,
};

export async function getStorageInfo(): Promise<StorageInfo> {
  if (!isTauri()) return WEB_FALLBACK_STORAGE;
  const inv = await getInvoke();
  if (!inv) return WEB_FALLBACK_STORAGE;
  return (await inv('get_storage_info')) as StorageInfo;
}

/** Set the app-wide storage quota in bytes. Pass 0 to clear (also disables all per-account offline toggles). */
export async function setStorageQuota(bytes: number): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('set_storage_quota', { bytes });
}

export async function getAccountOfflineDownload(accountId: string): Promise<boolean> {
  if (!isTauri()) return false;
  const inv = await getInvoke();
  if (!inv) return false;
  return (await inv('get_account_offline_download', { accountId })) as boolean;
}

export async function setAccountOfflineDownload(accountId: string, enabled: boolean): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('set_account_offline_download', { accountId, enabled });
}

export interface OfflineDownloadStatus {
  enabled: boolean;
  totalCount: number;
  pendingCount: number;
}

export async function getOfflineDownloadStatus(accountId: string): Promise<OfflineDownloadStatus> {
  if (!isTauri()) return { enabled: false, totalCount: 0, pendingCount: 0 };
  const inv = await getInvoke();
  if (!inv) return { enabled: false, totalCount: 0, pendingCount: 0 };
  return (await inv('get_offline_download_status', { accountId })) as OfflineDownloadStatus;
}

/** Update an existing account (metadata only — name, email, initials, color, avatarUrl). */
export async function updateAccount(account: Account): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('update_account', {
    id: account.id,
    name: account.name,
    email: account.email,
    initials: account.initials,
    color: account.color,
    avatarUrl: account.avatarUrl || null,
    alias: account.alias || null,
  });
}

/** Add a new account. */
export async function addAccount(data: {
  id: string;
  name: string;
  email: string;
  initials: string;
  color: string;
  avatarUrl?: string;
  alias?: string;
}): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('add_account', { ...data, avatarUrl: data.avatarUrl || null, alias: data.alias || null });
}

/** Delete an account by id. */
export async function deleteAccount(id: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('delete_account', { id });
}

/** Persist a new account display order. `orderedIds` lists account ids in the desired order. */
export async function setAccountPositions(orderedIds: string[]): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('set_account_positions', { orderedIds });
}

/** Mark an email as read/unread. */
export async function markEmailRead(accountId: string, emailId: string, isRead: boolean): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('update_email_read', { accountId, emailId, isRead });
}

/** Toggle the starred flag on an email. */
export async function updateEmailStarred(accountId: string, emailId: string, isStarred: boolean): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('update_email_starred', { accountId, emailId, isStarred });
}

/** Set the replied flag on an email (\Answered IMAP flag). */
export async function updateEmailReplied(accountId: string, emailId: string, isReplied: boolean): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('update_email_replied', { accountId, emailId, isReplied });
}

/** Toggle the pinned flag on an email. */
export async function updateEmailPinned(emailId: string, isPinned: boolean): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('update_email_pinned', { emailId, isPinned });
}

export async function updateEmailFocused(emailId: string, isFocused: boolean): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('update_email_focused', { emailId, isFocused });
}

/** Delete an email (moves to Deleted Items, or permanently deletes if already there). */
export async function deleteEmail(accountId: string, emailId: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('delete_email', { accountId, emailId });
}

/** Permanently delete all emails in a folder (e.g. Deleted, Junk). */
export async function emptyFolder(accountId: string, folder: string): Promise<number> {
  if (!isTauri()) return 0;
  const inv = await getInvoke();
  if (!inv) return 0;
  return (await inv('empty_folder', { accountId, folder })) as number;
}

/** Move an email to a different folder (Archive, Junk, Inbox, etc.). */
export async function moveEmail(accountId: string, emailId: string, targetFolder: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('move_email', { accountId, emailId, targetFolder });
}

// ── Server settings ─────────────────────────────────────

export interface MailServerSettings {
  protocol: 'imap' | 'pop3';
  incomingServer: string;
  incomingPort: number;
  incomingUsername: string;
  incomingPassword: string;
  incomingSecurity: 'ssl' | 'tls' | 'none';
  smtpServer: string;
  smtpPort: number;
  smtpUsername: string;
  smtpPassword: string;
  smtpSecurity: 'ssl' | 'tls' | 'none';
  syncIntervalMinutes: number;
}

export interface DavSettings {
  url: string;
  username: string;
  password: string;
}

export async function saveMailSettings(accountId: string, settings: MailServerSettings): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('save_mail_settings', { accountId, settings });
}

export async function loadMailSettings(accountId: string): Promise<MailServerSettings | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('load_mail_settings', { accountId })) as MailServerSettings | null;
}

export async function saveCalDavSettings(accountId: string, settings: DavSettings): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('save_caldav_settings', { accountId, settings });
}

export async function loadCalDavSettings(accountId: string): Promise<DavSettings | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('load_caldav_settings', { accountId })) as DavSettings | null;
}

export async function saveCardDavSettings(accountId: string, settings: DavSettings): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('save_carddav_settings', { accountId, settings });
}

export async function loadCardDavSettings(accountId: string): Promise<DavSettings | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('load_carddav_settings', { accountId })) as DavSettings | null;
}

// ── Auto-discovery ──────────────────────────────────────

export interface ServerConfig {
  protocol: string;
  hostname: string;
  port: number;
  socketType: string;
  auth: string;
  usernameTemplate: string;
}

export interface DiscoveredConfig {
  displayName: string;
  incoming: ServerConfig[];
  outgoing: ServerConfig[];
  source: string;
}

/** Auto-discover mail server settings for an email address.
 *  Queries Mozilla ISPDB, domain autoconfig, MS Autodiscover, DNS SRV, and port probing. */
export async function discoverMailSettings(email: string): Promise<DiscoveredConfig | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('discover_mail_settings', { email })) as DiscoveredConfig;
}

// ── Sync operations ─────────────────────────────────────

export interface MailSyncResult {
  newCount: number;
  deletedCount: number;
  flagUpdates: number;
  errors: string[];
}

export interface CalSyncResult {
  newCount: number;
  updatedCount: number;
  deletedCount: number;
}

export interface ContactSyncResult {
  newCount: number;
  updatedCount: number;
  deletedCount: number;
}

/** Sync mail (IMAP) for a given account. */
export async function syncMail(accountId: string): Promise<MailSyncResult | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('sync_mail', { accountId })) as MailSyncResult;
}

/** Sync calendars (CalDAV) for a given account. */
export async function syncCalendars(accountId: string): Promise<CalSyncResult | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('sync_calendars', { accountId })) as CalSyncResult;
}

/** Sync contacts (CardDAV) for a given account. */
export async function syncContacts(accountId: string): Promise<ContactSyncResult | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('sync_contacts', { accountId })) as ContactSyncResult;
}

// ── CalDAV CRUD ─────────────────────────────────────────

/** Create or update a calendar event on the CalDAV server + local DB. Returns the local event id. */
export async function saveCalendarEvent(
  accountId: string,
  eventId: string,
  title: string,
  start: string,
  end: string,
  location: string | undefined,
  description: string | undefined,
  isAllDay: boolean,
  calendarId: string,
  attendees: { name: string; email: string; initials: string; color: string; role: string }[],
  recurrence: { freq: string; interval: number; endDate?: string; byDay?: string; exdates?: string[] } | undefined,
  isOnlineMeeting: boolean,
  meetingUrl: string | undefined,
  alertMinutes: number | undefined,
): Promise<string | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('save_calendar_event', {
    accountId, eventId, title, start, end,
    location: location || null,
    description: description || null,
    isAllDay, calendarId,
    attendees,
    recurrence: recurrence || null,
    isOnlineMeeting,
    meetingUrl: meetingUrl || null,
    alertMinutes: alertMinutes ?? null,
  })) as string;
}

/** Delete a calendar event from the CalDAV server + local DB. */
export async function deleteCalendarEvent(accountId: string, eventId: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('delete_calendar_event', { accountId, eventId });
}

// ── CardDAV CRUD ────────────────────────────────────────

/** Create or update a contact on the CardDAV server + local DB. Returns the local contact id. */
export async function saveContactEntry(
  accountId: string,
  contact: {
    id: string;
    name: string;
    firstName?: string;
    lastName?: string;
    middleName?: string;
    prefix?: string;
    suffix?: string;
    emails: import('$lib/types').ContactEmailEntry[];
    phones: import('$lib/types').ContactPhoneEntry[];
    addresses: import('$lib/types').ContactAddressEntry[];
    jobTitle?: string;
    department?: string;
    organization?: string;
    birthday?: string;
    notes?: string;
    isFavorite: boolean;
    photoUrl?: string;
  },
): Promise<string | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('save_contact_entry', {
    accountId,
    contactId: contact.id,
    name: contact.name,
    firstName: contact.firstName || null,
    lastName: contact.lastName || null,
    middleName: contact.middleName || null,
    prefix: contact.prefix || null,
    suffix: contact.suffix || null,
    emails: contact.emails,
    phones: contact.phones,
    addresses: contact.addresses,
    jobTitle: contact.jobTitle || null,
    department: contact.department || null,
    organization: contact.organization || null,
    birthday: contact.birthday || null,
    notes: contact.notes || null,
    isFavorite: contact.isFavorite,
    photoUrl: contact.photoUrl || null,
  })) as string;
}

/** Delete a contact from the CardDAV server + local DB. */
export async function deleteContactEntry(accountId: string, contactId: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('delete_contact_entry', { accountId, contactId });
}

// ── Send email ──────────────────────────────────────────

export interface OutboundEmail {
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  bodyHtml: string;
  attachments: { name: string; path: string }[];
}

/** Send an email via SMTP. Falls back to outbox queue on failure. */
export async function sendEmail(accountId: string, email: OutboundEmail): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('send_email', { accountId, email });
}

/** Flush the outbox queue (retry pending sends). */
export async function flushOutbox(accountId: string): Promise<number> {
  if (!isTauri()) return 0;
  const inv = await getInvoke();
  if (!inv) return 0;
  return (await inv('flush_outbox', { accountId })) as number;
}

// ── Fetch email body on demand ──────────────────────────

/** Fetch the full HTML body of an email (lazy-loaded from IMAP). */
export async function fetchEmailBody(accountId: string, emailId: string): Promise<{ body: string; preview: string; authResults?: string; hasAttachment?: boolean; attachments?: import('$lib/types').Attachment[] } | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('fetch_email_body', { accountId, emailId })) as { body: string; preview: string; authResults?: string; hasAttachment?: boolean; attachments?: import('$lib/types').Attachment[] };
}

/** Fill previews for messages near an anchor (one IMAP round trip). */
export interface PreviewUpdate {
  id: string;
  body: string;
  preview: string;
  authResults?: string;
  hasAttachment?: boolean;
  attachments?: import('$lib/types').Attachment[];
}

export async function fetchPreviewsAround(
  accountId: string,
  emailId: string,
  ahead: number = 20,
  behind: number = 10,
): Promise<PreviewUpdate[]> {
  if (!isTauri()) return [];
  const inv = await getInvoke();
  if (!inv) return [];
  const res = (await inv('fetch_previews_around', { accountId, emailId, ahead, behind })) as { updates: PreviewUpdate[] };
  return res?.updates ?? [];
}

/** Open an attachment using the system default application. */
export async function openAttachment(accountId: string, emailId: string, attachmentIndex: number): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('open_attachment', { accountId, emailId, attachmentIndex });
}

/** Save an attachment to a user-specified file path. */
export async function saveAttachment(accountId: string, emailId: string, attachmentIndex: number, savePath: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('save_attachment', { accountId, emailId, attachmentIndex, savePath });
}

// ── Connection tests ────────────────────────────────────

export async function testImapConnection(settings: MailServerSettings): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('test_imap_connection', { settings });
}

export async function testSmtpConnection(settings: MailServerSettings): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('test_smtp_connection', { settings });
}

export async function testCalDavConnection(url: string, username: string, password: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('test_caldav_connection', { url, username, password });
}

export async function testCardDavConnection(url: string, username: string, password: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('test_carddav_connection', { url, username, password });
}

// ── DAV server ──────────────────────────────────────────

/** Start the embedded DAV server on the given bind address (e.g. "0.0.0.0:5232"). */
export async function startDavServer(bindAddr: string): Promise<string | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('start_dav_server', { bindAddr })) as string;
}

/** Stop the embedded DAV server. */
export async function stopDavServer(): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('stop_dav_server');
}

/** Get the address the DAV server is listening on, or null if not running. */
export async function getDavServerStatus(): Promise<string | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('get_dav_server_status')) as string | null;
}

/** Add a user to the DAV server (email + password → account mapping). */
export async function addDavUser(email: string, password: string, accountId: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('add_dav_user', { email, password, accountId });
}

/** Remove a DAV server user. */
export async function removeDavUser(email: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('remove_dav_user', { email });
}

/** List all DAV server users as [email, accountId] tuples. */
export async function listDavUsers(): Promise<[string, string][]> {
  if (!isTauri()) return [];
  const inv = await getInvoke();
  if (!inv) return [];
  return (await inv('list_dav_users')) as [string, string][];
}

// ── Contact Lists ──────────────────────────────────────

import type { ContactList, ContactListMember } from '$lib/types';

/** Get all contact lists with members for an account. */
export async function getContactLists(accountId: string): Promise<ContactList[]> {
  if (!isTauri()) return [];
  const inv = await getInvoke();
  if (!inv) return [];
  return (await inv('get_contact_lists', { accountId })) as ContactList[];
}

/** Create or update a contact list. */
export async function saveContactList(accountId: string, id: string, name: string, members: ContactListMember[]): Promise<string | null> {
  if (!isTauri()) return null;
  const inv = await getInvoke();
  if (!inv) return null;
  return (await inv('save_contact_list', { accountId, id, name, members })) as string;
}

/** Delete a contact list. */
export async function deleteContactList(id: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('delete_contact_list', { id });
}

// ── Toggle Contact Favorite (local-only) ──────────────────

/** Toggle a contact's favorite status without a full CardDAV save. */
export async function toggleContactFavorite(contactId: string, isFavorite: boolean): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('toggle_contact_favorite', { contactId, isFavorite });
}

// ── Ignored (Muted) Addresses ─────────────────────────────

/** Add an email address to the ignored/muted list. */
export async function addIgnoredAddress(email: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('add_ignored_address', { email });
}

/** Remove an email address from the ignored/muted list. */
export async function removeIgnoredAddress(email: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('remove_ignored_address', { email });
}

/** Get all ignored/muted email addresses. */
export async function getIgnoredAddresses(): Promise<string[]> {
  if (!isTauri()) return [];
  const inv = await getInvoke();
  if (!inv) return [];
  return (await inv('get_ignored_addresses')) as string[];
}

// ── Sender Blocklist ──────────────────────────────────────

export interface SenderBlockEntry {
  email: string;
  accountId: string;
  listType: string;
  createdAt: string;
}

/** Add a sender to the blocklist (block or allow). */
export async function addToSenderBlocklist(accountId: string, email: string, listType: string = 'block'): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('add_to_sender_blocklist', { accountId, email, listType });
}

/** Remove a sender from the blocklist. */
export async function removeFromSenderBlocklist(accountId: string, email: string): Promise<void> {
  if (!isTauri()) return;
  const inv = await getInvoke();
  if (!inv) return;
  await inv('remove_from_sender_blocklist', { accountId, email });
}

/** Get all blocklist entries for an account. */
export async function getSenderBlocklist(accountId: string): Promise<SenderBlockEntry[]> {
  if (!isTauri()) return [];
  const inv = await getInvoke();
  if (!inv) return [];
  return (await inv('get_sender_blocklist', { accountId })) as SenderBlockEntry[];
}
