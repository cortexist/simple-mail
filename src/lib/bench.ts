/**
 * Reading-path memory bench.
 *
 * Walks every message through the reading pane, the way a person reading
 * their whole mailbox would, and logs the process memory after each one. The
 * point is to see whether memory tracks what was rendered — and whether it
 * comes back down between rounds — under reading alone, with no sending.
 *
 * Selection is driven directly rather than through `handleSelectEmail`, which
 * would mark every unread message read and fire an IMAP round trip for each.
 * The bench must not touch the mailbox; it only reads.
 *
 * Runs only when the backend says a bench was requested (a debug build with
 * `SIMPLE_MAIL_BENCH=1`). `scripts/bench-reading.sh` sets that up and turns
 * the log into a report. Also reachable as `__mailBench` from the console.
 */

import { invoke } from '@tauri-apps/api/core';
import type { Email } from '$lib/types';

/** What the page hands the runner. Getters, because the page's state is
 *  reassigned wholesale on every echo. */
export interface BenchHost {
  accounts(): Array<{ id: string }>;
  activeAccountId(): string;
  selectAccount(id: string): void | Promise<void>;
  emails(): Email[];
  select(id: string): void;
  fetchBody(accountId: string, id: string): void;
}

interface BenchConfig {
  rounds: number;
  settleMs: number;
  roundIdleMs: number;
  timeoutMs: number;
  limit: number;
  logPath: string;
}

interface BenchSample {
  tsMs: number;
  mainAnonKb: number;
  mainRssKb: number;
  webAnonKb: number;
  webRssKb: number;
  netAnonKb: number;
  webProcessCount: number;
}

interface RenderTraceEntry { load: number | null; settled: number | null }
interface RenderTraceConsole {
  readonly enabled: boolean;
  readonly entries: RenderTraceEntry[];
  enable(): void;
  disable(): void;
}

const COLUMNS = [
  'ts_ms', 'phase', 'round', 'account', 'idx', 'email_id', 'status',
  'body_bytes', 'img_count', 'anim_img_count', 'render_load_ms', 'render_settled_ms',
  'main_anon_kb', 'main_rss_kb', 'web_anon_kb', 'web_rss_kb', 'net_anon_kb', 'web_procs',
];

let running = false;
let stopRequested = false;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

async function until(cond: () => boolean, timeoutMs: number, everyMs = 25): Promise<boolean> {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    if (cond()) return true;
    await sleep(everyMs);
  }
  return cond();
}

function csv(fields: Array<string | number | null | undefined>): string {
  return fields.map((f) => {
    if (f === null || f === undefined) return '';
    const s = String(f);
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  }).join(',');
}

async function sampleRow(
  cfg: BenchConfig,
  phase: string,
  fields: { round?: number; account?: string; idx?: number; emailId?: string; status?: string;
            bodyBytes?: number; imgCount?: number; animImgCount?: number; load?: number | null; settled?: number | null },
) {
  const s = await invoke<BenchSample>('bench_sample');
  const line = csv([
    s.tsMs, phase, fields.round, fields.account, fields.idx, fields.emailId, fields.status,
    fields.bodyBytes, fields.imgCount, fields.animImgCount,
    fields.load == null ? null : Math.round(fields.load),
    fields.settled == null ? null : Math.round(fields.settled),
    s.mainAnonKb, s.mainRssKb, s.webAnonKb, s.webRssKb, s.netAnonKb, s.webProcessCount,
  ]);
  await invoke('bench_append', { path: cfg.logPath, line });
  return s;
}

/** Start the bench if the backend was launched with one configured. Safe to
 *  call unconditionally; it is a no-op otherwise. */
export async function startBenchIfConfigured(host: BenchHost): Promise<void> {
  let cfg: BenchConfig | null;
  try {
    cfg = await invoke<BenchConfig | null>('bench_config');
  } catch {
    return; // command absent — not a build that can bench
  }
  if (!cfg) return;
  void run(host, cfg);
}

