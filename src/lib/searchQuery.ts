import type { Email } from '$lib/types';

export interface SearchClause {
  /** Field name (lowercased) or empty string for free-text. */
  field: string;
  value: string;
  /** Pre-compiled regex for `re:` clauses. null if the pattern was invalid
      or exceeded the length cap; such clauses match nothing. */
  regex?: RegExp | null;
}

/** Reject pathological patterns outright to bound worst-case ReDoS cost. */
const MAX_REGEX_LENGTH = 200;

/**
 * Tokenize a search query into clauses. Supports:
 *   - `key:value` operators (from:, to:, subject:, label:, is:, has:)
 *   - quoted values: `label:"My Project"` or `from:"Alice Smith"`
 *   - free-text terms (any token without a `:`)
 *
 * Multiple clauses are AND-combined. Whitespace separates tokens.
 */
export function parseSearchQuery(input: string): SearchClause[] {
  const clauses: SearchClause[] = [];
  const trimmed = input.trim();
  if (!trimmed) return clauses;

  let i = 0;
  while (i < trimmed.length) {
    while (i < trimmed.length && /\s/.test(trimmed[i])) i++;
    if (i >= trimmed.length) break;

    // Read until ':' (operator) or whitespace (free text).
    let tokenStart = i;
    while (i < trimmed.length && trimmed[i] !== ':' && !/\s/.test(trimmed[i])) i++;

    if (i < trimmed.length && trimmed[i] === ':') {
      const field = trimmed.slice(tokenStart, i).toLowerCase();
      i++; // consume ':'
      const value = readValue(trimmed, i);
      const clause: SearchClause = { field, value: value.value };
      if (field === 're') clause.regex = compileRegex(value.value);
      clauses.push(clause);
      i = value.next;
    } else {
      const word = trimmed.slice(tokenStart, i);
      if (word) clauses.push({ field: '', value: word });
    }
  }

  return clauses;
}

/** Compile a user-supplied regex. Case-insensitive by default. Returns null
    for invalid patterns or patterns over the length cap (clause matches nothing). */
function compileRegex(pattern: string): RegExp | null {
  if (!pattern || pattern.length > MAX_REGEX_LENGTH) return null;
  try {
    return new RegExp(pattern, 'i');
  } catch {
    return null;
  }
}

/** Read a possibly-quoted value starting at `start`; return value and next index. */
function readValue(s: string, start: number): { value: string; next: number } {
  if (start >= s.length) return { value: '', next: start };
  if (s[start] === '"') {
    let i = start + 1;
    let out = '';
    while (i < s.length && s[i] !== '"') {
      out += s[i];
      i++;
    }
    if (i < s.length) i++; // closing quote
    return { value: out, next: i };
  }
  let i = start;
  while (i < s.length && !/\s/.test(s[i])) i++;
  return { value: s.slice(start, i), next: i };
}

/**
 * Test whether an email matches all clauses. Field semantics:
 *   - from / to: substring match on name or email (case-insensitive)
 *   - subject: substring match on subject
 *   - label: case-insensitive exact match on any label
 *   - is: read | unread | starred | replied | pinned | priority | regular
 *   - has: attachment | attach
 *   - free-text: substring match across subject, from name/email, and the
 *     body's pre-indexed `searchText`. Emails without a downloaded body are
 *     only searchable on subject/sender until the body arrives.
 */
export function emailMatchesQuery(email: Email, clauses: SearchClause[]): boolean {
  for (const c of clauses) {
    if (!matchesClause(email, c)) return false;
  }
  return true;
}

function matchesClause(email: Email, clause: SearchClause): boolean {
  const v = clause.value.trim().toLowerCase();
  if (!v && clause.field !== '') return true; // empty operator value matches everything

  switch (clause.field) {
    case 'from':
      return (
        email.from.name.toLowerCase().includes(v) ||
        email.from.email.toLowerCase().includes(v)
      );
    case 'to':
      return (email.to ?? []).some(
        (r) => r.name.toLowerCase().includes(v) || r.email.toLowerCase().includes(v),
      ) || (email.cc ?? []).some(
        (r) => r.name.toLowerCase().includes(v) || r.email.toLowerCase().includes(v),
      );
    case 'subject':
      return email.subject.toLowerCase().includes(v);
    case 'label':
      return (email.labels ?? []).some((l) => l.toLowerCase() === v);
    case 'is':
      switch (v) {
        case 'read':     return email.isRead === true;
        case 'unread':   return email.isRead === false;
        case 'starred':  return email.isStarred === true;
        case 'pinned':   return email.isPinned === true;
        case 'replied':  return email.isReplied === true;
        case 'priority': return email.isFocused !== false;
        case 'regular':  return email.isFocused === false;
        default: return true;
      }
    case 'has':
      if (v === 'attachment' || v === 'attach') return email.hasAttachment === true;
      if (v === 'label') return !!email.labels && email.labels.length > 0;
      return true;
    case 're':
      return matchesRegex(email, clause.regex);
    default:
      // Free text (field === '') or unknown operator — match value across common fields.
      return matchesFreeText(email, v);
  }
}

function matchesRegex(email: Email, re: RegExp | null | undefined): boolean {
  if (!re) return false; // Invalid or overlong pattern — match nothing.
  return (
    re.test(email.subject) ||
    re.test(email.from.name) ||
    re.test(email.from.email) ||
    (email.searchText !== undefined && re.test(email.searchText))
  );
}

function matchesFreeText(email: Email, needle: string): boolean {
  // `needle` is already lowercased by the caller.
  // `searchText` is pre-lowercased and HTML-stripped; it's the full body text,
  // so we don't also check `preview` (which is just a truncated slice of it).
  return (
    email.subject.toLowerCase().includes(needle) ||
    email.from.name.toLowerCase().includes(needle) ||
    email.from.email.toLowerCase().includes(needle) ||
    (email.searchText !== undefined && email.searchText.includes(needle))
  );
}

/**
 * Build the lowercased, HTML-stripped body text cached on `Email.searchText`.
 * Regex-based (no DOM) so it's cheap to run across thousands of emails at
 * account-load time.
 */
export function buildSearchText(html: string): string {
  if (!html) return '';
  return html
    .replace(/<style[^>]*>[\s\S]*?<\/style>/gi, ' ')
    .replace(/<script[^>]*>[\s\S]*?<\/script>/gi, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    .replace(/&#(\d+);/g, (_, d) => String.fromCharCode(parseInt(d, 10)))
    .replace(/\s+/g, ' ')
    .trim()
    .toLowerCase();
}
