/**
 * Global network-activity counter driving the titlebar indicator.
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
 * increments that outlive any legitimate operation, FIFO (the oldest pending
 * entry is the one presumed dead). A late echo after expiry is harmless —
 * `dec` floors at zero and the stores guard their own decrements by key.
 */
export const networkActivity = $state({ count: 0 });

const STALE_MS = 3 * 60 * 1000;
const SWEEP_MS = 30 * 1000;

/** Start timestamps of pending operations, oldest first. */
const pending: number[] = [];
let sweeper: ReturnType<typeof setInterval> | null = null;

function stopSweeperIfIdle() {
  if (pending.length === 0 && sweeper !== null) {
    clearInterval(sweeper);
    sweeper = null;
  }
}

function sweep() {
  const cutoff = Date.now() - STALE_MS;
  let expired = 0;
  while (pending.length > 0 && pending[0] < cutoff) {
    pending.shift();
    expired++;
  }
  if (expired > 0) {
    console.warn(`networkActivity: expired ${expired} stale operation(s) — a backend echo never arrived`);
    networkActivity.count = Math.max(0, networkActivity.count - expired);
  }
  stopSweeperIfIdle();
}

export function incNetwork() {
  networkActivity.count++;
  pending.push(Date.now());
  if (sweeper === null) sweeper = setInterval(sweep, SWEEP_MS);
}

export function decNetwork() {
  if (networkActivity.count > 0) networkActivity.count--;
  pending.shift(); // drop the oldest pending marker
  stopSweeperIfIdle();
}