async function run(host: BenchHost, cfg: BenchConfig) {
  if (running) return;
  running = true;
  stopRequested = false;

  const trace = (globalThis as unknown as { __mailRenderTrace?: RenderTraceConsole }).__mailRenderTrace;
  const traceWasEnabled = trace?.enabled ?? false;
  trace?.enable();

  const log = (msg: string) => console.log(`[mail-bench] ${msg}`);
  try {
    await invoke('bench_append', { path: cfg.logPath, line: COLUMNS.join(',') });
    log(`writing ${cfg.logPath}; ${cfg.rounds} round(s), settle ${cfg.settleMs} ms`);

    // Let the launch sync and worker resume settle before the baseline.
    await sleep(8000);
    await sampleRow(cfg, 'baseline', {});

    for (let round = 1; round <= cfg.rounds && !stopRequested; round += 1) {
      for (const account of host.accounts()) {
        if (stopRequested) break;
        if (host.activeAccountId() !== account.id) {
          await host.selectAccount(account.id);
          await sleep(200);
        }
        // Drafts open the composer rather than the reading pane; a sending
        // draft is hidden from the list. Neither is reading.
        let ids = host.emails().filter((e) => e.folder !== 'drafts' && !e.isSending).map((e) => e.id);
        if (cfg.limit > 0) ids = ids.slice(0, cfg.limit);
        log(`round ${round} account ${account.id}: ${ids.length} messages`);

        for (let idx = 0; idx < ids.length && !stopRequested; idx += 1) {
          const id = ids[idx];
          const before = host.emails().find((e) => e.id === id);
          if (!before) continue; // removed by a sync mid-round

          // The trace buffer is a ring of 50, so its length stops moving once
          // full; a new render is detected by the newest entry being a
          // different object, not by the count.
          const newest = () => trace?.entries[trace.entries.length - 1] ?? null;
          const lastBefore = newest();
          host.select(id);
          if (!before.body) host.fetchBody(account.id, id);

          const gotBody = await until(() => !!host.emails().find((e) => e.id === id)?.body, cfg.timeoutMs);
          // The pane publishes a trace from its settle poll only when the
          // document's body exists right after the write; when the parser
          // finishes asynchronously the trace is published at the next
          // message's teardown instead. So a missing trace does not mean the
          // message did not render — it is shown either way. Wait briefly for
          // timings, then settle regardless.
          const rendered = gotBody && (!trace || await until(() => newest() !== lastBefore, 3000));
          const status = !gotBody ? 'body-timeout' : rendered ? 'ok' : 'no-trace';

          await sleep(cfg.settleMs);
          const after = host.emails().find((e) => e.id === id);
          const body = after?.body ?? '';
          const last = rendered ? newest() : null;
          await sampleRow(cfg, 'message', {
            round, account: account.id, idx, emailId: id, status,
            bodyBytes: body.length,
            imgCount: (body.match(/<img/gi) || []).length,
            animImgCount: (body.match(/<img[^>]+src\s*=\s*["']?[^"'\s>]*\.(gif|webp|apng)(\?|["'\s>])/gi) || []).length,
            load: last?.load ?? null, settled: last?.settled ?? null,
          });
        }
      }

      // Idle with nothing selected, then sample: this is where a collector
      // that waits for quiet gets to run.
      host.select('');
      await sleep(cfg.roundIdleMs);
      await sampleRow(cfg, 'round-idle', { round });
      log(`round ${round} done`);
    }

    await sampleRow(cfg, stopRequested ? 'stopped' : 'done', {});
    log(stopRequested ? 'stopped' : 'done');
  } catch (error) {
    console.error('[mail-bench] failed:', error);
    try { await invoke('bench_append', { path: cfg.logPath, line: csv([Date.now(), 'error', String(error)]) }); } catch { /* nothing left to do */ }
  } finally {
    if (!traceWasEnabled) trace?.disable();
    running = false;
  }
}

/** Console handle: `__mailBench.stop()` ends a run after the current message. */
export function installBenchConsole(host: BenchHost) {
  Object.defineProperty(globalThis, '__mailBench', {
    configurable: true,
    value: {
      get running() { return running; },
      stop() { stopRequested = true; },
      start(overrides: Partial<BenchConfig> = {}) {
        void invoke<BenchConfig | null>('bench_config').then((cfg) => {
          if (!cfg) { console.warn('[mail-bench] not enabled: debug build + SIMPLE_MAIL_BENCH=1 required'); return; }
          void run(host, { ...cfg, ...overrides });
        });
      },
    },
  });
}
