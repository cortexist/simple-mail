/**
 * contactsStore.svelte.ts — Phase 3 contacts bridge.
 *
 * Fire-and-forget actions for save/delete/sync/favorite, plus list CRUD.
 * Subscribers route backend echoes back into the page's local state.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { incNetwork, decNetwork } from '$lib/networkActivity.svelte';
import type { FullContact, ContactList, ContactListMember } from '$lib/types';

export interface ContactUpsertedEvent {
  accountId: string;
  requestedId: string;
  contact: FullContact;
  seq: number;
}

export interface ContactRemovedEvent {
  accountId: string;
  contactId: string;
  seq: number;
}

export interface ContactFavoriteChangedEvent {
  contactId: string;
  isFavorite: boolean;
  seq: number;
}

export interface ContactsSyncCompletedEvent {
  accountId: string;
  ok: boolean;
  newCount: number;
  updatedCount: number;
  deletedCount: number;
  error: string | null;
  seq: number;
}

export interface ContactListUpsertedEvent {
  accountId: string;
  requestedId: string;
  list: ContactList;
  seq: number;
}

export interface ContactListRemovedEvent {
  listId: string;
  seq: number;
}

// ── In-flight bookkeeping ───────────────────────────────

const inFlightCounts = new Map<string, number>();
const seqKey = (scope: string, id: string) => `${scope}:${id}`;

function incOp(key: string) {
  inFlightCounts.set(key, (inFlightCounts.get(key) ?? 0) + 1);
  incNetwork();
}

function decOp(key: string) {
  const next = (inFlightCounts.get(key) ?? 0) - 1;
  if (next <= 0) inFlightCounts.delete(key); else inFlightCounts.set(key, next);
  decNetwork();
}

// ── Subscriptions ───────────────────────────────────────

export type ContactUpsertedHandler = (ev: ContactUpsertedEvent) => void;
export type ContactRemovedHandler = (ev: ContactRemovedEvent) => void;
export type ContactFavoriteHandler = (ev: ContactFavoriteChangedEvent) => void;
export type ContactsSyncHandler = (ev: ContactsSyncCompletedEvent) => void;
export type ContactListUpsertedHandler = (ev: ContactListUpsertedEvent) => void;
export type ContactListRemovedHandler = (ev: ContactListRemovedEvent) => void;

export async function subscribeContactUpserted(handler: ContactUpsertedHandler): Promise<UnlistenFn> {
  return listen<ContactUpsertedEvent>('contacts:contact-upserted', (event) => {
    const ev = event.payload;
    const k = seqKey('contact', ev.requestedId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeContactRemoved(handler: ContactRemovedHandler): Promise<UnlistenFn> {
  return listen<ContactRemovedEvent>('contacts:contact-removed', (event) => {
    const ev = event.payload;
    const k = seqKey('contact', ev.contactId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeContactFavoriteChanged(handler: ContactFavoriteHandler): Promise<UnlistenFn> {
  return listen<ContactFavoriteChangedEvent>('contacts:favorite-changed', (event) => handler(event.payload));
}

export async function subscribeContactsSyncCompleted(handler: ContactsSyncHandler): Promise<UnlistenFn> {
  return listen<ContactsSyncCompletedEvent>('contacts:sync-completed', (event) => {
    const ev = event.payload;
    const k = seqKey('sync', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeContactListUpserted(handler: ContactListUpsertedHandler): Promise<UnlistenFn> {
  return listen<ContactListUpsertedEvent>('contacts:list-upserted', (event) => {
    const ev = event.payload;
    const k = seqKey('list', ev.requestedId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeContactListRemoved(handler: ContactListRemovedHandler): Promise<UnlistenFn> {
  return listen<ContactListRemovedEvent>('contacts:list-removed', (event) => {
    const ev = event.payload;
    const k = seqKey('list', ev.listId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

// ── Actions (fire-and-forget) ───────────────────────────

export function fireSaveContactEntry(accountId: string, contact: FullContact): void {
  const k = seqKey('contact', contact.id);
  incOp(k);
  invoke('save_contact_entry', {
    accountId,
    contactId: contact.id,
    name: contact.name,
    firstName: contact.firstName ?? null,
    lastName: contact.lastName ?? null,
    middleName: contact.middleName ?? null,
    prefix: contact.prefix ?? null,
    suffix: contact.suffix ?? null,
    emails: contact.emails,
    phones: contact.phones,
    addresses: contact.addresses,
    jobTitle: contact.jobTitle ?? null,
    department: contact.department ?? null,
    organization: contact.organization ?? null,
    birthday: contact.birthday ?? null,
    notes: contact.notes ?? null,
    isFavorite: contact.isFavorite,
    photoUrl: contact.photoUrl ?? null,
  }).catch((err) => {
    console.error('save_contact_entry rejected:', err);
    decOp(k);
  });
}

export function fireDeleteContactEntry(accountId: string, contactId: string): void {
  const k = seqKey('contact', contactId);
  incOp(k);
  invoke('delete_contact_entry', { accountId, contactId }).catch((err) => {
    console.error('delete_contact_entry rejected:', err);
    decOp(k);
  });
}

/** Local-only DB write. Backend emits the favorite-changed echo. */
export function fireToggleContactFavorite(contactId: string, isFavorite: boolean): void {
  invoke('toggle_contact_favorite', { contactId, isFavorite })
    .catch((err) => console.error('toggle_contact_favorite rejected:', err));
}

export function fireSyncContacts(accountId: string): void {
  const k = seqKey('sync', accountId);
  incOp(k);
  invoke('sync_contacts', { accountId }).catch((err) => {
    console.error('sync_contacts rejected:', err);
    decOp(k);
  });
}

export function fireSaveContactList(
  accountId: string,
  id: string,
  name: string,
  members: ContactListMember[],
): void {
  const k = seqKey('list', id);
  incOp(k);
  invoke('save_contact_list', { accountId, id, name, members })
    .catch((err) => {
      console.error('save_contact_list rejected:', err);
      decOp(k);
    });
}

export function fireDeleteContactList(id: string): void {
  const k = seqKey('list', id);
  incOp(k);
  invoke('delete_contact_list', { id })
    .catch((err) => {
      console.error('delete_contact_list rejected:', err);
      decOp(k);
    });
}
