/**
 * Global network-activity counter driving the titlebar indicator, and the
 * record of what each pending operation is, so the indicator can be explained.
 *
 * Increments when a fire-and-forget operation starts; decrements when its
 * backend echo arrives. That contract has a failure mode: if the echo never
 * comes — a missed emit, a killed session, an operation that dies past every
 * retry — the counter is stuck high and the indicator spins forever with no
 * one to decrement it. (This is exactly the "indicator active forever" the
 * user hit after a send whose post-send sync hung.)
 *
 * Backend operations are all bounded now (IMAP connect 30s, SMTP 60s), so a
 * genuine op resolves — success or failure — well within a few minutes. This
 * watchdog is the backstop for the echo going missing regardless: it expires
 * increments that outlive any legitimate operation, oldest first. A late echo
 * after expiry is harmless — `dec` floors at zero and the stores guard their
 * own decrements by key.
 *
 * Each increment records what the operation is — which server it talks to,
 * in which direction, for which account — and hands back a token. Decrement
 * with the token and exactly that entry is released; without one, the oldest
 * is, which keeps the count right but can mislabel what remains. Every caller
 * in the app passes its token.
 */

/** What an operation is doing, from the user's point of view. `local` is a
 *  database write that never leaves the machine; it counts toward the
 *  indicator but is not worth a toast. */
export type ActivityKind = 'imap-fetch' | 'imap-update' | 'smtp' | 'caldav' | 'carddav' | 'ai' | 'local';

export interface Activity {
  kind: ActivityKind;
  accountId?: string;
}

export interface PendingActivity extends Activity {
  token: number;
  startedAt: number;
}

export const networkActivity = $state({
  count: 0,
  /** Pending operations, oldest first. A fresh array on every change. */
  active: [] as PendingActivity[],
});

/** Completed operations with how long they took, newest last, for answering
 *  "why did the indicator light?" from the console: `__mailActivity.recent()`.
 *  Not reactive; nothing renders from it. */
export interface CompletedActivity extends Activity {
  ms: number;
  finishedAt: number;
  expired: boolean;
}
const recent: CompletedActivity[] = [];
const RECENT_MAX = 50;

function remember(p: PendingActivity, expired: boolean) {
  const now = Date.now();
  recent.push({ kind: p.kind, accountId: p.accountId, ms: now - p.startedAt, finishedAt: now, expired });
  if (recent.length > RECENT_MAX) recent.shift();
}

let consoleInstalled = false;
/** Register `__mailActivity` on the global for a running build. */
export function installActivityConsole() {
  if (consoleInstalled) return;
  consoleInstalled = true;
  Object.defineProperty(globalThis, '__mailActivity', {
    configurable: true,
    value: {
      /** Pending now, with age in ms. */
      pending: () => pending.map((p) => ({ kind: p.kind, accountId: p.accountId, ms: Date.now() - p.startedAt })),
      /** The last 50 completed, newest last. */
      recent: () => recent.slice(),
      clear() { recent.length = 0; },
    },
  });
}

const STALE_MS = 3 * 60 * 1000;
const SWEEP_MS = 30 * 1000;

/** Pending operations, oldest first. */
const pending: PendingActivity[] = [];
let nextToken = 1;
let sweeper: ReturnType<typeof setInterval> | null = null;

function publish() {
  networkActivity.count = pending.length;
  networkActivity.active = pending.slice();
}

function stopSweeperIfIdle() {
  if (pending.length === 0 && sweeper !== null) {
    clearInterval(sweeper);
    sweeper = null;
  }
}

function sweep() {
  const cutoff = Date.now() - STALE_MS;
  let expired = 0;
  while (pending.length > 0 && pending[0].startedAt < cutoff) {
    remember(pending.shift()!, true);
    expired++;
  }
  if (expired > 0) {
    console.warn(`networkActivity: expired ${expired} stale operation(s) — a backend echo never arrived`);
    publish();
  }
  stopSweeperIfIdle();
}

/** Register a pending operation. Returns a token for `decNetwork`. */
export function incNetwork(activity: Activity = { kind: 'local' }): number {
  const token = nextToken++;
  pending.push({ ...activity, token, startedAt: Date.now() });
  publish();
  if (sweeper === null) sweeper = setInterval(sweep, SWEEP_MS);
  return token;
}

/** Release a pending operation — the one with `token`, or the oldest. */
export function decNetwork(token?: number) {
  if (pending.length === 0) return;
  const at = token === undefined ? -1 : pending.findIndex((p) => p.token === token);
  if (at === -1) {
    // Unknown or absent token: the entry may already have been swept. Fall
    // back to the oldest so the count still comes down.
    remember(pending.shift()!, false);
  } else {
    remember(pending.splice(at, 1)[0], false);
  }
  publish();
  stopSweeperIfIdle();
}
