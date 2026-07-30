/**
 * check-body-unwrap.mjs — assert that unwrapping a mail body never deletes the
 * mail.
 *
 * ReadingPane renders a received message by pulling the content out of its
 * HTML wrapper (`unwrapHtmlDocument`) and re-wrapping it in our own document.
 * That transform is regex-based, and its failure mode is silent: the pane
 * renders a well-formed, correctly-sized document containing none of the
 * message. Two shipped bugs looked exactly like that from the outside, so the
 * invariant is checked directly instead: visible text must survive the unwrap.
 *
 * Run it after any change to src/lib/emailBody.ts:
 *
 *   node scripts/check-body-unwrap.mjs                # fixtures + local corpus
 *   node scripts/check-body-unwrap.mjs --self-test    # fixtures only (no DB)
 *   node scripts/check-body-unwrap.mjs --db <path>    # a specific mail.db
 *   node scripts/check-body-unwrap.mjs -v             # list every mail checked
 *
 * Exits non-zero on any loss, so it can gate a release.
 */

import { homedir } from 'node:os';
import { join } from 'node:path';
import { existsSync } from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import { isHtmlDocument, unwrapHtmlDocument } from '../src/lib/emailBody.ts';

const args = process.argv.slice(2);
const verbose = args.includes('-v') || args.includes('--verbose');
const selfTestOnly = args.includes('--self-test');
const dbPath = args.includes('--db')
  ? args[args.indexOf('--db') + 1]
  : join(homedir(), '.cortexist', 'mail', 'mail.db');

/** Text a reader should end up seeing, measured independently of the transform
 *  under test — head blocks (every one, since nested documents bring their own)
 *  plus comments, styles and scripts removed, then all markup dropped.
 *
 *  Deliberately a second, dumber implementation. A reference that reused
 *  `unwrapHtmlDocument` would agree with it by construction and prove nothing. */
function visibleText(html) {
  return html
    .replace(/<head[^>]*>[\s\S]*?<\/head>/gi, ' ')
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<script[\s\S]*?<\/script>/gi, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&\S{1,8};/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Loss is reported in characters of visible text. A couple of characters of
 *  slack absorbs entity/whitespace-normalization noise between the reference
 *  and the transform; anything above that is real content going missing. */
const SLACK = 4;

function check(body) {
  const expected = visibleText(body).length;
  const actual = visibleText(isHtmlDocument(body) ? unwrapHtmlDocument(body) : body).length;
  return { expected, actual, lost: expected - actual, ok: expected - actual <= SLACK };
}

// ── Fixtures ───────────────────────────────────────────────────────────────
// Run with or without a corpus, so the invariant is still enforced on a fresh
// checkout or in CI where nobody's mail.db exists.

const FIXTURES = [
  {
    name: 'nested document in an HTML block (Medium, 3DEXPERIENCE)',
    // The shipped bug: a lazy `</body>` match stopped at the inner document's
    // close and dropped everything after it.
    body: '<!DOCTYPE html><html><head><style>p{color:#000}</style></head><body>'
      + '<table><tr><td>OUTER_TOP</td></tr><tr><td>'
      + '<!DOCTYPE html><html><head><title>t</title></head><body>INNER</body></html>'
      + '</td></tr></table>OUTER_TAIL</body></html>',
    mustContain: ['OUTER_TOP', 'INNER', 'OUTER_TAIL'],
  },
  {
    name: 'ordinary single document',
    body: '<!DOCTYPE html><html><head><title>subject</title></head><body><p>HELLO</p></body></html>',
    mustContain: ['HELLO'],
  },
  {
    name: 'bare fragment, no wrapper',
    body: '<div><p>HELLO</p></div>',
    mustContain: ['HELLO'],
  },
  {
    name: 'document with no closing body tag',
    body: '<!DOCTYPE html><html><head></head><body><p>HELLO</p>',
    mustContain: ['HELLO'],
  },
  {
    name: '<header> is not mistaken for <head>',
    body: '<!DOCTYPE html><html><body><header>BRANDING</header><p>HELLO</p></body></html>',
    mustContain: ['BRANDING', 'HELLO'],
  },
];

let failures = 0;

for (const f of FIXTURES) {
  const out = isHtmlDocument(f.body) ? unwrapHtmlDocument(f.body) : f.body;
  const missing = f.mustContain.filter((needle) => !out.includes(needle));
  const { lost, ok } = check(f.body);
  if (missing.length || !ok) {
    failures++;
    console.error(`FAIL  fixture: ${f.name}`);
    if (missing.length) console.error(`        dropped: ${missing.join(', ')}`);
    if (!ok) console.error(`        lost ${lost} chars of visible text`);
  } else if (verbose) {
    console.log(`ok    fixture: ${f.name}`);
  }
}

console.log(`fixtures: ${FIXTURES.length - failures}/${FIXTURES.length} passed`);

// ── Corpus ─────────────────────────────────────────────────────────────────
// Real mail is the part that matters: these bugs are found by volume and
// sender diversity, not by cases anyone thought to write down.

if (!selfTestOnly) {
  if (!existsSync(dbPath)) {
    console.log(`corpus:   skipped (no database at ${dbPath})`);
  } else {
    const db = new DatabaseSync(dbPath, { readOnly: true });
    const rows = db
      .prepare("SELECT from_email, subject, body FROM emails WHERE body != '' ORDER BY date DESC")
      .all();
    db.close();

    let checked = 0;
    let lossy = 0;
    for (const row of rows) {
      if (!isHtmlDocument(row.body)) continue;
      checked++;
      const { expected, actual, lost, ok } = check(row.body);
      if (!ok) {
        lossy++;
        failures++;
        const pct = expected ? Math.round((lost / expected) * 100) : 100;
        console.error(`FAIL  ${row.from_email}  ${JSON.stringify(row.subject.slice(0, 60))}`);
        console.error(`        ${actual} of ${expected} chars of visible text survived (lost ${pct}%)`);
      } else if (verbose) {
        console.log(`ok    ${row.from_email}  ${expected} chars`);
      }
    }
    console.log(`corpus:   ${checked - lossy}/${checked} HTML mails kept their text  (${dbPath})`);
  }
}

if (failures) {
  console.error(`\n${failures} check(s) failed — the unwrap transform is deleting message content.`);
  process.exit(1);
}
console.log('\nAll checks passed.');
