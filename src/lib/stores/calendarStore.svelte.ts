/**
 * calendarStore.svelte.ts — Phase 2 calendar bridge.
 *
 * Same pattern as `mailStore.svelte.ts`: fire-and-forget actions for
 * `save_calendar_event`, `delete_calendar_event`, `sync_calendars`; subscribers
 * for `calendar:event-upserted`, `calendar:event-removed`, `calendar:sync-completed`.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { incNetwork, decNetwork } from '$lib/networkActivity.svelte';
import type { CalendarEvent } from '$lib/types';

export interface CalendarEventUpsertedEvent {
  accountId: string;
  requestedId: string;
  event: CalendarEvent;
  seq: number;
}

export interface CalendarEventRemovedEvent {
  accountId: string;
  eventId: string;
  seq: number;
}

export interface CalendarSyncCompletedEvent {
  accountId: string;
  ok: boolean;
  newCount: number;
  updatedCount: number;
  deletedCount: number;
  error: string | null;
  seq: number;
}

// ── In-flight bookkeeping ───────────────────────────────
// Per-operation key. Body-fetched / preview-style ops are scoped by entity
// id; sync is scoped by account id.

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

export type EventUpsertedHandler = (ev: CalendarEventUpsertedEvent) => void;
export type EventRemovedHandler = (ev: CalendarEventRemovedEvent) => void;
export type CalSyncCompletedHandler = (ev: CalendarSyncCompletedEvent) => void;

export async function subscribeEventUpserted(handler: EventUpsertedHandler): Promise<UnlistenFn> {
  return listen<CalendarEventUpsertedEvent>('calendar:event-upserted', (event) => {
    const ev = event.payload;
    const k = seqKey('event', ev.requestedId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeEventRemoved(handler: EventRemovedHandler): Promise<UnlistenFn> {
  return listen<CalendarEventRemovedEvent>('calendar:event-removed', (event) => {
    const ev = event.payload;
    const k = seqKey('event', ev.eventId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

export async function subscribeCalendarSyncCompleted(handler: CalSyncCompletedHandler): Promise<UnlistenFn> {
  return listen<CalendarSyncCompletedEvent>('calendar:sync-completed', (event) => {
    const ev = event.payload;
    const k = seqKey('sync', ev.accountId);
    if (inFlightCounts.has(k)) decOp(k);
    handler(ev);
  });
}

// ── Actions (fire-and-forget) ───────────────────────────

export interface SaveCalendarEventArgs {
  accountId: string;
  eventId: string;
  title: string;
  start: string;
  end: string;
  location?: string;
  description?: string;
  isAllDay: boolean;
  calendarId: string;
  attendees: { name: string; email: string; initials: string; color: string; role: string }[];
  recurrence?: { freq: string; interval: number; endDate?: string; byDay?: string; exdates?: string[] };
  isOnlineMeeting: boolean;
  meetingUrl?: string;
  alertMinutes?: number;
}

export function fireSaveCalendarEvent(args: SaveCalendarEventArgs): void {
  const k = seqKey('event', args.eventId);
  incOp(k);
  invoke('save_calendar_event', {
    accountId: args.accountId,
    eventId: args.eventId,
    title: args.title,
    start: args.start,
    end: args.end,
    location: args.location ?? null,
    description: args.description ?? null,
    isAllDay: args.isAllDay,
    calendarId: args.calendarId,
    attendees: args.attendees,
    recurrence: args.recurrence ?? null,
    isOnlineMeeting: args.isOnlineMeeting,
    meetingUrl: args.meetingUrl ?? null,
    alertMinutes: args.alertMinutes ?? null,
  }).catch((err) => {
    console.error('save_calendar_event rejected:', err);
    decOp(k);
  });
}

export function fireDeleteCalendarEvent(accountId: string, eventId: string): void {
  const k = seqKey('event', eventId);
  incOp(k);
  invoke('delete_calendar_event', { accountId, eventId }).catch((err) => {
    console.error('delete_calendar_event rejected:', err);
    decOp(k);
  });
}

export function fireSyncCalendars(accountId: string): void {
  const k = seqKey('sync', accountId);
  incOp(k);
  invoke('sync_calendars', { accountId }).catch((err) => {
    console.error('sync_calendars rejected:', err);
    decOp(k);
  });
}
