/**
 * mailStore.svelte.ts — Phase 1 foundation slice.
 *
 * Owns the fire-and-forget bridge for mail mutations: actions queue an
 * `invoke(...)` without awaiting and the backend emits `mail:email-flag-changed`
 * when the work (DB write + IMAP round trip) is done. The page subscribes via
 * `subscribeFlagEvents` and applies the echoed value into its own emails array.
 *
 * In a later phase the emails array itself moves into this store; for now we
 * just centralize the bookkeeping (pending ops, monotonic-seq stale-echo drop,
 * and the network indicator counter) so all the new mail actions can be
 * grafted onto the same plumbing.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { incNetwork, decNetwork } from '$lib/networkActivity.svelte';

export type EmailFlagField = 'read' | 'starred' | 'replied' | 'pinned' | 'focused';

export interface EmailFlagChangedEvent {
  accountId: string;
  emailId: string;
  field: EmailFlagField;
  value: boolean;
  seq: number;
}

export interface EmailFolderChangedEvent {
  accountId: string;
  emailId: string;
  folder: string;
  seq: number;
}

export interface EmailRemovedEvent {
  accountId: string;
  emailIds: string[];
  seq: number;
}

export interface MailSyncCompletedEvent {
  accountId: string;
  ok: boolean;
  newCount: number;
  deletedCount: number;
  flagUpdates: number;
  errors: string[];
  seq: number;
}

export interface MailSendFailedEvent {
  accountId: string;
  error: string;
  seq: number;
}

export interface AttachmentMeta {
  index: number;
  filename: string;
  mimeType: string;
  size: number;
}

export interface EmailBodyFetchedEvent {
  accountId: string;
  emailId: string;
  body: string;
  preview: string;
  authResults: string;
  hasAttachment: boolean;
  attachments: AttachmentMeta[];
  seq: number;
}

export interface EmailPreviewUpdate {
  id: string;
  body: string;
  preview: string;
  authResults: string;
  hasAttachment: boolean;
  attachments: AttachmentMeta[];
}

export interface EmailPreviewsFetchedEvent {
  accountId: string;
  updates: EmailPreviewUpdate[];
  seq: number;
}

/** An AI-generated reply draft is ready (from `ai_draft_reply`). */
export interface MailDraftGeneratedEvent {
  accountId: string;
  inReplyTo: string | null;
  to: string[];
  cc: string[];
  subject: string;
  bodyHtml: string;
  references: string[];
  seq: number;
}

export interface OutboundEmailPayload {
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  bodyHtml: string;
  attachments: { name: string; path: string }[];
}

// ── Pending-op bookkeeping (reactive) ────────────────────
// `pendingOps.map[emailId]` is a Set of fields with in-flight backend work.
// Wholesale replacement on every mutation keeps Svelte reactivity working —
// don't mutate the inner Set in place.

export const pendingOps = $state<{ map: Record<string, Set<EmailFlagField>> }>({ map: {} });

export function isPending(emailId: string, field: EmailFlagField): boolean {
  return pendingOps.map[emailId]?.has(field) ?? false;
}

function markPending(emailId: string, field: EmailFlagField) {
  const cur = pendingOps.map[emailId] ?? new Set<EmailFlagField>();
  const next = new Set(cur);
  next.add(field);
  pendingOps.map = { ...pendingOps.map, [emailId]: next };
}

function clearPending(emailId: string, field: EmailFlagField) {
  const cur = pendingOps.map[emailId];
  if (!cur || !cur.has(field)) return;
  const next = new Set(cur);
  next.delete(field);
  const map = { ...pendingOps.map };
  if (next.size === 0) delete map[emailId];
  else map[emailId] = next;
  pendingOps.map = map;
}

// ── In-flight bookkeeping ───────────────────────────────
// Tracks how many of our own fires are still awaiting an echo for each
// (emailId, key) — `key` is a flag field name ("starred", "read", ...)
// for flag ops, or the literal "folder" for folder/delete ops. One inc
// per fire, one dec per matching echo — so a rapid star→unstar→star
// sequence holds pending until all three round trips have echoed, and
// the global network counter stays balanced.

const inFlightCounts = new Map<string, number>();
const seqKey = (emailId: string, key: string) => `${emailId}:${key}`;

function incInFlight(emailId: string, key: string) {
  const k = seqKey(emailId, key);
  inFlightCounts.set(k, (inFlightCounts.get(k) ?? 0) + 1);
  if (isFlagField(key)) markPending(emailId, key);
  incNetwork();
}

