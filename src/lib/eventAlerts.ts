/**
 * eventAlerts.ts — Calendar event notification scheduler.
 *
 * Maintains a sorted FIFO queue of upcoming event alerts. When the next alert
 * time arrives, fires an OS notification via the Web Notification API. The
 * queue is rebuilt whenever events change (add/edit/delete/sync).
 */

import type { CalendarEvent } from '$lib/types';

interface AlertEntry {
  eventId: string;       // base event id
  instanceKey: string;   // unique key: eventId or eventId + instance date
  title: string;
  location?: string;
  alertTime: number;     // ms timestamp when notification should fire
  eventStart: Date;
}

let queue: AlertEntry[] = [];
let timerId: ReturnType<typeof setTimeout> | null = null;
let firedKeys = new Set<string>();  // avoid re-firing after rebuild

/** Request notification permission (call once on app startup). */
export function requestNotificationPermission() {
  if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
    Notification.requestPermission();
  }
}

/** Rebuild the alert queue from all calendar events across all accounts. */
export function rebuildAlertQueue(allEvents: CalendarEvent[]) {
  const now = Date.now();
  const horizon = now + 7 * 24 * 60 * 60 * 1000; // look ahead 7 days
  const entries: AlertEntry[] = [];

  for (const event of allEvents) {
    if (event.alertMinutes == null) continue;

    if (event.recurrence) {
      // Generate upcoming instances within the horizon
      const instances = expandRecurringInstances(event, now, horizon);
      for (const inst of instances) {
        const alertTime = inst.start.getTime() - event.alertMinutes * 60000;
        if (alertTime <= now) continue;
        const dateStr = formatDateKey(inst.start);
        const key = `${event.id}_${dateStr}`;
        if (firedKeys.has(key)) continue;
        entries.push({
          eventId: event.id,
          instanceKey: key,
          title: event.title,
          location: event.location,
          alertTime,
          eventStart: inst.start,
        });
      }
    } else {
      const alertTime = event.start.getTime() - event.alertMinutes * 60000;
      if (alertTime <= now) continue;
      if (alertTime > horizon) continue;
      const key = event.id;
      if (firedKeys.has(key)) continue;
      entries.push({
        eventId: event.id,
        instanceKey: key,
        title: event.title,
        location: event.location,
        alertTime,
        eventStart: event.start,
      });
    }
  }

  // Sort by alert time (earliest first)
  entries.sort((a, b) => a.alertTime - b.alertTime);
  queue = entries;

  scheduleNext();
}

/** Stop the scheduler and clear the queue. */
export function clearAlertQueue() {
  if (timerId != null) {
    clearTimeout(timerId);
    timerId = null;
  }
  queue = [];
  firedKeys.clear();
}

// ── Internal ────────────────────────────────────────────

function scheduleNext() {
  if (timerId != null) {
    clearTimeout(timerId);
    timerId = null;
  }

  if (queue.length === 0) return;

  const next = queue[0];
  const delay = Math.max(0, next.alertTime - Date.now());

  timerId = setTimeout(() => {
    timerId = null;
    fireAlert(next);
    // Remove fired entry and schedule next
    queue.shift();
    firedKeys.add(next.instanceKey);
    scheduleNext();
  }, delay);
}

function fireAlert(entry: AlertEntry) {
  if (typeof Notification === 'undefined') return;
  if (Notification.permission !== 'granted') return;

  const timeStr = entry.eventStart.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  const body = entry.location
    ? `${timeStr} — ${entry.location}`
    : timeStr;

  new Notification(entry.title, { body, icon: undefined });
}

function formatDateKey(d: Date): string {
  return `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}T${String(d.getHours()).padStart(2, '0')}${String(d.getMinutes()).padStart(2, '0')}${String(d.getSeconds()).padStart(2, '0')}`;
}

/** Expand recurring event into concrete instances within [startMs, endMs]. */
function expandRecurringInstances(event: CalendarEvent, startMs: number, endMs: number): { start: Date; end: Date }[] {
  const rec = event.recurrence;
  if (!rec) return [];

  const base = event.start;
  const baseDate = new Date(base.getFullYear(), base.getMonth(), base.getDate());
  const duration = event.end.getTime() - event.start.getTime();
  const results: { start: Date; end: Date }[] = [];

  // Determine the end boundary
  let recEnd = endMs;
  if (rec.endDate) {
    const [ey, em, ed] = rec.endDate.split('-').map(Number);
    const endDateMs = new Date(ey, em - 1, ed, 23, 59, 59).getTime();
    if (endDateMs < recEnd) recEnd = endDateMs;
  }

  // Include alert lead time in the scan window
  const alertLead = (event.alertMinutes ?? 0) * 60000;
  const scanStart = new Date(startMs - alertLead);
  const scanEnd = new Date(recEnd);

  // Iterate through potential dates
  const cursor = new Date(baseDate);
  const maxIterations = 400; // safety limit
  let iterations = 0;

  while (cursor.getTime() <= scanEnd.getTime() && iterations < maxIterations) {
    iterations++;
    const curDate = new Date(cursor);

    if (curDate.getTime() >= baseDate.getTime()) {
      // Check exception dates
      const dateStr = `${curDate.getFullYear()}-${String(curDate.getMonth() + 1).padStart(2, '0')}-${String(curDate.getDate()).padStart(2, '0')}`;
      const excluded = rec.exdates?.includes(dateStr);

      if (!excluded) {
        const instStart = new Date(curDate);
        instStart.setHours(base.getHours(), base.getMinutes(), base.getSeconds(), 0);
        const alertTime = instStart.getTime() - alertLead;

        if (alertTime > startMs && instStart.getTime() <= recEnd) {
          results.push({ start: instStart, end: new Date(instStart.getTime() + duration) });
        }
      }
    }

    // Advance cursor
    switch (rec.freq) {
      case 'daily':
        cursor.setDate(cursor.getDate() + rec.interval);
        break;
      case 'weekly':
        cursor.setDate(cursor.getDate() + rec.interval * 7);
        break;
      case 'monthly':
        cursor.setMonth(cursor.getMonth() + rec.interval);
        break;
      case 'yearly':
        cursor.setFullYear(cursor.getFullYear() + rec.interval);
        break;
    }
  }

  return results;
}
