export function formatDate(date: Date): string {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const emailDate = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  const diffDays = Math.floor((today.getTime() - emailDate.getTime()) / (1000 * 60 * 60 * 24));
  const dayNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

  const time = date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit', hour12: true });

  // Today → time only: "9:20 PM"
  if (diffDays === 0) {
    return time;
  }
  // Within the past week → "Mon 9:20 PM"
  if (diffDays < 7) {
    return `${dayNames[date.getDay()]} ${time}`;
  }
  // Within 4 weeks → "Fri 3/6"
  if (diffDays < 28) {
    return `${dayNames[date.getDay()]} ${date.getMonth() + 1}/${date.getDate()}`;
  }
  // Older → "1/31/2026"
  return `${date.getMonth() + 1}/${date.getDate()}/${date.getFullYear()}`;
}

export function formatFullDate(date: Date): string {
  return date.toLocaleDateString('en-US', {
    weekday: 'long',
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    hour12: true,
  });
}

/* ── Calendar Helpers ── */

/** Get the Sunday-start week containing a given date */
export function getWeekDays(date: Date): Date[] {
  const d = new Date(date);
  const day = d.getDay(); // 0=Sun..6=Sat
  const sunday = new Date(d.getFullYear(), d.getMonth(), d.getDate() - day);
  return Array.from({ length: 7 }, (_, i) => {
    const dt = new Date(sunday);
    dt.setDate(sunday.getDate() + i);
    return dt;
  });
}

/** Get all dates in a month grid (6 rows × 7 columns, starting Sunday) */
export function getMonthGrid(year: number, month: number): Date[] {
  const first = new Date(year, month, 1);
  const day = first.getDay(); // 0=Sun..6=Sat
  const start = new Date(year, month, 1 - day);
  return Array.from({ length: 42 }, (_, i) => {
    const dt = new Date(start);
    dt.setDate(start.getDate() + i);
    return dt;
  });
}

/** Check if two dates are the same day */
export function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/** Format time as "9 AM", "2:30 PM" (locale-aware) */
export function formatTime(date: Date, loc: string = 'en'): string {
  return date.toLocaleTimeString(loc, { hour: 'numeric', minute: '2-digit' });
}

/** Format time range "9 AM – 10:30 AM" (locale-aware) */
export function formatTimeRange(start: Date, end: Date, loc: string = 'en'): string {
  return `${formatTime(start, loc)} – ${formatTime(end, loc)}`;
}

/** Short day header "Mon 3" (locale-aware) */
export function formatDayHeader(date: Date, loc: string = 'en'): { weekday: string; day: number } {
  const weekday = date.toLocaleDateString(loc, { weekday: 'long' });
  return { weekday, day: date.getDate() };
}

/** Month + Year header "March 2026" (locale-aware) */
export function formatMonthYear(date: Date, loc: string = 'en'): string {
  return date.toLocaleDateString(loc, { month: 'long', year: 'numeric' });
}

/** Pragmatic email check — local@domain.tld with a 2+ char TLD.
 *  Not RFC 5322; rejects whitespace, missing @, missing TLD, and trailing dots. */
const EMAIL_RE = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/;
export function isLikelyEmail(s: string): boolean {
  return EMAIL_RE.test(s.trim());
}