function decInFlight(emailId: string, key: string) {
  const k = seqKey(emailId, key);
  const next = (inFlightCounts.get(k) ?? 0) - 1;
  if (next <= 0) {
    inFlightCounts.delete(k);
    if (isFlagField(key)) clearPending(emailId, key);
  } else {
    inFlightCounts.set(k, next);
  }
  decNetwork();
}

function isFlagField(key: string): key is EmailFlagField {
  return key === 'read' || key === 'starred' || key === 'replied'
    || key === 'pinned' || key === 'focused';
}

// ── Stale-echo detection ────────────────────────────────
// Backend stamps each event with a monotonic seq. Rapid star→unstar can
// complete on IMAP in reverse order; we drop any echo whose seq is older
// than the last one we accepted for the same (emailId, key). The
// in-flight counter is settled regardless — stale only means "don't
// apply the value", not "ignore the event entirely".

const lastSeq = new Map<string, number>();

function acceptSeq(emailId: string, key: string, seq: number): boolean {
  const k = seqKey(emailId, key);
  const prev = lastSeq.get(k) ?? 0;
  if (seq <= prev) return false;
  lastSeq.set(k, seq);
  return true;
}

// ── Event subscription ──────────────────────────────────

export type FlagEchoHandler = (ev: EmailFlagChangedEvent) => void;

/** Subscribe to backend flag-change echoes. The handler receives only
 * fresh (non-stale) events. Callers own the emails array and apply the
 * new value themselves — this lets us roll the store out one action at
 * a time without ripping the page apart.
 *
 * Echoes for ops we didn't fire ourselves (e.g. an MCP agent toggled
 * the flag) still reach the handler — they just don't move the
 * in-flight counters.
 */
export async function subscribeFlagEvents(handler: FlagEchoHandler): Promise<UnlistenFn> {
  return listen<EmailFlagChangedEvent>('mail:email-flag-changed', (event) => {
    const ev = event.payload;
    if (inFlightCounts.has(seqKey(ev.emailId, ev.field))) {
      decInFlight(ev.emailId, ev.field);
    }
    if (!acceptSeq(ev.emailId, ev.field, ev.seq)) return;
    handler(ev);
  });
}

// ── Folder-change subscription ──────────────────────────
// Folder changes use a coarser bookkeeping model than flag changes: the
// "field" is always "folder" so there's just one in-flight slot per
// emailId. We still tag with monotonic seq for out-of-order safety.

export type FolderEchoHandler = (ev: EmailFolderChangedEvent) => void;
export type RemovedEchoHandler = (ev: EmailRemovedEvent) => void;

const FOLDER_KEY = 'folder';

export async function subscribeFolderEvents(handler: FolderEchoHandler): Promise<UnlistenFn> {
  return listen<EmailFolderChangedEvent>('mail:email-folder-changed', (event) => {
    const ev = event.payload;
    if (inFlightCounts.has(seqKey(ev.emailId, FOLDER_KEY))) {
      decInFlight(ev.emailId, FOLDER_KEY);
    }
    if (!acceptSeq(ev.emailId, FOLDER_KEY, ev.seq)) return;
    handler(ev);
  });
}

export async function subscribeRemovedEvents(handler: RemovedEchoHandler): Promise<UnlistenFn> {
  return listen<EmailRemovedEvent>('mail:email-removed', (event) => {
    const ev = event.payload;
    // A remove echo settles any in-flight folder op too — once a row is gone,
    // no later folder echo can apply to it.
    for (const id of ev.emailIds) {
      if (inFlightCounts.has(seqKey(id, FOLDER_KEY))) {
        decInFlight(id, FOLDER_KEY);
      }
    }
    handler(ev);
  });
}

// ── Actions (fire-and-forget) ───────────────────────────

function fireFlag(
  emailId: string,
  field: EmailFlagField,
  command: string,
  payload: Record<string, unknown>,
): void {
  incInFlight(emailId, field);
  invoke(command, payload).catch((err) => {
    console.error(`${command} failed before backgrounding:`, err);
    decInFlight(emailId, field);
  });
}

/** Toggle the starred flag on an email. */
export function fireToggleStar(accountId: string, emailId: string, newValue: boolean): void {
  fireFlag(emailId, 'starred', 'update_email_starred', {
    accountId, emailId, isStarred: newValue,
  });
}

/** Mark an email as read or unread. */
export function fireMarkRead(accountId: string, emailId: string, isRead: boolean): void {
  fireFlag(emailId, 'read', 'update_email_read', {
    accountId, emailId, isRead,
  });
}

/** Toggle the replied (\Answered) flag on an email. */
export function fireToggleReplied(accountId: string, emailId: string, isReplied: boolean): void {
  fireFlag(emailId, 'replied', 'update_email_replied', {
    accountId, emailId, isReplied,
  });
}

