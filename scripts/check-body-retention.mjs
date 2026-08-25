/**
 * check-body-retention.mjs — assert that the retained-body set stays bounded
 * and never evicts the message being read.
 *
 * `Email.body` is now held only for recently-viewed messages; everything else
 * falls back to a lazy re-fetch out of SQLite. Two things have to hold for that
 * to be safe, and both fail silently if broken: the set must actually stay
 * bounded (or the growth it was meant to stop comes straight back), and the
 * most recently retained id must never be the one handed back for eviction (or
 * the open message blanks out mid-read).
 *
 *   node scripts/check-body-retention.mjs
 *
 * Exits non-zero on any violation, so it can gate a release.
 */

import {
  retainBody,
  releaseBody,
  retainedBodyCount,
  resetBodyRetention,
} from '../src/lib/bodyRetention.ts';

let failures = 0;

function check(name, fn) {
  resetBodyRetention();
  try {
    fn();
    console.log(`  ok    ${name}`);
  } catch (error) {
    failures += 1;
    console.error(`  FAIL  ${name}\n        ${error.message}`);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

console.log('body retention:');

check('stays bounded across far more messages than it can hold', () => {
  for (let i = 0; i < 5000; i += 1) retainBody(`mail-${i}`);
  assert(retainedBodyCount() <= 40, `retained ${retainedBodyCount()}, expected <= 40`);
});

check('never evicts the message just retained', () => {
  for (let i = 0; i < 5000; i += 1) {
    const evicted = retainBody(`mail-${i}`);
    assert(!evicted.includes(`mail-${i}`), `mail-${i} evicted itself`);
  }
});

check('evicts least-recently-viewed first', () => {
  for (let i = 0; i < 40; i += 1) retainBody(`mail-${i}`);
  const evicted = retainBody('mail-new');
  assert(evicted.length === 1, `expected 1 eviction, got ${evicted.length}`);
  assert(evicted[0] === 'mail-0', `expected mail-0, got ${evicted[0]}`);
});

check('re-viewing a message keeps it out of the eviction queue', () => {
  for (let i = 0; i < 40; i += 1) retainBody(`mail-${i}`);
  retainBody('mail-0');                       // back to most recent
  const evicted = retainBody('mail-new');
  assert(evicted[0] === 'mail-1', `expected mail-1, got ${evicted[0]}`);
});

check('retaining the same message repeatedly does not grow the set', () => {
  for (let i = 0; i < 500; i += 1) retainBody('mail-same');
  assert(retainedBodyCount() === 1, `retained ${retainedBodyCount()}, expected 1`);
});

check('released ids leave the set', () => {
  retainBody('mail-a');
  retainBody('mail-b');
  releaseBody('mail-a');
  assert(retainedBodyCount() === 1, `retained ${retainedBodyCount()}, expected 1`);
  const evicted = retainBody('mail-a');
  assert(evicted.length === 0, 'releasing then re-retaining should not evict');
  assert(retainedBodyCount() === 2, `retained ${retainedBodyCount()}, expected 2`);
});

check('releasing an unknown id is a no-op', () => {
  retainBody('mail-a');
  releaseBody('mail-missing');
  assert(retainedBodyCount() === 1, `retained ${retainedBodyCount()}, expected 1`);
});

if (failures > 0) {
  console.error(`\n${failures} check(s) failed.`);
  process.exit(1);
}
console.log('\nAll checks passed.');
