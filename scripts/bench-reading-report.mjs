/**
 * bench-reading-report.mjs — turn a reading-bench CSV into a report.
 *
 *   node scripts/bench-reading-report.mjs [~/.cortexist/mail/bench-reading.csv]
 *
 * Answers, in order: did memory grow across rounds (a leak) or only within the
 * first (one-time cost)? did the idle pause after each round give any back?
 * and which messages moved it most — with their image counts, so an image-
 * decode explanation can be checked against the data rather than assumed.
 */

import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

const path = process.argv[2] ?? join(homedir(), '.cortexist/mail/bench-reading.csv');
const text = readFileSync(path, 'utf8');

// The runner writes a header at the start of every run; keep the last run.
const lines = text.split('\n').filter(Boolean);
const startAt = Math.max(0, lines.map((l) => l.startsWith('ts_ms,')).lastIndexOf(true));
const header = lines[startAt].split(',');
const rows = lines.slice(startAt + 1).map((l) => {
  const cells = l.split(',');
  const r = {};
  header.forEach((h, i) => { r[h] = cells[i] === '' || cells[i] === undefined ? null : cells[i]; });
  for (const k of ['round', 'idx', 'body_bytes', 'img_count', 'render_load_ms', 'render_settled_ms',
                   'main_anon_kb', 'main_rss_kb', 'web_anon_kb', 'web_rss_kb', 'net_anon_kb', 'web_procs']) {
    if (r[k] !== null) r[k] = Number(r[k]);
  }
  return r;
}).filter((r) => r.phase);

const mb = (kb) => (kb / 1024).toFixed(1).padStart(7) + ' MB';
const pct = (a, b) => (b === 0 ? '   n/a' : ((a / b - 1) * 100).toFixed(0).padStart(5) + '%');

const baseline = rows.find((r) => r.phase === 'baseline');
const messages = rows.filter((r) => r.phase === 'message');
const idles = rows.filter((r) => r.phase === 'round-idle');
const end = rows.find((r) => r.phase === 'done' || r.phase === 'stopped');
const errors = rows.filter((r) => r.phase === 'error');

console.log(`reading bench — ${path}`);
console.log(`${messages.length} message samples, ${idles.length} round(s) completed${end ? ` (${end.phase})` : ' (no end marker — run may have been cut short)'}`);
if (errors.length) console.log(`ERRORS: ${errors.length}`);
const bodyTimeouts = messages.filter((r) => r.status === 'body-timeout').length;
const renderTimeouts = messages.filter((r) => r.status === 'render-timeout' || r.status === 'no-trace').length;
if (bodyTimeouts) console.log(`${bodyTimeouts} message(s) never got a body (excluded)`);
if (renderTimeouts) console.log(`${renderTimeouts} message(s) gave no render signal (kept — they were shown and sampled after the wait)`);
console.log();

if (!baseline || messages.length === 0) {
  console.log('not enough data for a report');
  process.exit(0);
}

