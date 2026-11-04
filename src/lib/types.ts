export interface Contact {
  name: string;
  email: string;
  initials: string;
  color: string;
  photoUrl?: string;
}

export type ContactEmailLabel = 'home' | 'work' | 'other';
export type ContactPhoneLabel = 'home' | 'work' | 'cell' | 'other';
export type ContactAddressLabel = 'home' | 'work' | 'other';
export type ContactPhoneSubtype = 'voice' | 'text' | 'fax' | 'video' | 'pager';

export interface ContactEmailEntry {
  email: string;
  label: ContactEmailLabel | string;
  isDefault: boolean;
}

export interface ContactPhoneEntry {
  number: string;
  label: ContactPhoneLabel | string;
  subtypes: (ContactPhoneSubtype | string)[];
  isDefault: boolean;
}

export interface ContactAddressEntry {
  street: string;
  city: string;
  region: string;
  postalCode: string;
  country: string;
  label: ContactAddressLabel | string;
  isDefault: boolean;
}

export interface FullContact extends Contact {
  id: string;
  firstName?: string;
  lastName?: string;
  middleName?: string;
  prefix?: string;
  suffix?: string;
  organization?: string;
  emails: ContactEmailEntry[];
  phones: ContactPhoneEntry[];
  addresses: ContactAddressEntry[];
  jobTitle?: string;
  department?: string;
  birthday?: string;
  notes?: string;
  isFavorite: boolean;
  photoUrl?: string;
  /** ISO-ish timestamp from vCard REV (e.g. 20140301T221110Z). */
  rev?: string;
  isReadOnly?: boolean;
}

export interface Attachment {
  index: number;
  filename: string;
  mimeType: string;
  size: number;
}

export interface Email {
  id: string;
  from: Contact;
  to: Contact[];
  cc?: Contact[];
  subject: string;
  preview: string;
  body: string;
  /** Lowercased, HTML-stripped body text cached for free-text search. Populated when body is fetched; absent until then. */
  searchText?: string;
  date: Date;
  isRead: boolean;
  isStarred: boolean;
  isPinned: boolean;
  hasAttachment: boolean;
  attachments?: Attachment[];
  folder: string;
  /** Inbox priority bucket (true = Priority, false = Regular). */
  isFocused?: boolean;
  replyTo?: string;
  /** A reply cannot reach anyone (no-reply address, or a very confident model
   * verdict). Reply / Reply All are disabled; Forward stays available. */
  noReply?: boolean;
  messageId?: string;
  /** SPF/DKIM/DMARC authentication results, e.g. "spf=pass dkim=pass dmarc=pass" */
  authResults?: string;
  /** Whether the email has been replied to (\Answered IMAP flag). */
  isReplied?: boolean;
  /** ISO timestamp of when the reply was sent (only set for replies sent from this app). */
  repliedAt?: string;
  /** Compose state stored for draft emails so they can be reopened in ComposePane */
  draftData?: ComposeDraft;
  /** True while the email is being sent in the background (hidden from the drafts list). */
  isSending?: boolean;
  /** User-visible labels (currently Gmail X-GM-LABELS sans internal markers). */
  labels?: string[];
}

export interface Folder {
  id: string;
  name: string;
  icon: string;
  isFavorite: boolean;
  isSystem?: boolean;
}

export interface EventAttendee {
  name: string;
  email: string;
  initials: string;
  color: string;
  role: 'required' | 'optional';
}

export interface EventRecurrence {
  freq: 'daily' | 'weekly' | 'monthly' | 'yearly';
  interval: number;
  endDate?: string;  // ISO date string e.g. "2026-12-31"
  byDay?: string;    // monthly-by-weekday: e.g. "3TU" (3rd Tuesday), "-1MO" (last Monday)
  exdates?: string[]; // exception dates (deleted occurrences), e.g. ["2026-04-15"]
}

export interface CalendarEvent {
  id: string;
  title: string;
  start: Date;
  end: Date;
  color: string;
  location?: string;
  description?: string;
  isAllDay: boolean;
  calendarId: string;
  calendarName: string;
  attendees?: EventAttendee[];
  recurrence?: EventRecurrence;
  isOnlineMeeting?: boolean;
  meetingUrl?: string;
  alertMinutes?: number;     // minutes before event to alert; 0 = at time; undefined = no alert
}

/**
 * A calendar invitation parsed out of an `.ics` attachment, shown for review
 * before the user decides whether to import it into the calendar.
 */
