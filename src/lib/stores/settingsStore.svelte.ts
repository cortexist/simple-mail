/**
 * settingsStore.svelte.ts — Phase 4 settings bridge.
 *
 * Fire-and-forget actions and event subscribers for everything the Settings
 * surface mutates: account CRUD/reorder, kv settings (theme/accent/locale),
 * storage quota, per-account offline download, mail/CalDAV/CardDAV server
 * configs, and the embedded DAV server (start/stop, user list).
 *
 * Mirrors the contactsStore pattern: subscribers route backend echoes back
 * into the page's local state; views never optimistically write.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from './safeListen';
import { incNetwork, decNetwork, type Activity } from '$lib/networkActivity.svelte';
import type { Account, AiConfigView, AiConfigInput, AccountAiFeatures } from '$lib/types';
import type { MailServerSettings, DavSettings } from '$lib/data/dataService';

// ── Event payloads ──────────────────────────────────────

export interface SettingsAccountUpsertedEvent {
  requestedId: string;
  account: Account;
  seq: number;
}

export interface SettingsAccountRemovedEvent {
  accountId: string;
  seq: number;
}

export interface SettingsAccountsReorderedEvent {
  orderedIds: string[];
  seq: number;
}

export interface SettingsKvChangedEvent {
  key: string;
  value: string;
  seq: number;
}

export interface SettingsStorageQuotaChangedEvent {
  quotaBytes: number | null;
  seq: number;
}

export interface SettingsOfflineDownloadChangedEvent {
  accountId: string;
  enabled: boolean;
  seq: number;
}

export interface SettingsMailServerSavedEvent {
  accountId: string;
  settings: MailServerSettings;
  seq: number;
}

export interface SettingsDavSavedEvent {
  accountId: string;
  settings: DavSettings;
  seq: number;
}

export interface SettingsDavServerChangedEvent {
  addr: string | null;
  seq: number;
}

export interface SettingsDavUsersChangedEvent {
  users: [string, string][];
  seq: number;
}

export interface SettingsAiConfigChangedEvent {
  enabled: boolean;
  baseUrl: string;
  hasKey: boolean;
  classifyModel: string;
  draftModel: string;
  maxTokens: number;
  temperature: number;
  seq: number;
}

export interface SettingsAiFeaturesChangedEvent {
  accountId: string;
  junkFilter: boolean;
  priorityDetect: boolean;
  autoDraft: boolean;
  externalAccess: boolean;
  seq: number;
}

// ── In-flight bookkeeping ───────────────────────────────

const inFlightCounts = new Map<string, number>();
const seqKey = (scope: string, id: string) => `${scope}:${id}`;

/** Network-activity tokens per op key, so an echo releases its own entry.
 *  Settings writes are local database work, so they count toward the
 *  indicator without a toast. */
const activityTokens = new Map<string, number[]>();

function incOp(key: string, activity: Activity = { kind: 'local' }) {
  inFlightCounts.set(key, (inFlightCounts.get(key) ?? 0) + 1);
  activityTokens.set(key, [...(activityTokens.get(key) ?? []), incNetwork(activity)]);
}

function decOp(key: string) {
  const next = (inFlightCounts.get(key) ?? 0) - 1;
  if (next <= 0) inFlightCounts.delete(key); else inFlightCounts.set(key, next);
  const list = activityTokens.get(key);
  const token = list?.shift();
  if (list && list.length === 0) activityTokens.delete(key);
  decNetwork(token);
}

// ── Subscriptions ───────────────────────────────────────

