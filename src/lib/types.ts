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

export interface Account {
  id: string;
  name: string;
  alias?: string;
  email: string;
  initials: string;
  color: string;
  avatarUrl?: string;
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