export interface CalendarInvite {
  /** iCalendar UID, carried into the imported event. */
  uid: string;
  /** iTIP METHOD: 'REQUEST' | 'CANCEL' | 'REPLY' | 'PUBLISH' | '' when absent. */
  method: string;
  title: string;
  /** Local-time or UTC ISO string, as written in the invitation. */
  start: string;
  end: string;
  isAllDay: boolean;
  location?: string;
  description?: string;
  organizer?: EventAttendee;
  attendees: EventAttendee[];
  recurrence?: EventRecurrence;
  alertMinutes?: number;
  /** The zone the organiser wrote the times in, when the file names one.
   *  `start`/`end` are already converted to UTC; this only labels them. */
  sourceTzid?: string;
  /** Attachment filename, so the preview can offer Save As… under it. */
  filename: string;
}

export interface CalendarCategory {
  id: string;
  name: string;
  color: string;
  visible: boolean;
  group: 'my' | 'other';
}

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

export interface CalDavSettings {
  url: string;
  username: string;
  password: string;
}

export interface CardDavSettings {
  url: string;
  username: string;
  password: string;
}

/** Global LLM config as returned by get_ai_config — never includes the key. */
export interface AiConfigView {
  enabled: boolean;
  baseUrl: string;
  hasKey: boolean;
  classifyModel: string;
  draftModel: string;
  maxTokens: number;
  temperature: number;
  decisionBaseUrl: string;
  hasDecisionKey: boolean;
  decisionModel: string;
}

/** Global LLM config sent to set_ai_config / test_ai_config. An empty apiKey
 * preserves the stored key (the UI never receives the real key). */
export interface AiConfigInput {
  enabled: boolean;
  baseUrl: string;
  apiKey: string;
  classifyModel: string;
  draftModel: string;
  maxTokens: number;
  temperature: number;
  /** Decision model (TypeSafe Jev protocol). Empty base URL = unset. */
  decisionBaseUrl: string;
  /** Same keep-on-empty contract as apiKey. */
  decisionApiKey: string;
  decisionModel: string;
}

/** Per-account AI feature toggles (all default off). */
export interface AccountAiFeatures {
  junkFilter: boolean;
  priorityDetect: boolean;
  autoDraft: boolean;
  externalAccess: boolean;
  replyDetect: boolean;
}

export interface Account {
  id: string;
  name: string;
  alias?: string;
  email: string;
  initials: string;
  color: string;
  avatarUrl?: string;
  /**
   * The login (credential set) this account is a view onto. Several accounts
   * share one when several addresses are delivered to the same server account;
   * their mail is fetched and stored once, under this id. Sync loops must
   * iterate unique logins rather than accounts.
   */
  loginId?: string;
  /**
   * Show only mail addressed to `email`, rather than everything on the login.
   * A pure read-time filter — toggling it needs no re-sync.
   */
  isMailbox?: boolean;
  folders: Folder[];
  emails: Email[];
  calendarEvents: CalendarEvent[];
  calendarCategories: CalendarCategory[];
  contacts: FullContact[];
  serverSettings?: MailServerSettings;
  calDavSettings?: CalDavSettings;
  cardDavSettings?: CardDavSettings;
  signature?: string;
}

export interface RecipientSuggestion {
  name: string;
  email: string;
  initials: string;
  color: string;
  photoUrl?: string;
  isFavorite: boolean;
  source: 'contact' | 'sender' | 'list';
  listId?: string;
}

export interface ContactListMember {
  name: string;
  email: string;
}

export interface ContactList {
  id: string;
  name: string;
  members: ContactListMember[];
}

/** Which window controls this environment expects the app to draw — see
 *  `src-tauri/src/chrome.rs`. A tiling compositor implements neither minimize
 *  nor maximize, so drawing those buttons there would draw buttons that do
 *  nothing. */
export interface WindowChrome {
  controls: boolean;
  minimize: boolean;
  maximize: boolean;
  /** Leave the left of the bar clear for the platform's own controls. */
  trafficLightInset: boolean;
}

export type NavItem = 'mail' | 'calendar' | 'contacts';
export type CalendarViewMode = 'day' | 'week' | 'month';
export type ComposeMode = 'new' | 'reply' | 'replyAll' | 'forward';
export type Theme = 'light' | 'dark' | 'system';

export interface ComposeDraft {
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  showCc: boolean;
  showBcc: boolean;
  attachments: { name: string; path: string; size: number }[];
}