export async function subscribeSettingsAccountUpserted(
  handler: (ev: SettingsAccountUpsertedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsAccountUpsertedEvent>('settings:account-upserted', (event) => {
    const ev = event.payload;
    const k = seqKey('account', ev.requestedId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsAccountRemoved(
  handler: (ev: SettingsAccountRemovedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsAccountRemovedEvent>('settings:account-removed', (event) => {
    const ev = event.payload;
    const k = seqKey('account', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsAccountsReordered(
  handler: (ev: SettingsAccountsReorderedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsAccountsReorderedEvent>('settings:accounts-reordered', (event) => {
    const ev = event.payload;
    const k = seqKey('reorder', 'all');
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsKvChanged(
  handler: (ev: SettingsKvChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsKvChangedEvent>('settings:kv-changed', (event) => {
    const ev = event.payload;
    const k = seqKey('kv', ev.key);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsStorageQuotaChanged(
  handler: (ev: SettingsStorageQuotaChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsStorageQuotaChangedEvent>('settings:storage-quota-changed', (event) => {
    const ev = event.payload;
    const k = seqKey('quota', 'app');
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsOfflineDownloadChanged(
  handler: (ev: SettingsOfflineDownloadChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsOfflineDownloadChangedEvent>('settings:offline-download-changed', (event) => {
    const ev = event.payload;
    const k = seqKey('offline', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsMailServerSaved(
  handler: (ev: SettingsMailServerSavedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsMailServerSavedEvent>('settings:mail-server-saved', (event) => {
    const ev = event.payload;
    const k = seqKey('mail', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsCalDavSaved(
  handler: (ev: SettingsDavSavedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsDavSavedEvent>('settings:caldav-saved', (event) => {
    const ev = event.payload;
    const k = seqKey('caldav', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsCardDavSaved(
  handler: (ev: SettingsDavSavedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsDavSavedEvent>('settings:carddav-saved', (event) => {
    const ev = event.payload;
    const k = seqKey('carddav', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsDavServerChanged(
  handler: (ev: SettingsDavServerChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsDavServerChangedEvent>('settings:dav-server-changed', (event) => {
    const ev = event.payload;
    const k = seqKey('davserver', 'app');
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsDavUsersChanged(
  handler: (ev: SettingsDavUsersChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsDavUsersChangedEvent>('settings:dav-users-changed', (event) => {
    const ev = event.payload;
    const k = seqKey('davusers', 'app');
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsAiConfigChanged(
  handler: (ev: SettingsAiConfigChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsAiConfigChangedEvent>('settings:ai-config-changed', (event) => {
    const ev = event.payload;
    const k = seqKey('aiconfig', 'app');
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeSettingsAiFeaturesChanged(
  handler: (ev: SettingsAiFeaturesChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<SettingsAiFeaturesChangedEvent>('settings:ai-features-changed', (event) => {
    const ev = event.payload;
    const k = seqKey('aifeatures', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

// ── Actions (fire-and-forget) ───────────────────────────

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function fireUpdateAccount(account: Account): void {
  if (!inTauri()) return;
  const k = seqKey('account', account.id);
  incOp(k);
  invoke('update_account', {
    id: account.id,
    name: account.name,
    email: account.email,
    initials: account.initials,
    color: account.color,
    avatarUrl: account.avatarUrl ?? null,
    alias: account.alias ?? null,
  }).catch((err) => {
    console.error('update_account rejected:', err);
    decOp(k);
  });
}

export function fireAddAccount(data: {
  id: string;
  name: string;
  email: string;
  initials: string;
  color: string;
  avatarUrl?: string;
  alias?: string;
}): void {
  if (!inTauri()) return;
  const k = seqKey('account', data.id);
  incOp(k);
  invoke('add_account', {
    id: data.id,
    name: data.name,
    email: data.email,
    initials: data.initials,
    color: data.color,
    avatarUrl: data.avatarUrl ?? null,
    alias: data.alias ?? null,
  }).catch((err) => {
    console.error('add_account rejected:', err);
    decOp(k);
  });
}

export function fireDeleteAccount(id: string): void {
  if (!inTauri()) return;
  const k = seqKey('account', id);
  incOp(k);
  invoke('delete_account', { id }).catch((err) => {
    console.error('delete_account rejected:', err);
    decOp(k);
  });
}

/**
 * Turn an account's mailbox filter on or off.
 *
 * The backend echoes `settings:account-upserted` for *every* account on the
 * login, not just this one — flipping one mailbox changes which messages the
 * others show — so the page re-renders all the affected views from the echoes.
 */
export function fireSetAccountIsMailbox(accountId: string, isMailbox: boolean): void {
  if (!inTauri()) return;
  const k = seqKey('account', accountId);
  incOp(k);
  invoke('set_account_is_mailbox', { accountId, isMailbox }).catch((err) => {
    console.error('set_account_is_mailbox rejected:', err);
    decOp(k);
  });
}

/** A CalDAV/CardDAV collection on an account's login, and whether it's scoped to it. */
export interface AssignableCollection {
  url: string;
  displayName: string;
  assigned: boolean;
}

/** Every collection on the account's login — you can't assign what you can't see. */
export async function getAccountCollections(
  accountId: string,
  kind: 'caldav' | 'carddav',
): Promise<AssignableCollection[]> {
  if (!inTauri()) return [];
  try {
    return (await invoke('get_account_collections', { accountId, kind })) as AssignableCollection[];
  } catch (err) {
    console.error('get_account_collections rejected:', err);
    return [];
  }
}

/** Scope a mailbox to a collection, or unscope it. */
export function fireSetAccountCollection(
  accountId: string,
  kind: 'caldav' | 'carddav',
  collectionUrl: string,
  assigned: boolean,
): void {
  if (!inTauri()) return;
  const k = seqKey('account', accountId);
  incOp(k);
  invoke('set_account_collection', { accountId, kind, collectionUrl, assigned }).catch((err) => {
    console.error('set_account_collection rejected:', err);
    decOp(k);
  });
}

/** Addresses of the other accounts sharing this account's login. */
export async function getLoginSiblings(accountId: string): Promise<string[]> {
  if (!inTauri()) return [];
  try {
    return (await invoke('get_login_siblings', { accountId })) as string[];
  } catch (err) {
    console.error('get_login_siblings rejected:', err);
    return [];
  }
}

export function fireSetAccountPositions(orderedIds: string[]): void {
  if (!inTauri()) return;
  const k = seqKey('reorder', 'all');
  incOp(k);
  invoke('set_account_positions', { orderedIds }).catch((err) => {
    console.error('set_account_positions rejected:', err);
    decOp(k);
  });
}

export function fireSetSetting(key: string, value: string): void {
  if (!inTauri()) return;
  const k = seqKey('kv', key);
  incOp(k);
  invoke('set_setting', { key, value }).catch((err) => {
    console.error('set_setting rejected:', err);
    decOp(k);
  });
}

/**
 * Set the app-wide storage quota. Pass 0 to clear.
 * Returns the promise so callers can surface backend validation errors
 * (e.g. "Storage limit must be at least 100 MB."), in addition to the echo.
 */
export function fireSetStorageQuota(bytes: number): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  const k = seqKey('quota', 'app');
  incOp(k);
  return invoke<void>('set_storage_quota', { bytes }).catch((err) => {
    decOp(k);
    throw err;
  });
}

export function fireSetAccountOfflineDownload(accountId: string, enabled: boolean): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  const k = seqKey('offline', accountId);
  incOp(k);
  return invoke<void>('set_account_offline_download', { accountId, enabled }).catch((err) => {
    decOp(k);
    throw err;
  });
}

/**
 * Persisting credentials can fail outright — the password is encrypted with a
 * master key held in the OS secret store, so a machine with no Secret Service
 * provider rejects the write. Returning the promise lets the caller report that
 * instead of flashing "Saved" over a save the backend refused.
 */
export function fireSaveMailSettings(accountId: string, settings: MailServerSettings): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  const k = seqKey('mail', accountId);
  incOp(k);
  return invoke<void>('save_mail_settings', { accountId, settings }).catch((err) => {
    console.error('save_mail_settings rejected:', err);
    decOp(k);
    throw err;
  });
}

export function fireSaveCalDavSettings(accountId: string, settings: DavSettings): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  const k = seqKey('caldav', accountId);
  incOp(k);
  return invoke<void>('save_caldav_settings', { accountId, settings }).catch((err) => {
    console.error('save_caldav_settings rejected:', err);
    decOp(k);
    throw err;
  });
}

export function fireSaveCardDavSettings(accountId: string, settings: DavSettings): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  const k = seqKey('carddav', accountId);
  incOp(k);
  return invoke<void>('save_carddav_settings', { accountId, settings }).catch((err) => {
    console.error('save_carddav_settings rejected:', err);
    decOp(k);
    throw err;
  });
}

/**
 * Start the embedded DAV server. Returns the promise so the caller can
 * surface the bind error directly; on success, the dav-server-changed echo
 * carries the bound address.
 */
export function fireStartDavServer(bindAddr: string): Promise<string> {
  if (!inTauri()) return Promise.resolve('');
  const k = seqKey('davserver', 'app');
  incOp(k);
  return invoke<string>('start_dav_server', { bindAddr }).catch((err) => {
    decOp(k);
    throw err;
  });
}

export function fireStopDavServer(): void {
  if (!inTauri()) return;
  const k = seqKey('davserver', 'app');
  incOp(k);
  invoke('stop_dav_server').catch((err) => {
    console.error('stop_dav_server rejected:', err);
    decOp(k);
  });
}

export function fireAddDavUser(email: string, password: string, accountId: string): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  const k = seqKey('davusers', 'app');
  incOp(k);
  return invoke<void>('add_dav_user', { email, password, accountId }).catch((err) => {
    decOp(k);
    throw err;
  });
}

export function fireRemoveDavUser(email: string): void {
  if (!inTauri()) return;
  const k = seqKey('davusers', 'app');
  incOp(k);
  invoke('remove_dav_user', { email }).catch((err) => {
    console.error('remove_dav_user rejected:', err);
    decOp(k);
  });
}

// ── AI config + per-account AI features ─────────────────

export async function getAiConfig(): Promise<AiConfigView | null> {
  if (!inTauri()) return null;
  return invoke<AiConfigView>('get_ai_config');
}

/** Save the global AI config. The caller awaits for success/error feedback;
 * the ai-config-changed echo is emitted for any other listeners. Not tied to
 * the network in-flight counter (a settings save, not a sync). */
export function fireSetAiConfig(config: AiConfigInput): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  return invoke<void>('set_ai_config', { config });
}

/** Live connectivity test against the configured endpoint. Returns the
 * model's reply text. Throws on failure so the UI can surface it. */
export function fireTestAiConfig(config: AiConfigInput): Promise<string> {
  if (!inTauri()) return Promise.resolve('');
  return invoke<string>('test_ai_config', { config });
}

export async function getAccountAiFeatures(accountId: string): Promise<AccountAiFeatures | null> {
  if (!inTauri()) return null;
  return invoke<AccountAiFeatures>('get_account_ai_features', { accountId });
}

export function fireSetAccountAiFeatures(accountId: string, features: AccountAiFeatures): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  return invoke<void>('set_account_ai_features', { accountId, features });
}

// ── MCP agent server + app keys ─────────────────────────

export interface AgentKeyInfo {
  id: string;
  label: string;
  createdAt: string;
  lastUsedAt: string | null;
}

/** Bound address of the MCP agent server, or null when stopped. */
export async function getAgentServerStatus(): Promise<string | null> {
  if (!inTauri()) return null;
  return invoke<string | null>('get_agent_server_status');
}

/** Start the MCP agent server; resolves to the bound address. */
export function fireStartAgentServer(bindAddr: string): Promise<string> {
  if (!inTauri()) return Promise.resolve('');
  return invoke<string>('start_agent_server', { bindAddr });
}

export function fireStopAgentServer(): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  return invoke<void>('stop_agent_server');
}

/** Generate a new app key. Returns `{ id, key }`; the plaintext `key` is only
 * available here, once. */
export function generateAgentKey(label: string): Promise<{ id: string; key: string }> {
  if (!inTauri()) return Promise.resolve({ id: '', key: '' });
  return invoke<{ id: string; key: string }>('generate_agent_key', { label });
}

export async function listAgentKeys(): Promise<AgentKeyInfo[]> {
  if (!inTauri()) return [];
  return invoke<AgentKeyInfo[]>('list_agent_keys');
}

export function revokeAgentKey(id: string): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  return invoke<void>('revoke_agent_key', { id });
}