/** Toggle the pin flag (local-only DB state). */
export function fireTogglePinned(emailId: string, isPinned: boolean): void {
  fireFlag(emailId, 'pinned', 'update_email_pinned', { emailId, isPinned });
}

/** Toggle the focused flag (local-only DB state — Priority/Regular). */
export function fireToggleFocused(emailId: string, isFocused: boolean): void {
  fireFlag(emailId, 'focused', 'update_email_focused', { emailId, isFocused });
}

/** Move an email to a target folder. */
export function fireMoveEmail(accountId: string, emailId: string, targetFolder: string): void {
  incInFlight(emailId, FOLDER_KEY);
  invoke('move_email', { accountId, emailId, targetFolder }).catch((err) => {
    console.error('move_email failed before backgrounding:', err);
    decInFlight(emailId, FOLDER_KEY);
  });
}

/** Delete an email: soft-delete (moves to "deleted" folder) or permanent
 * if already there. Soft delete echoes `mail:email-folder-changed`;
 * permanent delete echoes `mail:email-removed`. */
export function fireDeleteEmail(accountId: string, emailId: string): void {
  incInFlight(emailId, FOLDER_KEY);
  invoke('delete_email', { accountId, emailId }).catch((err) => {
    console.error('delete_email failed before backgrounding:', err);
    decInFlight(emailId, FOLDER_KEY);
  });
}

/** Empty a folder (permanently remove every email in it). Echoes a
 * single `mail:email-removed` with the batch of removed ids. */
export function fireEmptyFolder(accountId: string, folder: string): void {
  // No per-email in-flight bookkeeping — the batch echo doesn't carry
  // individual seq numbers, just the list of ids to remove.
  incNetwork();
  invoke('empty_folder', { accountId, folder })
    .then(() => decNetwork())
    .catch((err) => {
      console.error('empty_folder failed:', err);
      decNetwork();
    });
}

// ── Send / sync / body / previews ────────────────────────
//
// These four use per-account/per-email "operation" keys in the in-flight
// counter so multiple concurrent ops (e.g. two simultaneous syncs across
// different accounts) don't collide on the same key. The completion echoes
// settle the corresponding inc.

export type SyncCompletedHandler = (ev: MailSyncCompletedEvent) => void;
export type SendFailedHandler = (ev: MailSendFailedEvent) => void;
export type SentHandler = (accountId: string) => void;
export type BodyFetchedHandler = (ev: EmailBodyFetchedEvent) => void;
export type PreviewsFetchedHandler = (ev: EmailPreviewsFetchedEvent) => void;

/** Fired after `sync_mail` finishes (success or error). The `errors` field
 * may be non-empty on partial success — that's a sync that completed but
 * with per-folder warnings. */
export async function subscribeSyncCompleted(handler: SyncCompletedHandler): Promise<UnlistenFn> {
  return listen<MailSyncCompletedEvent>('mail:sync-completed', (event) => {
    const ev = event.payload;
    const k = seqKey(`sync:${ev.accountId}`, '');
    if (inFlightCounts.has(k)) {
      const next = (inFlightCounts.get(k) ?? 0) - 1;
      if (next <= 0) inFlightCounts.delete(k); else inFlightCounts.set(k, next);
      decNetwork();
    }
    handler(ev);
  });
}

/** Fired only when `send_email` couldn't submit (SMTP rejected, etc.). The
 * outbox row stays in 'pending' state with the retry count incremented;
 * `flush_outbox` will retry. */
export async function subscribeSendFailed(handler: SendFailedHandler): Promise<UnlistenFn> {
  return listen<MailSendFailedEvent>('mail:send-failed', (event) => {
    const ev = event.payload;
    const k = seqKey(`send:${ev.accountId}`, '');
    if (inFlightCounts.has(k)) {
      const next = (inFlightCounts.get(k) ?? 0) - 1;
      if (next <= 0) inFlightCounts.delete(k); else inFlightCounts.set(k, next);
      decNetwork();
    }
    handler(ev);
  });
}

/** Fired when a send completes successfully (SMTP submit + IMAP APPEND to
 * Sent). Note: the existing `mail:sent` event carries only the accountId
 * as its payload (unchanged from before the refactor). */
export async function subscribeSent(handler: SentHandler): Promise<UnlistenFn> {
  return listen<string>('mail:sent', (event) => {
    const accountId = event.payload;
    const k = seqKey(`send:${accountId}`, '');
    if (inFlightCounts.has(k)) {
      const next = (inFlightCounts.get(k) ?? 0) - 1;
      if (next <= 0) inFlightCounts.delete(k); else inFlightCounts.set(k, next);
      decNetwork();
    }
    handler(accountId);
  });
}

