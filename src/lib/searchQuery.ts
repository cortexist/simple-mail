import type { Email } from '$lib/types';

export interface SearchClause {
  /** Field name (lowercased) or empty string for free-text. */
  field: string;
  value: string;
}

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
      clauses.push({ field, value: value.value });
      i = value.next;
    } else {
      const word = trimmed.slice(tokenStart, i);
      if (word) clauses.push({ field: '', value: word });
    }
  }

  return clauses;
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
 *   - free-text: substring match across subject, from name/email, preview
 */
export function emailMatchesQuery(email: Email, clauses: SearchClause[]): boolean {
  for (const c of clauses) {
    if (!matchesClause(email, c)) return false;
  }
  return true;
}

function matchesClause(email: Email, clause: SearchClause): boolean {
  const v = clause.value.toLowerCase();
  if (!v && clause.field !== '') return true; // empty operator value matches everything

  switch (clause.field) {
    case '':
      return (
        email.subject.toLowerCase().includes(v) ||
        email.from.name.toLowerCase().includes(v) ||
        email.from.email.toLowerCase().includes(v) ||
        email.preview.toLowerCase().includes(v)
      );
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
    default:
      // Unknown operator — fall through to free-text behavior on the value
      // so the token isn't silently dropped.
      return (
        email.subject.toLowerCase().includes(v) ||
        email.from.name.toLowerCase().includes(v) ||
        email.preview.toLowerCase().includes(v)
      );
  }
}