// ── Per round ────────────────────────────────────────────
console.log('web process anon (JS heap + decoded images + layout), per round:');
console.log('  round   first msg     peak         last msg     after idle   Δ round      Δ idle');
const rounds = [...new Set(messages.map((r) => r.round))].sort((a, b) => a - b);
let prevIdle = baseline;
for (const round of rounds) {
  const m = messages.filter((r) => r.round === round);
  const idle = idles.find((r) => r.round === round);
  const first = m[0], last = m[m.length - 1];
  const peak = m.reduce((p, r) => (r.web_anon_kb > p.web_anon_kb ? r : p), m[0]);
  const dRound = last.web_anon_kb - prevIdle.web_anon_kb;
  const dIdle = idle ? idle.web_anon_kb - last.web_anon_kb : null;
  console.log(
    `  ${String(round).padStart(5)}   ${mb(first.web_anon_kb)}  ${mb(peak.web_anon_kb)}  ${mb(last.web_anon_kb)}  ` +
    `${idle ? mb(idle.web_anon_kb) : '        n/a'}  ${mb(dRound)}  ${dIdle === null ? '        n/a' : mb(dIdle)}`,
  );
  if (idle) prevIdle = idle;
}
console.log();
const finalIdle = idles[idles.length - 1] ?? messages[messages.length - 1];
console.log(`baseline → final:  web ${mb(baseline.web_anon_kb)} → ${mb(finalIdle.web_anon_kb)}  (${pct(finalIdle.web_anon_kb, baseline.web_anon_kb)})`);
console.log(`                   app ${mb(baseline.main_anon_kb)} → ${mb(finalIdle.main_anon_kb)}  (${pct(finalIdle.main_anon_kb, baseline.main_anon_kb)})`);
if (rounds.length >= 2) {
  const r1 = idles.find((r) => r.round === rounds[0]);
  const rN = idles.find((r) => r.round === rounds[rounds.length - 1]);
  if (r1 && rN) {
    const perRound = (rN.web_anon_kb - r1.web_anon_kb) / (rounds.length - 1);
    console.log();
    console.log(`after round 1 → after round ${rounds.length}: ${mb(rN.web_anon_kb - r1.web_anon_kb)} total, ${mb(perRound)} per round`);
    console.log(perRound > 20 * 1024
      ? '  → memory keeps climbing on repeat reads of the SAME messages: that is retention, not one-time cost.'
      : perRound > 5 * 1024
        ? '  → modest growth on repeat rounds; worth a longer run to see if it plateaus.'
        : '  → repeat rounds are flat: round-1 growth is one-time cost (fonts, decoders, JIT), not a leak.');
  }
}
console.log();

// ── Attribution ──────────────────────────────────────────
// Growth per message = web anon after this message minus after the previous
// one in the same round. Noisy per message (GC runs whenever it likes), but
// the top of the list and its image counts are still informative. A render
// timeout still rendered and still got sampled after the wait — only a body
// timeout means nothing was shown — so only those are left out.
const deltas = [];
for (const round of rounds) {
  const m = messages.filter((r) => r.round === round && r.status !== 'body-timeout');
  for (let i = 1; i < m.length; i += 1) {
    deltas.push({ ...m[i], delta: m[i].web_anon_kb - m[i - 1].web_anon_kb });
  }
}
const top = [...deltas].sort((a, b) => b.delta - a.delta).slice(0, 12);
console.log('largest single-message growth (web anon):');
console.log('  Δ            round  imgs   body KB   settled ms  email_id');
for (const r of top) {
  console.log(`  ${mb(r.delta)}  ${String(r.round).padStart(5)}  ${String(r.img_count).padStart(4)}  ${String(Math.round(r.body_bytes / 1024)).padStart(8)}  ${String(r.render_settled_ms ?? '').padStart(10)}  ${r.email_id}`);
}
console.log();

// Pearson r between image count and growth, across all ok messages.
const xs = deltas.map((r) => r.img_count), ys = deltas.map((r) => r.delta);
const mean = (a) => a.reduce((s, v) => s + v, 0) / a.length;
const mx = mean(xs), my = mean(ys);
const cov = xs.reduce((s, x, i) => s + (x - mx) * (ys[i] - my), 0);
const sx = Math.sqrt(xs.reduce((s, x) => s + (x - mx) ** 2, 0));
const sy = Math.sqrt(ys.reduce((s, y) => s + (y - my) ** 2, 0));
const r = sx && sy ? cov / (sx * sy) : 0;
const withImgs = deltas.filter((d) => d.img_count > 0), without = deltas.filter((d) => d.img_count === 0);
console.log(`image count vs growth:  r = ${r.toFixed(2)}   ` +
  `mean Δ with images ${withImgs.length ? mb(mean(withImgs.map((d) => d.delta))) : 'n/a'} (n=${withImgs.length})   ` +
  `without ${without.length ? mb(mean(without.map((d) => d.delta))) : 'n/a'} (n=${without.length})`);
console.log(Math.abs(r) >= 0.4
  ? '  → growth tracks image count: decoded images are a real contributor.'
  : '  → weak relation to image count: look elsewhere (DOM/listener retention, JS heap).');