/** Fired when `fetch_email_body` finishes. Only emitted on success. */
export async function subscribeBodyFetched(handler: BodyFetchedHandler): Promise<UnlistenFn> {
  return listen<EmailBodyFetchedEvent>('mail:email-body-fetched', (event) => {
    const ev = event.payload;
    const k = seqKey(`body:${ev.emailId}`, '');
    if (inFlightCounts.has(k)) {
      const next = (inFlightCounts.get(k) ?? 0) - 1;
      if (next <= 0) inFlightCounts.delete(k); else inFlightCounts.set(k, next);
      decNetwork();
    }
    if (!acceptSeq(`body:${ev.emailId}`, '', ev.seq)) return;
    handler(ev);
  });
}

/** Fired when `fetch_previews_around` finishes with a non-empty batch. */
export async function subscribePreviewsFetched(handler: PreviewsFetchedHandler): Promise<UnlistenFn> {
  return listen<EmailPreviewsFetchedEvent>('mail:email-previews-fetched', (event) => {
    // We don't dec the in-flight counter here — the previews fire-action uses
    // a promise .finally to balance its incNetwork, since the backend only
    // emits this event when the batch is non-empty (so an empty result still
    // needs the inc to be undone).
    handler(event.payload);
  });
}

export type DraftGeneratedHandler = (ev: MailDraftGeneratedEvent) => void;

/** Fired when `ai_draft_reply` produces a draft. The frontend turns it into an
 * ephemeral Drafts entry and opens the composer. */
export async function subscribeDraftGenerated(handler: DraftGeneratedHandler): Promise<UnlistenFn> {
  return listen<MailDraftGeneratedEvent>('mail:draft-generated', (event) => {
    handler(event.payload);
  });
}

function incOp(key: string) {
  inFlightCounts.set(key, (inFlightCounts.get(key) ?? 0) + 1);
  incNetwork();
}

function decOp(key: string) {
  const next = (inFlightCounts.get(key) ?? 0) - 1;
  if (next <= 0) inFlightCounts.delete(key); else inFlightCounts.set(key, next);
  decNetwork();
}

/** Send an email. Completion is signaled by `subscribeSent` on success or
 * `subscribeSendFailed` on SMTP failure. */
export function fireSendEmail(accountId: string, email: OutboundEmailPayload): void {
  const opKey = seqKey(`send:${accountId}`, '');
  incOp(opKey);
  invoke('send_email', { accountId, email }).catch((err) => {
    // The catch covers errors thrown before backgrounding (e.g. no settings).
    // Real SMTP failures come via mail:send-failed.
    console.error('send_email rejected at invoke-time:', err);
    decOp(opKey);
  });
}

/** Trigger an IMAP sync. Completion via `subscribeSyncCompleted`. */
export function fireSyncMail(accountId: string): void {
  const opKey = seqKey(`sync:${accountId}`, '');
  incOp(opKey);
  invoke('sync_mail', { accountId }).catch((err) => {
    console.error('sync_mail rejected at invoke-time:', err);
    decOp(opKey);
  });
}

/** Request an AI reply draft for an email. The backend runs reference retrieval
 * + drafting and emits `mail:draft-generated` (see `subscribeDraftGenerated`).
 * Returns the invoke promise so callers can show a loading state and surface
 * errors; the draft itself arrives via the event. */
export function fireAiDraftReply(
  accountId: string,
  emailId: string,
  instructions?: string,
): Promise<void> {
  return invoke('ai_draft_reply', { accountId, emailId, instructions: instructions ?? null });
}

/** Fetch a single email body. Completion via `subscribeBodyFetched`. */
export function fireFetchEmailBody(accountId: string, emailId: string): void {
  const opKey = seqKey(`body:${emailId}`, '');
  incOp(opKey);
  invoke('fetch_email_body', { accountId, emailId }).catch((err) => {
    console.error('fetch_email_body rejected at invoke-time:', err);
    decOp(opKey);
  });
}

/** Fill previews near a viewport anchor. Completion via `subscribePreviewsFetched`. */
export function fireFetchPreviewsAround(
  accountId: string,
  emailId: string,
  ahead: number = 20,
  behind: number = 10,
): void {
  // No seq-based dedup for previews — multiple concurrent calls produce
  // non-overlapping batches and we want all of them applied.
  incNetwork();
  invoke('fetch_previews_around', { accountId, emailId, ahead, behind })
    .catch((err) => console.error('fetch_previews_around rejected at invoke-time:', err))
    .finally(() => decNetwork());
}
