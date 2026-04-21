<script lang="ts">
  import type { CalendarEvent, CalendarCategory, CalendarViewMode, EventAttendee, EventRecurrence, FullContact } from '$lib/types';
  import {
    getWeekDays,
    getMonthGrid,
    isSameDay,
    formatTime,
    formatTimeRange,
    formatDayHeader,
    formatMonthYear,
  } from '$lib/utils';
  import { open as shellOpen } from '@tauri-apps/plugin-shell';
  import { t, locale } from '$lib/i18n/index.svelte';
  import DatePicker from './DatePicker.svelte';
  import TimePicker from './TimePicker.svelte';

  interface Props {
    events: CalendarEvent[];
    categories: CalendarCategory[];
    viewMode: CalendarViewMode;
    contacts?: FullContact[];
    onSaveEvent?: (event: CalendarEvent) => void;
    onDeleteEvent?: (eventId: string, instanceDate?: string, deleteMode?: 'single' | 'future' | 'all') => void;
    requestNewEvent?: number;
    onResetNewEvent?: () => void;
    prefillMeeting?: { title: string; attendees: EventAttendee[] } | null;
    onResetPrefillMeeting?: () => void;
    searchQuery?: string;
    onClearSearch?: () => void;
    calFocusedPane?: 'none' | 'cal-sidebar' | 'cal-main';
    calInnerPane?: 'none' | 'cal-list-inner' | 'cal-mini-inner' | 'cal-main-inner';
    onFocusPaneRequest?: (pane: 'cal-list-inner'| 'cal-mini-inner' | 'cal-main') => void;
  }

  let { events, categories, viewMode, contacts = [], onSaveEvent, onDeleteEvent, requestNewEvent, onResetNewEvent, prefillMeeting = null, onResetPrefillMeeting, searchQuery = '', onClearSearch, calFocusedPane = 'none', calInnerPane = 'none', onFocusPaneRequest }: Props = $props();

  const CALENDAR_NAME_KEYS: Record<string, string> = {
    'Calendar': 'calendar.calPersonal',
    'Personal': 'calendar.calPersonal',
    'Work': 'calendar.calWork',
    'Birthdays': 'calendar.calBirthdays',
    'Holidays': 'calendar.calHolidays',
    'US Holidays': 'calendar.calHolidays',
  };

  function calendarDisplayName(cat: CalendarCategory): string {
    const key = CALENDAR_NAME_KEYS[cat.name];
    return key ? t(key) : cat.name;
  }

  const HOUR_HEIGHT = 48;
  const START_HOUR = 0;
  const END_HOUR = 24;
  const hours = Array.from({ length: END_HOUR - START_HOUR }, (_, i) => i + START_HOUR);
  const today = new Date();

  let currentDate = $state(new Date());
  let selectedEvent = $state<CalendarEvent | null>(null);
  let miniCalMonth = $state(today.getMonth());
  let miniCalYear = $state(today.getFullYear());

  let weekDays = $derived(getWeekDays(currentDate));
  let miniCalGrid = $derived(getMonthGrid(miniCalYear, miniCalMonth));
  let visibleCategories = $derived(categories.filter((c) => c.visible).map((c) => c.id));

  // ── Keyboard nav state ──
  let calListFocusedIndex = $state(-1);

  // ── Event detail card focus ──
  let detailEditBtnEl = $state<HTMLButtonElement | null>(null);
  let detailDeleteBtnEl = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    if (selectedEvent) {
      // Focus the Edit button once the card renders
      setTimeout(() => detailEditBtnEl?.focus(), 0);
    }
  });

  let filteredEvents = $derived(
    events.filter((e) => visibleCategories.includes(e.calendarId))
  );

  // ── Search results ──
  let isSearching = $derived(searchQuery.trim().length > 0);
  let searchSelectedEvent = $state<CalendarEvent | null>(null);

  let searchResults = $derived.by(() => {
    if (!isSearching) return { upcoming: [] as CalendarEvent[], past: [] as CalendarEvent[] };
    const q = searchQuery.trim().toLowerCase();
    const now = new Date();

    // Filter events matching query
    const matched = filteredEvents.filter(e =>
      e.title.toLowerCase().includes(q) ||
      (e.location ?? '').toLowerCase().includes(q) ||
      (e.description ?? '').toLowerCase().includes(q) ||
      (e.attendees ?? []).some(a => a.name.toLowerCase().includes(q))
    );

    // Deduplicate recurring events: keep only the nearest instance per base ID
    const deduped = new Map<string, CalendarEvent>();
    for (const e of matched) {
      const baseId = e.recurrence ? e.id.replace(/_\d{8}T\d{6}$/, '') : e.id;
      const existing = deduped.get(baseId);
      if (!existing) {
        deduped.set(baseId, e);
      } else {
        // Prefer the instance closest to now (upcoming > past)
        const existDist = Math.abs(existing.start.getTime() - now.getTime());
        const newDist = Math.abs(e.start.getTime() - now.getTime());
        if (e.start >= now && existing.start < now) {
          deduped.set(baseId, e); // prefer upcoming
        } else if (!(existing.start >= now && e.start < now) && newDist < existDist) {
          deduped.set(baseId, e); // prefer closer to now
        }
      }
    }

    const all = Array.from(deduped.values());
    const upcoming = all.filter(e => e.end >= now).sort((a, b) => a.start.getTime() - b.start.getTime());
    const past = all.filter(e => e.end < now).sort((a, b) => b.start.getTime() - a.start.getTime());
    return { upcoming, past };
  });

  // Search result navigation state
  let searchFocusedIndex = $state(-1);
  let searchListScrollEl = $state<HTMLDivElement | undefined>();
  let allSearchResults = $derived([...searchResults.upcoming, ...searchResults.past]);

  // Reset search nav state when search query changes
  $effect(() => {
    searchQuery; // track
    searchSelectedEvent = null;
    searchFocusedIndex = -1;
  });

  export function focusSearchResults() {
    const all = allSearchResults;
    if (all.length > 0) {
      searchFocusedIndex = 0;
      searchSelectedEvent = all[0];
      requestAnimationFrame(() => {
        searchListScrollEl?.querySelector('.search-event-item.active')?.scrollIntoView({ block: 'nearest' });
      });
    } else {
      searchFocusedIndex = -1;
      searchSelectedEvent = null;
    }
  }

  /** Returns true if the key was handled, false if the caller should handle it (e.g. ArrowLeft → go to cal-sidebar). */
  export function navigateSearchResults(key: string): boolean {
    const all = allSearchResults;
    if (key === 'ArrowDown') {
      const next = searchFocusedIndex === -1 ? 0 : Math.min(searchFocusedIndex + 1, all.length - 1);
      searchFocusedIndex = next;
      searchSelectedEvent = all[next] ?? null;
      requestAnimationFrame(() => {
        searchListScrollEl?.querySelector('.search-event-item.active')?.scrollIntoView({ block: 'nearest' });
      });
      return true;
    }
    if (key === 'ArrowUp') {
      if (searchFocusedIndex <= 0) return true;
      const next = searchFocusedIndex - 1;
      searchFocusedIndex = next;
      searchSelectedEvent = all[next] ?? null;
      requestAnimationFrame(() => {
        searchListScrollEl?.querySelector('.search-event-item.active')?.scrollIntoView({ block: 'nearest' });
      });
      return true;
    }
    if (key === 'ArrowLeft') return false; // let page handler move to cal-sidebar
    if (key === 'ArrowRight') return true; // nowhere to go right
    return true;
  }

  export function editSearchSelectedEvent() {
    if (searchSelectedEvent) openEditEvent(searchSelectedEvent);
  }

  export function joinMeeting(): boolean {
    const ev = searchSelectedEvent ?? selectedEvent;
    if (!ev?.isOnlineMeeting || !ev.meetingUrl || ev.end < new Date()) return false;
    shellOpen(ev.meetingUrl).catch(() => window.open(ev.meetingUrl, '_blank', 'noopener,noreferrer'));
    return true;
  }

  export function deleteSearchSelectedEvent() {
    if (searchSelectedEvent) {
      const ev = searchSelectedEvent;
      confirmDeleteEvent(ev);
      if (!ev.recurrence) searchSelectedEvent = null;
    }
  }

  export function goToToday() {
    const now = new Date();
    currentDate = now;
    miniCalMonth = now.getMonth();
    miniCalYear = now.getFullYear();
    if (viewMode !== 'month') {
      const minutes = Math.floor((now.getHours() * 60 + now.getMinutes()) / 30) * 30;
      selectedSlot = { date: now, minutes };
      scrollSlotIntoView(minutes);
    }
  }

  function navigateWeek(delta: number) {
    const d = new Date(currentDate);
    if (viewMode === 'month') {
      d.setMonth(d.getMonth() + delta);
    } else if (viewMode === 'day') {
      d.setDate(d.getDate() + delta);
    } else {
      d.setDate(d.getDate() + delta * 7);
    }
    currentDate = d;
  }

  function miniCalPrev() {
    if (miniCalMonth === 0) {
      miniCalMonth = 11;
      miniCalYear--;
    } else {
      miniCalMonth--;
    }
  }

  function miniCalNext() {
    if (miniCalMonth === 11) {
      miniCalMonth = 0;
      miniCalYear++;
    } else {
      miniCalMonth++;
    }
  }

  function selectMiniCalDay(date: Date) {
    currentDate = date;
    miniCalMonth = date.getMonth();
    miniCalYear = date.getFullYear();
  }

  /** Check whether a date falls in the same week (Mon–Sun) as currentDate */
  function isInSelectedWeek(date: Date): boolean {
    const week = getWeekDays(currentDate);
    return week.some((d) => isSameDay(d, date));
  }

  function getEventTooltip(event: CalendarEvent): string {
    let tip = event.title;
    tip += '\n' + formatTimeRange(event.start, event.end, locale());
    if (event.location) tip += '\n' + event.location;
    return tip;
  }

  /**
   * If event has recurrence, returns a virtual instance adjusted to `day`,
   * or null if the recurrence doesn't land on that day.
   */
  function getRecurringInstanceForDay(event: CalendarEvent, day: Date): CalendarEvent | null {
    const { recurrence } = event;
    if (!recurrence) return null;

    const base = event.start;
    const baseDate = new Date(base.getFullYear(), base.getMonth(), base.getDate());
    const targetDate = new Date(day.getFullYear(), day.getMonth(), day.getDate());

    if (targetDate < baseDate) return null;

    if (recurrence.endDate) {
      const [ey, em, ed] = recurrence.endDate.split('-').map(Number);
      if (targetDate > new Date(ey, em - 1, ed)) return null;
    }

    const diffDays = Math.round((targetDate.getTime() - baseDate.getTime()) / 86400000);
    let occurs = false;

    switch (recurrence.freq) {
      case 'daily':
        occurs = diffDays % recurrence.interval === 0;
        break;
      case 'weekly':
        occurs = diffDays % (recurrence.interval * 7) === 0;
        break;
      case 'monthly': {
        const monthDiff =
          (targetDate.getFullYear() - baseDate.getFullYear()) * 12 +
          (targetDate.getMonth() - baseDate.getMonth());
        if (monthDiff % recurrence.interval !== 0) { occurs = false; break; }
        occurs = recurrence.byDay
          ? matchesByDay(recurrence.byDay, targetDate)
          : targetDate.getDate() === baseDate.getDate();
        break;
      }
      case 'yearly': {
        const yearDiff = targetDate.getFullYear() - baseDate.getFullYear();
        if (yearDiff % recurrence.interval !== 0 || targetDate.getMonth() !== baseDate.getMonth()) { occurs = false; break; }
        occurs = recurrence.byDay
          ? matchesByDay(recurrence.byDay, targetDate)
          : targetDate.getDate() === baseDate.getDate();
        break;
      }
    }

    if (!occurs) return null;

    // Check exception dates (deleted single occurrences)
    if (recurrence.exdates?.length) {
      const targetStr = `${targetDate.getFullYear()}-${String(targetDate.getMonth() + 1).padStart(2, '0')}-${String(targetDate.getDate()).padStart(2, '0')}`;
      if (recurrence.exdates.includes(targetStr)) return null;
    }

    const duration = event.end.getTime() - event.start.getTime();
    const instStart = new Date(targetDate);
    instStart.setHours(base.getHours(), base.getMinutes(), base.getSeconds(), 0);
    const instEnd = new Date(instStart.getTime() + duration);

    return { ...event, start: instStart, end: instEnd };
  }

  function getEventsForDay(day: Date): CalendarEvent[] {
    const result: CalendarEvent[] = [];
    for (const e of filteredEvents) {
      if (e.recurrence) {
        const inst = getRecurringInstanceForDay(e, day);
        if (inst && !inst.isAllDay) result.push(inst);
      } else if (!e.isAllDay && isSameDay(e.start, day)) {
        result.push(e);
      }
    }
    return result.sort((a, b) => a.start.getTime() - b.start.getTime());
  }

  function getOverlappingEvents(event: CalendarEvent): CalendarEvent[] {
    return getEventsForDay(event.start).filter(e => e.start < event.end && event.start < e.end);
  }

  function handleDetailKeydown(e: KeyboardEvent) {
    e.stopPropagation();

    if (e.key === 'Escape') {
      selectedEvent = null;
      return;
    }

    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      const focused = document.activeElement;
      if (focused === detailEditBtnEl) {
        detailDeleteBtnEl?.focus();
      } else {
        detailEditBtnEl?.focus();
      }
      return;
    }

    if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && selectedEvent) {
      e.preventDefault();
      const peers = viewMode === 'month'
        ? getEventsForDay(selectedEvent.start)
        : getOverlappingEvents(selectedEvent);
      if (peers.length > 1) {
        const idx = peers.findIndex(ev => ev.id === selectedEvent!.id);
        const next = e.key === 'ArrowDown'
          ? (idx + 1) % peers.length
          : (idx - 1 + peers.length) % peers.length;
        selectedEvent = peers[next];
      }
      return;
    }

    if (e.key === 'j' && e.ctrlKey && selectedEvent?.isOnlineMeeting) {
      e.preventDefault();
      if (selectedEvent.meetingUrl && selectedEvent.end >= new Date()) {
        shellOpen(selectedEvent.meetingUrl).catch(() => window.open(selectedEvent!.meetingUrl!, '_blank', 'noopener,noreferrer'));
        selectedEvent = null;
      }
    }
  }

  function getAllDayEventsForDay(day: Date): CalendarEvent[] {
    const result: CalendarEvent[] = [];
    for (const e of filteredEvents) {
      if (e.recurrence) {
        const inst = getRecurringInstanceForDay(e, day);
        if (inst?.isAllDay) result.push(inst);
      } else if (e.isAllDay && isSameDay(e.start, day)) {
        result.push(e);
      }
    }
    return result;
  }

  function getMonthEventsForDay(day: Date): CalendarEvent[] {
    const result: CalendarEvent[] = [];
    for (const e of filteredEvents) {
      if (e.recurrence) {
        const inst = getRecurringInstanceForDay(e, day);
        if (inst) result.push(inst);
      } else if (isSameDay(e.start, day)) {
        result.push(e);
      }
    }
    return result.sort((a, b) => {
      if (a.isAllDay !== b.isAllDay) return a.isAllDay ? -1 : 1;
      return a.start.getTime() - b.start.getTime();
    });
  }

  function layoutEventsForDay(events: CalendarEvent[]): { event: CalendarEvent; column: number; totalColumns: number }[] {
    const sorted = [...events].sort((a, b) => a.start.getTime() - b.start.getTime());
    const columnEnds: Date[] = [];
    const eventColumns: number[] = [];

    for (const event of sorted) {
      let col = 0;
      while (col < columnEnds.length && columnEnds[col] > event.start) col++;
      eventColumns.push(col);
      columnEnds[col] = event.end;
    }

    return sorted.map((event, i) => {
      let maxCol = eventColumns[i];
      for (let j = 0; j < sorted.length; j++) {
        if (i !== j && sorted[i].start < sorted[j].end && sorted[j].start < sorted[i].end) {
          maxCol = Math.max(maxCol, eventColumns[j]);
        }
      }
      return { event, column: eventColumns[i], totalColumns: maxCol + 1 };
    });
  }

  function getEventStyle(event: CalendarEvent, column = 0, totalColumns = 1): string {
    const startMinutes = event.start.getHours() * 60 + event.start.getMinutes();
    const endMinutes = event.end.getHours() * 60 + event.end.getMinutes();
    const top = (startMinutes / 60) * HOUR_HEIGHT;
    const height = Math.max(((endMinutes - startMinutes) / 60) * HOUR_HEIGHT, 20);
    const widthPct = 100 / totalColumns;
    const leftPct = column * widthPct;
    return `top: ${top}px; height: ${height}px; left: calc(${leftPct}% + 2px); width: calc(${widthPct}% - 6px); background: ${event.color}80; border-left: 4px solid ${event.color}; color: var(--text-primary);`;
  }

  function getHeaderLabel(): string {
    if (viewMode === 'day') {
      return currentDate.toLocaleDateString(locale(), { weekday: 'long', month: 'long', day: 'numeric', year: 'numeric' });
    }
    if (viewMode === 'month') {
      return formatMonthYear(currentDate, locale());
    }
    const week = getWeekDays(currentDate);
    const first = week[0];
    const last = week[6];
    if (first.getMonth() === last.getMonth()) {
      return `${first.toLocaleDateString(locale(), { month: 'long' })} ${first.getDate()}–${last.getDate()}, ${first.getFullYear()}`;
    }
    return `${first.toLocaleDateString(locale(), { month: 'short' })} ${first.getDate()} – ${last.toLocaleDateString(locale(), { month: 'short' })} ${last.getDate()}, ${last.getFullYear()}`;
  }

  function getMonthViewGrid(): Date[] {
    return getMonthGrid(currentDate.getFullYear(), currentDate.getMonth());
  }

  let timeGridScrollEl = $state<HTMLDivElement | null>(null);

  // ── Click-to-select slot ──
  // null means nothing selected; minutes=null means date-only (month view)
  let selectedSlot = $state<{ date: Date; minutes: number | null } | null>(null);

  function handleDayColumnClick(e: MouseEvent, day: Date) {
    const target = e.target as HTMLElement;
    if (target.closest('.event-block')) return;
    const col = e.currentTarget as HTMLElement;
    // getBoundingClientRect().top is already scroll-adjusted (viewport coords)
    const y = e.clientY - col.getBoundingClientRect().top;
    const rawMinutes = (y / HOUR_HEIGHT) * 60;
    const snapped = Math.round(rawMinutes / 30) * 30;
    const minutes = Math.max(0, Math.min(snapped, 23 * 60 + 30));
    selectedSlot = { date: day, minutes };
  }

  function handleMonthCellClick(e: MouseEvent, day: Date) {
    const target = e.target as HTMLElement;
    if (target.closest('.month-event-pill')) return;
    currentDate = day;
    selectedSlot = null;
  }

  // Auto-scroll Day/Week view to earliest event or 6 AM (minus 15 min gap)
  $effect(() => {
    if (!timeGridScrollEl || viewMode === 'month') return;

    // Access reactive deps
    const days = viewMode === 'day' ? [currentDate] : weekDays;
    const dayEvents = days.flatMap((d) => getEventsForDay(d));

    // Earliest event start in minutes from midnight
    let earliestMinutes = 6 * 60; // default 6:00 AM
    for (const ev of dayEvents) {
      const m = ev.start.getHours() * 60 + ev.start.getMinutes();
      if (m < earliestMinutes) earliestMinutes = m;
    }

    // Subtract 15 min gap, clamp to 0
    const scrollMinutes = Math.max(0, earliestMinutes - 15);
    const scrollPx = (scrollMinutes / 60) * HOUR_HEIGHT;

    // Use tick-like delay so DOM is ready
    requestAnimationFrame(() => {
      if (timeGridScrollEl) {
        timeGridScrollEl.scrollTop = scrollPx;
      }
    });
  });

  // ── Event Edit/Create Modal ──
  let showEventModal = $state(false);
  let editingEventId = $state<string | null>(null);

  let evTitle = $state('');
  let evDate = $state('');
  let evStartTime = $state('09:00');
  let evEndTime = $state('10:00');
  let evLocation = $state('');
  let evDescription = $state('');
  let evCalendarId = $state('');
  let evIsAllDay = $state(false);
  let evIsOnline = $state(false);
  let evMeetingUrl = $state('');

  // Attendees
  type AttendeeForm = { name: string; email: string; initials: string; color: string; role: 'required' | 'optional'; photoUrl?: string };
  let evAttendees = $state<AttendeeForm[]>([]);
  let evAttendeeQuery = $state('');
  let evAttendeeDropdownOpen = $state(false);
  let evModalBodyEl = $state<HTMLDivElement | null>(null);
  let evModalEl = $state<HTMLDivElement | null>(null);
  let attSearchEl = $state<HTMLInputElement | null>(null);
  let attDropdownStyle = $state('');
  let evAttendeeResults = $derived(
    evAttendeeQuery.trim().length < 1 ? [] :
    contacts
      .filter((c) =>
        (c.name.toLowerCase().includes(evAttendeeQuery.toLowerCase()) ||
         c.email.toLowerCase().includes(evAttendeeQuery.toLowerCase())) &&
        !evAttendees.some((a) => a.email.toLowerCase() === c.email.toLowerCase())
      )
      .slice(0, 6)
  );

  function openAttendeeDropdown() {
    if (attSearchEl) {
      const r = attSearchEl.getBoundingClientRect();
      attDropdownStyle = `bottom:${window.innerHeight - r.top + 4}px;left:${r.left}px;width:${r.width}px`;
    }
    evAttendeeDropdownOpen = true;
  }

  function addAttendee(c: FullContact) {
    evAttendees = [...evAttendees, { name: c.name, email: c.email, initials: c.initials, color: c.color, role: 'required', photoUrl: c.photoUrl }];
    evAttendeeQuery = '';
    requestAnimationFrame(() => evModalBodyEl?.scrollTo({ top: evModalBodyEl.scrollHeight, behavior: 'smooth' }));
  }

  function removeAttendee(email: string) {
    const key = email.toLowerCase();
    evAttendees = evAttendees.filter((a) => a.email.toLowerCase() !== key);
  }

  function toggleAttendeeRole(email: string) {
    const key = email.toLowerCase();
    evAttendees = evAttendees.map((a) =>
      a.email.toLowerCase() === key ? { ...a, role: a.role === 'required' ? 'optional' : 'required' } : a
    );
  }

  // Recurrence
  let evRepeats = $state(false);
  let evRecurFreq = $state<'daily' | 'weekly' | 'monthly' | 'yearly'>('weekly');
  let evRecurInterval = $state(1);
  let evRecurEndDate = $state('');
  /** 'date' = same day-of-month, 'weekday' = Nth weekday (monthly only) */
  let evRecurByDayMode = $state<'date' | 'weekday'>('date');

  // Auto-enable yearly repeat for birthday/anniversary calendars
  $effect(() => {
    const name = (categories.find(c => c.id === evCalendarId)?.name ?? '').toLowerCase();
    if (name.includes('birthday') || name.includes('anniversary')) {
      if (!evRepeats) {
        evRepeats = true;
        evRecurFreq = 'yearly';
        evRecurInterval = 1;
      }
    }
  });

  // Alert
  let evAlertMinutes = $state<number | null>(null);
  const ALERT_OPTIONS: { key: string; value: number | null }[] = [
    { key: 'calendar.alertNone', value: null },
    { key: 'calendar.alertAtTime', value: 0 },
    { key: 'calendar.alert5min', value: 5 },
    { key: 'calendar.alert15min', value: 15 },
    { key: 'calendar.alert30min', value: 30 },
    { key: 'calendar.alert1hour', value: 60 },
    { key: 'calendar.alert1day', value: 1440 },
  ];

  // Delete recurring confirmation
  let showDeleteRecurDialog = $state(false);
  let pendingDeleteEvent = $state<CalendarEvent | null>(null);
  let pendingDeleteInstanceDate = $state<string>('');

  function focusFirst(node: HTMLElement) {
    requestAnimationFrame(() => {
      const btn = node.querySelector('button') as HTMLElement | null;
      btn?.focus();
    });
  }

  function confirmDeleteEvent(event: CalendarEvent) {
    if (event.recurrence) {
      pendingDeleteEvent = event;
      pendingDeleteInstanceDate = `${event.start.getFullYear()}-${String(event.start.getMonth() + 1).padStart(2, '0')}-${String(event.start.getDate()).padStart(2, '0')}`;
      showDeleteRecurDialog = true;
    } else {
      onDeleteEvent?.(event.id);
      selectedEvent = null;
    }
  }

  function executeRecurDelete(mode: 'single' | 'future' | 'all') {
    if (!pendingDeleteEvent) return;
    onDeleteEvent?.(pendingDeleteEvent.id, pendingDeleteInstanceDate, mode);
    showDeleteRecurDialog = false;
    pendingDeleteEvent = null;
    selectedEvent = null;
    searchSelectedEvent = null;
    if (editingEventId) { showEventModal = false; editingEventId = null; }
  }

  const WDAY_ABBR = ['SU', 'MO', 'TU', 'WE', 'TH', 'FR', 'SA'] as const;
  function localizedWeekday(dayIndex: number): string {
    return new Date(2024, 0, 7 + dayIndex).toLocaleDateString(locale(), { weekday: 'long' });
  }
  function localizedMonth(monthIndex: number): string {
    return new Date(2024, monthIndex, 1).toLocaleDateString(locale(), { month: 'long' });
  }
  const ORDINALS = ['', '1st', '2nd', '3rd', '4th', 'last'];

  /** Derive BYDAY token from a date string: "3TU", "-1MO", etc. */
  function deriveByDay(dateStr: string): string {
    const d = new Date(dateStr + 'T00:00:00');
    const wday = WDAY_ABBR[d.getDay()];
    const day = d.getDate();
    const daysInMonth = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
    if (day + 7 > daysInMonth) return `-1${wday}`;
    return `${Math.ceil(day / 7)}${wday}`;
  }

  /** Human-readable label for the derived BYDAY: "the 3rd Tuesday", "the last Monday" */
  function byDayLabel(dateStr: string): string {
    const token = deriveByDay(dateStr);
    const m = token.match(/^(-?\d+)(MO|TU|WE|TH|FR|SA|SU)$/);
    if (!m) return '';
    const n = parseInt(m[1]);
    const wdayName = localizedWeekday(WDAY_ABBR.indexOf(m[2] as typeof WDAY_ABBR[number]));
    const ord = n === -1 ? ORDINALS[5] : ORDINALS[n] ?? `${n}th`;
    return `the ${ord} ${wdayName}`;
  }

  function byDayLabelYearly(dateStr: string): string {
    const d = new Date(dateStr + 'T00:00:00');
    return `${byDayLabel(dateStr)} in ${localizedMonth(d.getMonth())}`;
  }

  /**
   * Test whether `date` matches a BYDAY token (e.g. "3TU", "-1MO").
   * Used in getRecurringInstanceForDay for monthly-by-weekday.
   */
  function matchesByDay(byDay: string, date: Date): boolean {
    const m = byDay.match(/^(-?\d+)(MO|TU|WE|TH|FR|SA|SU)$/);
    if (!m) return false;
    const n = parseInt(m[1]);
    const targetWday = WDAY_ABBR.indexOf(m[2] as typeof WDAY_ABBR[number]);
    if (date.getDay() !== targetWday) return false;
    const year = date.getFullYear(), month = date.getMonth(), day = date.getDate();
    if (n > 0) {
      const firstOffset = (targetWday - new Date(year, month, 1).getDay() + 7) % 7;
      return day === 1 + firstOffset + (n - 1) * 7;
    } else {
      const lastDay = new Date(year, month + 1, 0);
      const lastOffset = (lastDay.getDay() - targetWday + 7) % 7;
      return day === lastDay.getDate() - lastOffset;
    }
  }

  function openNewEvent() {
    editingEventId = null;
    const d = selectedSlot?.date ?? currentDate;
    const startMin = selectedSlot?.minutes ?? 9 * 60;
    const endMin   = Math.min(startMin + 60, 23 * 60 + 30);
    const sh = Math.floor(startMin / 60), sm = startMin % 60;
    const eh = Math.floor(endMin / 60),   em = endMin % 60;
    evDate = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    evStartTime = `${String(sh).padStart(2, '0')}:${String(sm).padStart(2, '0')}`;
    evEndTime   = `${String(eh).padStart(2, '0')}:${String(em).padStart(2, '0')}`;
    evTitle = '';
    evLocation = '';
    evDescription = '';
    evCalendarId = categories[0]?.id ?? '';
    evIsAllDay = false;
    evIsOnline = false;
    evMeetingUrl = '';
    evAttendees = [];
    evAttendeeQuery = '';
    evRepeats = false;
    evRecurFreq = 'weekly';
    evRecurInterval = 1;
    evRecurEndDate = '';
    evRecurByDayMode = 'date';
    evAlertMinutes = null;
    showEventModal = true;
  }

  function openEditEvent(event: CalendarEvent) {
    editingEventId = event.id;
    const s = event.start;
    evDate = `${s.getFullYear()}-${String(s.getMonth() + 1).padStart(2, '0')}-${String(s.getDate()).padStart(2, '0')}`;
    evStartTime = `${String(s.getHours()).padStart(2, '0')}:${String(s.getMinutes()).padStart(2, '0')}`;
    const e = event.end;
    evEndTime = `${String(e.getHours()).padStart(2, '0')}:${String(e.getMinutes()).padStart(2, '0')}`;
    evTitle = event.title;
    evLocation = event.location ?? '';
    evDescription = event.description ?? '';
    evCalendarId = event.calendarId;
    evIsAllDay = event.isAllDay;
    evIsOnline = event.isOnlineMeeting ?? false;
    evMeetingUrl = event.meetingUrl ?? '';
    evAttendees = (event.attendees ?? []).map((a) => {
      const key = a.email.toLowerCase();
      const contact = contacts.find((c) =>
        c.email.toLowerCase() === key ||
        (c.emails ?? []).some((e) => e.email.toLowerCase() === key)
      );
      return {
        name: a.name, email: a.email, initials: a.initials, color: a.color,
        role: (a.role === 'optional' ? 'optional' : 'required') as 'required' | 'optional',
        photoUrl: contact?.photoUrl,
      };
    });
    evAttendeeQuery = '';
    const rec = event.recurrence;
    evRepeats = !!rec;
    evRecurFreq = (rec?.freq ?? 'weekly') as 'daily' | 'weekly' | 'monthly' | 'yearly';
    evRecurInterval = rec?.interval ?? 1;
    evRecurEndDate = rec?.endDate ?? '';
    evRecurByDayMode = (rec?.byDay && (evRecurFreq === 'monthly' || evRecurFreq === 'yearly')) ? 'weekday' : 'date';
    evAlertMinutes = event.alertMinutes ?? null;
    selectedEvent = null;
    showEventModal = true;
  }

  function closeEventModal() {
    showEventModal = false;
  }

  $effect(() => {
    if (!showEventModal) return;
    const timer = setTimeout(() => {
      (document.getElementById('ev-title') as HTMLInputElement | null)?.focus();
    }, 0);
    function trapTab(e: KeyboardEvent) {
      if (e.key !== 'Tab' || !evModalEl) return;
      const focusable = Array.from(evModalEl.querySelectorAll<HTMLElement>(
        'button:not([disabled]):not([tabindex="-1"]), input:not([disabled]), select:not([disabled]), textarea:not([disabled])'
      ));
      if (!focusable.length) { e.preventDefault(); return; }
      const first = focusable[0], last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (e.shiftKey) {
        if (!evModalEl.contains(active) || active === first) { e.preventDefault(); last.focus(); }
      } else {
        if (!evModalEl.contains(active) || active === last) { e.preventDefault(); first.focus(); }
      }
    }
    document.addEventListener('keydown', trapTab, true);
    return () => { clearTimeout(timer); document.removeEventListener('keydown', trapTab, true); };
  });

  function saveEvent() {
    if (!evTitle.trim() || !evDate) return;
    const [y, m, d] = evDate.split('-').map(Number);
    const [sh, sm] = evStartTime.split(':').map(Number);
    const [eh, em] = evEndTime.split(':').map(Number);
    const start = new Date(y, m - 1, d, evIsAllDay ? 0 : sh, evIsAllDay ? 0 : sm);
    const end = new Date(y, m - 1, d, evIsAllDay ? 23 : eh, evIsAllDay ? 59 : em);
    const cat = categories.find((c) => c.id === evCalendarId);

    // Auto-set yearly recurrence for birthday/anniversary calendars
    const catName = (cat?.name ?? '').toLowerCase();
    const isBirthdayOrAnniversary = catName.includes('birthday') || catName.includes('anniversary');
    const autoYearly = isBirthdayOrAnniversary && !evRepeats;
    const recurrence = (evRepeats || autoYearly) ? {
      freq: autoYearly ? 'yearly' : evRecurFreq,
      interval: autoYearly ? 1 : evRecurInterval,
      endDate: autoYearly ? undefined : (evRecurEndDate || undefined),
      byDay: (!autoYearly && (evRecurFreq === 'monthly' || evRecurFreq === 'yearly') && evRecurByDayMode === 'weekday') ? deriveByDay(evDate) : undefined,
      exdates: editingEventId ? events.find(e => e.id === editingEventId)?.recurrence?.exdates : undefined,
    } : undefined;

    const ev: CalendarEvent = {
      id: editingEventId ?? crypto.randomUUID(),
      title: evTitle.trim(),
      start,
      end,
      color: cat?.color ?? '#0078d4',
      location: evLocation.trim() || undefined,
      description: evDescription.trim() || undefined,
      isAllDay: evIsAllDay,
      calendarId: evCalendarId,
      calendarName: cat?.name ?? '',
      isOnlineMeeting: evIsOnline || undefined,
      meetingUrl: evIsOnline && evMeetingUrl.trim() ? evMeetingUrl.trim() : undefined,
      attendees: evAttendees.length > 0 ? evAttendees : undefined,
      recurrence,
      alertMinutes: evAlertMinutes ?? undefined,
    };

    onSaveEvent?.(ev);
    showEventModal = false;
  }

  function deleteEvent() {
    if (editingEventId) {
      const ev = events.find(e => e.id === editingEventId);
      if (ev) {
        // Build instance with the date from the edit form
        const [y, m, d] = evDate.split('-').map(Number);
        const instanceEvent = { ...ev, start: new Date(y, m - 1, d) };
        confirmDeleteEvent(instanceEvent);
      }
      return;
    }
  }

  // Reset calListFocusedIndex when leaving cal-list focus
  $effect(() => {
    if (calFocusedPane !== 'cal-sidebar' || calInnerPane !== 'cal-list-inner') calListFocusedIndex = -1;
  });

  export function navigateCalList(key: string): 'at-top' | 'at-bottom' | 'moved' {
    const n = categories.length;
    if (n === 0) return 'moved';
    if (key === 'ArrowUp') {
      if (calListFocusedIndex <= 0) { calListFocusedIndex = -1; return 'at-top'; }
      calListFocusedIndex--;
      return 'moved';
    } else {
      if (calListFocusedIndex >= n - 1) return 'at-bottom';
      calListFocusedIndex = calListFocusedIndex === -1 ? 0 : calListFocusedIndex + 1;
      return 'moved';
    }
  }

  /** Toggle visibility of the calendar item at the current keyboard-focused index. */
  export function toggleFocusedCalListItem(): boolean {
    if (calListFocusedIndex < 0 || calListFocusedIndex >= categories.length) return false;
    categories[calListFocusedIndex].visible = !categories[calListFocusedIndex].visible;
    return true;
  }

  export function navigateMiniCal(key: string): void {
    // Month mode: Left/Right move by month; Up/Down ignored
    if (viewMode === 'month') {
      if (key === 'ArrowLeft' || key === 'ArrowRight') {
        let m = miniCalMonth + (key === 'ArrowLeft' ? -1 : 1);
        let y = miniCalYear;
        if (m < 0) { m = 11; y--; } else if (m > 11) { m = 0; y++; }
        miniCalMonth = m;
        miniCalYear = y;
        const d = new Date(currentDate);
        d.setFullYear(y);
        d.setMonth(m);
        currentDate = d;
      }
      // Up/Down: ignored in month mode
      return;
    }
    // Week mode: Left/Right scroll mini-cal display month only, don't touch currentDate
    if (viewMode === 'week' && (key === 'ArrowLeft' || key === 'ArrowRight')) {
      let m = miniCalMonth + (key === 'ArrowLeft' ? -1 : 1);
      let y = miniCalYear;
      if (m < 0) { m = 11; y--; } else if (m > 11) { m = 0; y++; }
      miniCalMonth = m;
      miniCalYear = y;
      return;
    }
    // Day mode (and week Up/Down): navigate currentDate
    const d = new Date(currentDate);
    if (key === 'ArrowLeft') d.setDate(d.getDate() - 1);
    else if (key === 'ArrowRight') d.setDate(d.getDate() + 1);
    else if (key === 'ArrowUp') d.setDate(d.getDate() - 7);
    else if (key === 'ArrowDown') d.setDate(d.getDate() + 7);
    currentDate = d;
    miniCalMonth = d.getMonth();
    miniCalYear = d.getFullYear();
  }

  function scrollSlotIntoView(minutes: number): void {
    if (!timeGridScrollEl) return;
    const slotTop = (minutes / 60) * HOUR_HEIGHT;
    const slotBottom = slotTop + HOUR_HEIGHT;
    const { scrollTop, clientHeight } = timeGridScrollEl;
    if (slotTop < scrollTop) {
      timeGridScrollEl.scrollTop = slotTop;
    } else if (slotBottom > scrollTop + clientHeight) {
      timeGridScrollEl.scrollTop = slotBottom - clientHeight;
    }
  }

  export function navigateMainCal(key: string): void {
    if (viewMode === 'month') {
      if (key === 'Enter') {
        const first = getMonthEventsForDay(currentDate)[0];
        if (first) selectedEvent = first;
        return;
      }
      const d = new Date(currentDate);
      if (key === 'ArrowLeft') d.setDate(d.getDate() - 1);
      else if (key === 'ArrowRight') d.setDate(d.getDate() + 1);
      else if (key === 'ArrowUp') d.setDate(d.getDate() - 7);
      else if (key === 'ArrowDown') d.setDate(d.getDate() + 7);
      currentDate = d;
      return;
    }
    // Day / Week mode: navigate time slots
    const slotDate = selectedSlot?.date ?? new Date(currentDate);
    const slotMin  = selectedSlot?.minutes ?? 9 * 60;
    if (key === 'Enter') {
      const first = getEventsForDay(slotDate).find(e => {
        const startMin = e.start.getHours() * 60 + e.start.getMinutes();
        const endMin   = e.end.getHours() * 60 + e.end.getMinutes();
        return startMin < slotMin + 30 && endMin > slotMin;
      });
      if (first) selectedEvent = first;
      return;
    }
    if (key === 'ArrowUp') {
      const m = Math.max(0, slotMin - 30);
      selectedSlot = { date: slotDate, minutes: m };
      scrollSlotIntoView(m);
    } else if (key === 'ArrowDown') {
      const m = Math.min(23 * 60 + 30, slotMin + 30);
      selectedSlot = { date: slotDate, minutes: m };
      scrollSlotIntoView(m);
    } else if (key === 'ArrowLeft' || key === 'ArrowRight') {
      const d = new Date(viewMode === 'day' ? currentDate : slotDate);
      d.setDate(d.getDate() + (key === 'ArrowLeft' ? -1 : 1));
      currentDate = d;
      selectedSlot = { date: d, minutes: slotMin };
    }
  }

  export function hasDetailModal(): boolean {
    return !!selectedEvent;
  }

  // React to external request to open new event modal
  $effect(() => {
    if (requestNewEvent && requestNewEvent > 0) {
      openNewEvent();
      onResetNewEvent?.();
    }
  });

  // React to prefilled meeting request (e.g. from Contacts "Meet" button)
  $effect(() => {
    if (prefillMeeting) {
      openNewEvent();
      evTitle = prefillMeeting.title;
      evAttendees = prefillMeeting.attendees;
      evIsOnline = true;
      onResetPrefillMeeting?.();
    }
  });
</script>

<div class="calendar-view">
  <!-- Sidebar -->
  <div class="calendar-sidebar" class:pane-focused={calFocusedPane === 'cal-sidebar'}>
    <!-- Mini Month Calendar -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="mini-cal" onmousedown={() => onFocusPaneRequest?.('cal-mini-inner')}>
      <div class="mini-cal-header">
        <div class="mini-cal-color"></div>
        <span class="mini-cal-title" class:active={calInnerPane === 'cal-mini-inner'}>{formatMonthYear(new Date(miniCalYear, miniCalMonth), locale())}</span>
        <div class="mini-cal-nav">
          <button class="mini-cal-btn" tabindex="-1" onclick={miniCalPrev} data-tooltip={t('calendar.previousMonth')} data-tooltip-position="bottom" aria-label={t('calendar.previousMonth')}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
          </button>
          <button class="mini-cal-btn" tabindex="-1" onclick={miniCalNext} data-tooltip={t('calendar.nextMonth')} data-tooltip-position="bottom" aria-label={t('calendar.nextMonth')}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 6 15 12 9 18"/></svg>
          </button>
        </div>
      </div>
      <div class="mini-cal-weekdays">
        {#each Array.from({ length: 7 }, (_, i) => new Date(2024, 0, 7 + i).toLocaleDateString(locale(), { weekday: 'narrow' })) as day}
          <span class="mini-cal-weekday">{day}</span>
        {/each}
      </div>
      <div class="mini-cal-days" class:week-mode={viewMode === 'week'}>
        {#each miniCalGrid as day}
          <button
            class="mini-cal-day"
            class:other-month={day.getMonth() !== miniCalMonth}
            class:today={isSameDay(day, today)}
            class:selected={viewMode !== 'week' && isSameDay(day, currentDate)}
            class:week-selected={viewMode === 'week' && isInSelectedWeek(day)}
            class:has-event={filteredEvents.some((e) => isSameDay(e.start, day))}
            onclick={() => {
              if (viewMode === 'month') {
                const d = new Date(currentDate);
                d.setFullYear(day.getFullYear());
                d.setMonth(day.getMonth());
                currentDate = d;
                miniCalMonth = day.getMonth();
                miniCalYear = day.getFullYear();
              } else {
                selectMiniCalDay(day);
              }
            }}
          >
            {day.getDate()}
          </button>
        {/each}
      </div>
    </div>

    <!-- My Calendars + Other Calendars -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="cal-list-wrapper">
      <div class="calendar-list" onmousedown={() => onFocusPaneRequest?.('cal-list-inner')}>
        <h3 class="sidebar-section-title">{t('calendar.myCalendars')}</h3>
        {#each categories.filter(c => c.group !== 'other') as cat}
          {@const idx = categories.indexOf(cat)}
          <label class="calendar-item" class:item-focused={calFocusedPane === 'cal-sidebar' && calInnerPane === 'cal-list-inner' && calListFocusedIndex === idx}>
            <button
              type="button"
              class="toggle-switch"
              class:on={cat.visible}
              style="--toggle-color: {cat.color}"
              onclick={() => (cat.visible = !cat.visible)}
              aria-label={t('calendar.toggle', { name: calendarDisplayName(cat) })}
            ><span class="toggle-knob"></span></button>
            <span class="calendar-item-name">{calendarDisplayName(cat)}</span>
          </label>
        {/each}
      </div>

      {#if categories.some(c => c.group === 'other')}
      <div class="calendar-list">
        <h3 class="sidebar-section-title">{t('calendar.otherCalendars')}</h3>
        {#each categories.filter(c => c.group === 'other') as cat}
          {@const idx = categories.indexOf(cat)}
          <label class="calendar-item" class:item-focused={calFocusedPane === 'cal-sidebar' && calInnerPane === 'cal-list-inner' && calListFocusedIndex === idx}>
            <button
              type="button"
              class="toggle-switch"
              class:on={cat.visible}
              style="--toggle-color: {cat.color}"
              onclick={() => (cat.visible = !cat.visible)}
              aria-label={t('calendar.toggle', { name: calendarDisplayName(cat) })}
            ><span class="toggle-knob"></span></button>
            <span class="calendar-item-name">{calendarDisplayName(cat)}</span>
          </label>
        {/each}
      </div>
      {/if}
    </div>
  </div>

  <!-- Main Calendar Area -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="calendar-main" class:pane-focused={calFocusedPane === 'cal-main'} onmousedown={() => onFocusPaneRequest?.('cal-main')}>
    {#if isSearching}
    <!-- Search Results Pane -->
    <div class="search-results-pane">
      <div class="search-list" class:pane-focused={calFocusedPane === 'cal-main'}>
        <div class="search-list-header">
          <div class="cal-header-color"></div>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
          <span>{t('calendar.resultsFor', { query: searchQuery })}</span>
        </div>
        <div class="search-list-scroll" bind:this={searchListScrollEl}>
          {#if searchResults.upcoming.length > 0}
            <div class="search-group-label">{t('calendar.upNext')}</div>
            {#each searchResults.upcoming as event}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div
                class="search-event-item"
                class:active={searchSelectedEvent?.id === event.id}
                tabindex="-1"
                onclick={() => (searchSelectedEvent = event)}
              >
                <div class="search-event-color" style="background: {event.color}"></div>
                <div class="search-event-info">
                  <span class="search-event-title">{event.title}</span>
                  <span class="search-event-time">
                    {event.start.toLocaleDateString(locale(), { weekday: 'short', month: 'short', day: 'numeric' })}
                    {#if !event.isAllDay}
                      — {formatTimeRange(event.start, event.end, locale())}
                    {:else}
                      — {t('calendar.allDay')}
                    {/if}
                  </span>
                  {#if event.location}
                    <span class="search-event-location">{event.location}</span>
                  {/if}
                </div>
                <div class="search-event-badges">
                    <div class="search-hover-actions">
                        <button class="search-hover-btn" tabindex="-1" title={t('calendar.editEvent')} onclick={(e) => { e.stopPropagation(); searchSelectedEvent = event; openEditEvent(event); }}>
                            <svg width="14" height="14" viewBox="0 0 24 24"><path fill="currentColor" d="M13.25 4a.75.75 0 0 1 0 1.5h-7A1.75 1.75 0 0 0 4.5 7.25v10.5c0 .966.784 1.75 1.75 1.75h10.5a1.75 1.75 0 0 0 1.75-1.75v-7a.75.75 0 0 1 1.5 0v7A3.25 3.25 0 0 1 16.75 21H6.25A3.25 3.25 0 0 1 3 17.75V7.25A3.25 3.25 0 0 1 6.25 4zm6.47-.78a.75.75 0 1 1 1.06 1.06L10.59 14.47L9 15l.53-1.59z"/></svg>
                        </button>
                        <button class="search-hover-btn search-hover-delete" tabindex="-1" title={t('calendar.deleteEvent')} onclick={(e) => { e.stopPropagation(); searchSelectedEvent = event; confirmDeleteEvent(event); if (!event.recurrence) searchSelectedEvent = null; }}>
                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4a1 1 0 011-1h4a1 1 0 011 1v2"/></svg>
                        </button>
                    </div>
                    {#if event.recurrence}
                        <svg class="search-event-recur" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-label={t('calendar.recurring')}>
                            <polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/>
                        </svg>
                    {/if}
                </div>
              </div>
            {/each}
          {/if}
          {#if searchResults.past.length > 0}
            <div class="search-group-label">{t('calendar.past')}</div>
            {#each searchResults.past as event}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div
                class="search-event-item"
                class:active={searchSelectedEvent?.id === event.id}
                tabindex="-1"
                onclick={() => (searchSelectedEvent = event)}
              >
                <div class="search-event-color" style="background: {event.color}"></div>
                <div class="search-event-info">
                  <span class="search-event-title">{event.title}</span>
                  <span class="search-event-time">
                    {event.start.toLocaleDateString(locale(), { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' })}
                    {#if !event.isAllDay}
                      — {formatTimeRange(event.start, event.end, locale())}
                    {:else}
                      — {t('calendar.allDay')}
                    {/if}
                  </span>
                  {#if event.location}
                    <span class="search-event-location">{event.location}</span>
                  {/if}
                </div>
                <div class="search-event-badges">
                    <div class="search-hover-actions">
                        <button class="search-hover-btn" tabindex="-1" title={t('calendar.editEvent')} onclick={(e) => { e.stopPropagation(); searchSelectedEvent = event; openEditEvent(event); }}>
                            <svg width="14" height="14" viewBox="0 0 24 24"><path fill="currentColor" d="M13.25 4a.75.75 0 0 1 0 1.5h-7A1.75 1.75 0 0 0 4.5 7.25v10.5c0 .966.784 1.75 1.75 1.75h10.5a1.75 1.75 0 0 0 1.75-1.75v-7a.75.75 0 0 1 1.5 0v7A3.25 3.25 0 0 1 16.75 21H6.25A3.25 3.25 0 0 1 3 17.75V7.25A3.25 3.25 0 0 1 6.25 4zm6.47-.78a.75.75 0 1 1 1.06 1.06L10.59 14.47L9 15l.53-1.59z"/></svg>
                        </button>
                        <button class="search-hover-btn search-hover-delete" tabindex="-1" title={t('calendar.deleteEvent')} onclick={(e) => { e.stopPropagation(); searchSelectedEvent = event; confirmDeleteEvent(event); if (!event.recurrence) searchSelectedEvent = null; }}>
                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4a1 1 0 011-1h4a1 1 0 011 1v2"/></svg>
                        </button>
                        </div>
                        {#if event.recurrence}
                        <svg class="search-event-recur" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-label={t('calendar.recurring')}>
                            <polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/>
                        </svg>
                        {/if}
                    </div>
                </div>
            {/each}
          {/if}
          {#if searchResults.upcoming.length === 0 && searchResults.past.length === 0}
            <div class="search-empty">
              <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="var(--text-tertiary)" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
              <p>{t('calendar.noEventsFound')}</p>
            </div>
          {/if}
        </div>
      </div>
      <div class="search-detail">
        {#if searchSelectedEvent}
          <div class="search-detail-card">
            <div class="search-detail-header">
              <div class="search-detail-color" style="background: {searchSelectedEvent.color}"></div>
              <h2 class="search-detail-title">{searchSelectedEvent.title}</h2>
            </div>
            <div class="search-detail-body">
              <div class="event-detail-row">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
                <span>
                  {#if searchSelectedEvent.isAllDay}
                    {t('calendar.allDay')} — {searchSelectedEvent.start.toLocaleDateString(locale(), { weekday: 'long', month: 'long', day: 'numeric' })}
                  {:else}
                    {searchSelectedEvent.start.toLocaleDateString(locale(), { weekday: 'short', month: 'short', day: 'numeric' })}, {formatTimeRange(searchSelectedEvent.start, searchSelectedEvent.end, locale())}
                  {/if}
                </span>
              </div>
              {#if searchSelectedEvent.recurrence}
                <div class="event-detail-row">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/>
                  </svg>
                  <span>Repeats {searchSelectedEvent.recurrence.freq}{searchSelectedEvent.recurrence.interval > 1 ? ` every ${searchSelectedEvent.recurrence.interval}` : ''}{searchSelectedEvent.recurrence.endDate ? ` until ${searchSelectedEvent.recurrence.endDate}` : ''}</span>
                </div>
              {/if}
              {#if searchSelectedEvent.location}
                <div class="event-detail-row">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0118 0z"/><circle cx="12" cy="10" r="3"/></svg>
                  <span>{searchSelectedEvent.location}</span>
                </div>
              {/if}
              {#if searchSelectedEvent.isOnlineMeeting}
                {@const ended = searchSelectedEvent.end < new Date()}
                <div class="event-detail-row event-detail-link" class:disabled={ended || !searchSelectedEvent.meetingUrl}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M15 10l5-3v10l-5-3z"/><rect x="1" y="6" width="14" height="12" rx="2"/></svg>
                  {#if searchSelectedEvent.meetingUrl && !ended}
                    <a href={searchSelectedEvent.meetingUrl} title={searchSelectedEvent.meetingUrl} onclick={(e) => { e.preventDefault(); shellOpen(searchSelectedEvent!.meetingUrl!); }}>{t('calendar.joinMeetingShortcutJ')}</a>
                  {:else if ended}
                    <span>{t('calendar.meetingEnded')}</span>
                  {:else}
                    <span>{t('calendar.joinMeeting')}</span>
                  {/if}
                </div>
              {/if}
              {#if searchSelectedEvent.calendarName}
                <div class="event-detail-row">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="18" rx="2"/><line x1="3" y1="10" x2="21" y2="10"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="16" y1="2" x2="16" y2="6"/></svg>
                  <span>{searchSelectedEvent.calendarName}</span>
                </div>
              {/if}
              {#if searchSelectedEvent.description}
                <div class="event-detail-desc">{searchSelectedEvent.description}</div>
              {/if}
              {#if searchSelectedEvent.attendees && searchSelectedEvent.attendees.length > 0}
                <div class="event-detail-attendees">
                  <h4>{t('calendar.attendees')}</h4>
                  {#each searchSelectedEvent.attendees as a}
                    {@const photo = contacts.find((c) => { const k = a.email.toLowerCase(); return c.email.toLowerCase() === k || (c.emails ?? []).some((e) => e.email.toLowerCase() === k); })?.photoUrl}
                    <div class="event-attendee">
                      {#if photo}
                        <img class="event-attendee-avatar" src={photo} alt="" />
                      {:else}
                        <span class="event-attendee-avatar" style="background: {a.color}">{a.initials}</span>
                      {/if}
                      <span>{a.name}</span>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        {:else}
          <div class="search-detail-empty">
            <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--text-tertiary)" stroke-width="0.75" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3" y="4" width="18" height="18" rx="2"/><line x1="3" y1="10" x2="21" y2="10"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="16" y1="2" x2="16" y2="6"/>
            </svg>
            <p>{t('calendar.selectEventDetails')}</p>
          </div>
        {/if}
      </div>
    </div>
    {:else}
    <!-- Calendar Header -->
    <div class="calendar-header">
      <div class="cal-header-left">
      <div class="cal-header-color"></div>
        <button class="cal-today-btn" onclick={goToToday} data-tooltip={t('calendar.goToToday')} data-tooltip-position="bottom-start">{t('common.today')}</button>
        <div class="cal-nav-arrows">
          <button class="cal-nav-btn" onclick={() => navigateWeek(-1)} data-tooltip={t('common.previous')} data-tooltip-position="bottom" aria-label={t('common.previous')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
          </button>
          <button class="cal-nav-btn" onclick={() => navigateWeek(1)} data-tooltip={t('common.next')} data-tooltip-position="bottom" aria-label={t('common.next')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 6 15 12 9 18"/></svg>
          </button>
        </div>
        <span class="cal-header-title">{getHeaderLabel()}</span>
      </div>
    </div>

    {#if viewMode === 'month'}
      <!-- Month View -->
      <div class="month-view">
        <div class="month-weekday-headers">
          {#each Array.from({ length: 7 }, (_, i) => new Date(2024, 0, 7 + i).toLocaleDateString(locale(), { weekday: 'long' })) as day}
            <div class="month-weekday-header">{day}</div>
          {/each}
        </div>
        <div class="month-grid">
          {#each getMonthViewGrid() as day, i}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="month-cell"
              class:other-month={day.getMonth() !== currentDate.getMonth()}
              class:today={isSameDay(day, today)}
              class:selected={isSameDay(day, currentDate)}
              onclick={(e) => handleMonthCellClick(e, day)}
            >
              <span class="month-cell-day" class:today-circle={isSameDay(day, today)}>{day.getDate()}</span>
              <div class="month-cell-events">
                {#each getMonthEventsForDay(day).slice(0, 3) as event}
                  <button
                    class="month-event-pill"
                    style="background: {event.color}80; color: var(--text-primary); border-left: 4px solid {event.color};"
                    onclick={() => (selectedEvent = event)}
                    data-tooltip={event.title}
                    data-tooltip-position="bottom"
                  >
                    {#if !event.isAllDay}
                      <span class="month-event-time">{formatTime(event.start, locale())}</span>
                    {/if}
                    <span>{event.title}</span>
                  </button>
                {/each}
                {#if getMonthEventsForDay(day).length > 3}
                  <span class="month-more">{t('calendar.more', { count: getMonthEventsForDay(day).length - 3 })}</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {:else}
      <!-- Day / Week View -->
      <div class="week-view">
        <!-- Day column headers -->
        <div class="week-header">
          <div class="time-gutter-header"></div>
          {#each viewMode === 'day' ? [currentDate] : weekDays as day}
            {@const hdr = formatDayHeader(day, locale())}
            <div class="day-column-header" class:today={isSameDay(day, today)}>
              <span class="day-number">{hdr.day}</span>
              <span class="day-weekday">{hdr.weekday}</span>
            </div>
          {/each}
        </div>

        <!-- All-day events row -->
        <div class="all-day-row">
          <div class="time-gutter-header"></div>
          {#each viewMode === 'day' ? [currentDate] : weekDays as day}
            <div class="all-day-cell">
              {#each getAllDayEventsForDay(day) as event}
                <button
                  class="all-day-event"
                  style="background: {event.color}80; color: var(--text-primary); border-left: 4px solid {event.color};"
                  onclick={() => (selectedEvent = event)}
                  data-tooltip={event.title}
                >
                  {event.title}
                </button>
              {/each}
            </div>
          {/each}
        </div>

        <!-- Time grid -->
        <div class="time-grid-scroll" bind:this={timeGridScrollEl}>
          <div class="time-grid" style="height: {(END_HOUR - START_HOUR) * HOUR_HEIGHT}px">
            <!-- Hour lines -->
            <div class="time-gutter-col">
              {#each hours as hour}
                <div class="time-label" style="top: {hour * HOUR_HEIGHT}px">
                  {#if hour !== 0}
                    {new Date(2024, 0, 1, hour).toLocaleTimeString(locale(), { hour: 'numeric' })}
                  {/if}
                </div>
              {/each}
            </div>

            <div class="day-columns">
              {#each hours as hour}
                <div class="hour-line" style="top: {hour * HOUR_HEIGHT}px"></div>
                <div class="half-hour-line" style="top: {hour * HOUR_HEIGHT + HOUR_HEIGHT / 2}px"></div>
              {/each}

              {#each viewMode === 'day' ? [currentDate] : weekDays as day}
                {@const isWeekday = day.getDay() >= 1 && day.getDay() <= 5}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                  class="day-column"
                  class:today={isSameDay(day, today)}
                  onclick={(e) => handleDayColumnClick(e, day)}
                >
                  {#if viewMode === 'week' && !isWeekday}
                    <div class="non-working-shade" style="top: 0; bottom: 0"></div>
                  {:else if viewMode === 'week' && isWeekday}
                    <div class="non-working-shade" style="top: 0; height: {8 * HOUR_HEIGHT}px"></div>
                    <div class="non-working-shade" style="top: {17 * HOUR_HEIGHT}px; bottom: 0"></div>
                  {/if}
                  {#if selectedSlot && isSameDay(selectedSlot.date, day) && selectedSlot.minutes !== null}
                    <div
                      class="slot-highlight"
                      style="top: {(selectedSlot.minutes / 60) * HOUR_HEIGHT}px; height: {HOUR_HEIGHT}px"
                    ></div>
                  {/if}
                  {#each layoutEventsForDay(getEventsForDay(day)) as { event, column, totalColumns }}
                    <button
                      class="event-block"
                      style={getEventStyle(event, column, totalColumns)}
                      onclick={() => (selectedEvent = event)}
                      data-tooltip={getEventTooltip(event)}
                      data-tooltip-position="right"
                      aria-label={event.title}
                    >
                      <span class="event-title">{event.title}</span>
                      {#if event.location}
                        <span class="event-location">{event.location}</span>
                      {/if}
                    </button>
                  {/each}
                </div>
              {/each}
            </div>

            <!-- Current time indicator -->
            {#if (viewMode === 'day' ? [currentDate] : weekDays).some((d) => isSameDay(d, today))}
              {@const nowMinutes = today.getHours() * 60 + today.getMinutes()}
              {@const dayIndex = (viewMode === 'day' ? [currentDate] : weekDays).findIndex((d) => isSameDay(d, today))}
              {#if dayIndex >= 0}
                <div
                  class="now-dashline"
                  style="top: {(nowMinutes / 60) * HOUR_HEIGHT}px; left: 60px; width: calc({dayIndex} * (100% - 60px) / {viewMode === 'day' ? 1 : 7})"
                ></div>
                <div
                  class="now-indicator"
                  style="top: {(nowMinutes / 60) * HOUR_HEIGHT}px; left: calc(60px + {dayIndex} * (100% - 60px) / {viewMode === 'day' ? 1 : 7}); width: calc((100% - 60px) / {viewMode === 'day' ? 1 : 7})"
                >
                  <div class="now-dot"></div>
                  <div class="now-line"></div>
                </div>
              {/if}
            {/if}
          </div>
        </div>
      </div>
    {/if}
    {/if}
  </div>

  <!-- Event Detail Popup -->
  {#if selectedEvent}
    <div class="event-detail-overlay" onclick={() => (selectedEvent = null)} role="presentation">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="event-detail-card" onclick={(e) => e.stopPropagation()} onkeydown={handleDetailKeydown} role="dialog" tabindex="-1">
        <div class="event-detail-header" style="border-left: 4px solid {selectedEvent.color}">
          <h2 class="event-detail-title">{selectedEvent.title}</h2>
          <button class="event-detail-close" onclick={() => (selectedEvent = null)} data-tooltip={t('common.close')} data-tooltip-position="bottom" aria-label={t('common.close')}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          </button>
        </div>
        <div class="event-detail-body">
          <div class="event-detail-row">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
            <span>
              {#if selectedEvent.isAllDay}
                {t('calendar.allDay')} — {selectedEvent.start.toLocaleDateString(locale(), { weekday: 'long', month: 'long', day: 'numeric' })}
              {:else}
                {selectedEvent.start.toLocaleDateString(locale(), { weekday: 'short', month: 'short', day: 'numeric' })}, {formatTimeRange(selectedEvent.start, selectedEvent.end, locale())}
              {/if}
            </span>
          </div>
          {#if selectedEvent.location}
            <div class="event-detail-row">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0118 0z"/><circle cx="12" cy="10" r="3"/></svg>
              <span>{selectedEvent.location}</span>
            </div>
          {/if}
          {#if selectedEvent.isOnlineMeeting}
            {@const ended = selectedEvent.end < new Date()}
            <div class="event-detail-row event-detail-link" class:disabled={ended || !selectedEvent.meetingUrl}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M15 10l5-3v10l-5-3z"/><rect x="1" y="6" width="14" height="12" rx="2"/></svg>
              {#if selectedEvent.meetingUrl && !ended}
                <a href={selectedEvent.meetingUrl} title={selectedEvent.meetingUrl} onclick={(e) => { e.preventDefault(); shellOpen(selectedEvent!.meetingUrl!); selectedEvent = null; }}>{t('calendar.joinMeetingShortcut')}</a>
              {:else if ended}
                <span>{t('calendar.meetingEnded')}</span>
              {:else}
                <span>{t('calendar.joinMeeting')}</span>
              {/if}
            </div>
          {/if}
          {#if selectedEvent.description}
            <div class="event-detail-desc">{selectedEvent.description}</div>
          {/if}
          {#if selectedEvent.attendees && selectedEvent.attendees.length > 0}
            <div class="event-detail-attendees">
              <h4>{t('calendar.attendees')}</h4>
              {#each selectedEvent.attendees as a}
                {@const photo = contacts.find((c) => { const k = a.email.toLowerCase(); return c.email.toLowerCase() === k || (c.emails ?? []).some((e) => e.email.toLowerCase() === k); })?.photoUrl}
                <div class="event-attendee">
                  {#if photo}
                    <img class="event-attendee-avatar" src={photo} alt="" />
                  {:else}
                    <span class="event-attendee-avatar" style="background: {a.color}">{a.initials}</span>
                  {/if}
                  <span>{a.name}</span>
                </div>
              {/each}
            </div>
          {/if}
          <div class="event-detail-actions">
            <button bind:this={detailEditBtnEl} class="ev-detail-action-btn ev-edit-btn" onclick={() => openEditEvent(selectedEvent!)}>
              <svg width="14" height="14" viewBox="0 0 24 24">
                <path fill="currentColor" d="M13.25 4a.75.75 0 0 1 0 1.5h-7A1.75 1.75 0 0 0 4.5 7.25v10.5c0 .966.784 1.75 1.75 1.75h10.5a1.75 1.75 0 0 0 1.75-1.75v-7a.75.75 0 0 1 1.5 0v7A3.25 3.25 0 0 1 16.75 21H6.25A3.25 3.25 0 0 1 3 17.75V7.25A3.25 3.25 0 0 1 6.25 4zm6.47-.78a.75.75 0 1 1 1.06 1.06L10.59 14.47L9 15l.53-1.59z"/>
              </svg>
              {t('calendar.editBtn')}
            </button>
            <button bind:this={detailDeleteBtnEl} class="ev-detail-action-btn ev-delete-btn" onclick={() => confirmDeleteEvent(selectedEvent!)}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4a1 1 0 011-1h4a1 1 0 011 1v2"/>
              </svg>
              {t('calendar.deleteBtn')}
            </button>
          </div>
        </div>
      </div>
    </div>
  {/if}
</div>

<!-- Event Edit/Create Modal -->
{#if showEventModal}
  <div class="ev-modal-overlay" onclick={closeEventModal} role="presentation">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="ev-modal" bind:this={evModalEl} onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); closeEventModal(); } e.stopPropagation(); }}>
      <div class="ev-modal-header">
        <h2 class="ev-modal-title">{editingEventId ? t('calendar.editEventTitle') : t('calendar.newEventTitle')}</h2>
        <button class="ev-modal-close" tabindex="-1" onclick={closeEventModal} data-tooltip={t('common.close')} data-tooltip-position="bottom" aria-label={t('common.close')}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      <div class="ev-modal-body" bind:this={evModalBodyEl}>
        <div class="ev-field">
          <label class="ev-label" for="ev-title">{t('calendar.title')}</label>
          <input id="ev-title" type="text" class="ev-input" bind:value={evTitle} placeholder={t('calendar.addTitle')} />
        </div>

        <div class="ev-field">
          <label class="ev-label" for="ev-date">{t('calendar.date')}</label>
          <DatePicker id="ev-date" bind:value={evDate} placeholder={t('calendar.pickDate')} />
        </div>

        <div class="ev-row">
          <label class="ev-checkbox-label">
            <button type="button" class="toggle-switch" class:on={evIsAllDay} onclick={() => evIsAllDay = !evIsAllDay} aria-label={t('calendar.toggleAllDay')}><span class="toggle-knob"></span></button>
            {t('calendar.allDay')}
          </label>
        </div>

        {#if !evIsAllDay}
          <div class="ev-time-row">
            <div class="ev-field ev-field-half">
              <label class="ev-label" for="ev-start">{t('calendar.start')}</label>
              <TimePicker id="ev-start" bind:value={evStartTime} />
            </div>
            <div class="ev-field ev-field-half">
              <label class="ev-label" for="ev-end">{t('calendar.end')}</label>
              <TimePicker id="ev-end" bind:value={evEndTime} />
            </div>
          </div>
        {/if}

        <!-- Recurrence -->
        <div class="ev-row">
          <label class="ev-checkbox-label">
            <button type="button" class="toggle-switch" class:on={evRepeats} onclick={() => evRepeats = !evRepeats} aria-label={t('calendar.toggleRepeats')}><span class="toggle-knob"></span></button>
            {t('calendar.repeats')}
          </label>
        </div>
        {#if evRepeats}
          <div class="ev-recur-row">
            <span class="ev-recur-label">{t('calendar.every')}</span>
            <input type="text" inputmode="numeric" class="ev-input ev-input-small"
              value={evRecurInterval}
              onkeydown={(e) => { if (!/^[0-9]$/.test(e.key) && !['Backspace','Delete','ArrowLeft','ArrowRight','Tab','Home','End'].includes(e.key)) e.preventDefault(); }}
              oninput={(e) => { const raw = (e.target as HTMLInputElement).value.replace(/\D/g, '').slice(0, 3); (e.target as HTMLInputElement).value = raw; const v = parseInt(raw); evRecurInterval = v > 0 ? v : evRecurInterval; }}
            />
            <select class="ev-input ev-input-freq" bind:value={evRecurFreq}>
              <option value="daily">{t('calendar.days')}</option>
              <option value="weekly">{t('calendar.weeks')}</option>
              <option value="monthly">{t('calendar.months')}</option>
              <option value="yearly">{t('calendar.years')}</option>
            </select>
          </div>
          {#if (evRecurFreq === 'monthly' || evRecurFreq === 'yearly') && evDate}
            <div class="ev-recur-byday">
              <label class="ev-byday-opt">
                <input type="radio" bind:group={evRecurByDayMode} value="date" />
                {t('calendar.onDay', { day: new Date(evDate + 'T00:00:00').getDate() })}
              </label>
              <label class="ev-byday-opt">
                <input type="radio" bind:group={evRecurByDayMode} value="weekday" />
                On {evRecurFreq === 'yearly' ? byDayLabelYearly(evDate) : byDayLabel(evDate)}
              </label>
            </div>
          {/if}
          <div class="ev-field">
            <label class="ev-label" for="ev-recur-end">{t('calendar.endDateOptional')}</label>
            <DatePicker id="ev-recur-end" bind:value={evRecurEndDate} placeholder={t('calendar.noEndDate')} />
          </div>
        {/if}

        <div class="ev-field">
          <label class="ev-label" for="ev-location">{t('calendar.location')}</label>
          <input id="ev-location" type="text" class="ev-input" bind:value={evLocation} placeholder={t('calendar.addLocation')} />
        </div>

        <div class="ev-row">
          <label class="ev-checkbox-label">
            <button type="button" class="toggle-switch" class:on={evIsOnline} onclick={() => evIsOnline = !evIsOnline} aria-label={t('calendar.toggleOnlineMeeting')}><span class="toggle-knob"></span></button>
            {t('calendar.onlineMeeting')}
          </label>
        </div>
        {#if evIsOnline}
          <div class="ev-field">
            <label class="ev-label" for="ev-meeting-url">{t('calendar.meetingUrl')}</label>
            <input id="ev-meeting-url" class="ev-input" type="url" placeholder="https://..." bind:value={evMeetingUrl} />
          </div>
        {/if}

        <div class="ev-field">
          <label class="ev-label" for="ev-alert">{t('calendar.alert')}</label>
          <select id="ev-alert" class="ev-input" bind:value={evAlertMinutes}>
            {#each ALERT_OPTIONS as opt}
              <option value={opt.value}>{t(opt.key)}</option>
            {/each}
          </select>
        </div>

        <div class="ev-field">
          <label class="ev-label" for="ev-calendar">{t('calendar.calendarLabel')}</label>
          <select id="ev-calendar" class="ev-input" bind:value={evCalendarId}>
            {#each categories as cat}
              <option value={cat.id}>
                {calendarDisplayName(cat)}
              </option>
            {/each}
          </select>
        </div>

        <div class="ev-field">
          <label class="ev-label" for="ev-desc">{t('calendar.description')}</label>
          <textarea id="ev-desc" class="ev-input ev-textarea" bind:value={evDescription} placeholder={t('calendar.addDescription')} rows="3"></textarea>
        </div>

        <!-- Attendees -->
        <div class="ev-field">
          <span class="ev-label">{t('calendar.attendees')}</span>

          {#if evAttendees.length > 0}
            <div class="ev-att-list">
              {#each evAttendees as att}
                <div class="ev-att-chip">
                  <div class="ev-att-chip-name-group">
                    {#if att.photoUrl}
                      <img class="ev-att-chip-avatar" src={att.photoUrl} alt="" />
                    {:else}
                      <span class="ev-att-chip-avatar" style="background:{att.color}">{att.initials}</span>
                    {/if}
                    <span class="ev-att-chip-name">{att.name || att.email}</span>
                    <button class="ev-att-remove-btn" onclick={() => removeAttendee(att.email)} aria-label={t('common.remove')}>×</button>
                  </div>
                  <label class="ev-att-required-toggle">
                    <button type="button" class="toggle-switch toggle-switch-sm" class:on={att.role === 'required'} onclick={() => toggleAttendeeRole(att.email)} aria-label={t('calendar.required')}><span class="toggle-knob"></span></button>
                    <span class="ev-att-required-label">{t('calendar.required')}</span>
                  </label>
                </div>
              {/each}
            </div>
          {/if}

          <div class="ev-att-search-wrap">
            <input
              bind:this={attSearchEl}
              type="text"
              class="ev-input"
              placeholder={t('calendar.searchContacts')}
              bind:value={evAttendeeQuery}
              onfocus={openAttendeeDropdown}
              onblur={() => setTimeout(() => (evAttendeeDropdownOpen = false), 150)}
            />
            {#if evAttendeeDropdownOpen && evAttendeeResults.length > 0}
              <div class="ev-att-dropdown" style={attDropdownStyle}>
                {#each evAttendeeResults as c}
                  <button class="ev-att-option" onmousedown={(e) => { e.preventDefault(); addAttendee(c); }}>
                    {#if c.photoUrl}
                      <img class="ev-att-av" src={c.photoUrl} alt="" />
                    {:else}
                      <span class="ev-att-av" style="background:{c.color}">{c.initials}</span>
                    {/if}
                    <div class="ev-att-info">
                      <span class="ev-att-name">{c.name}</span>
                      <span class="ev-att-email">{c.email}</span>
                    </div>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      </div>
      <div class="ev-modal-footer">
        {#if editingEventId}
          <button class="ev-btn ev-btn-danger" onclick={deleteEvent}>{t('common.delete')}</button>
        {/if}
        <div class="ev-footer-spacer"></div>
        <button class="ev-btn ev-btn-secondary" onclick={closeEventModal}>{t('common.cancel')}</button>
        <button class="ev-btn ev-btn-primary" onclick={saveEvent} disabled={!evTitle.trim()}>{t('common.save')}</button>
      </div>
    </div>
  </div>
{/if}

{#if showDeleteRecurDialog && pendingDeleteEvent}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ev-modal-overlay" onclick={() => { showDeleteRecurDialog = false; pendingDeleteEvent = null; }}
    onkeydown={(e) => {
      e.stopPropagation();
      if (e.key === 'Escape') { showDeleteRecurDialog = false; pendingDeleteEvent = null; return; }
      if (e.key === 'Tab') { e.preventDefault(); return; }
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const btns = [...(e.currentTarget as HTMLElement).querySelectorAll('.ev-delete-recur-dialog button')] as HTMLElement[];
        const idx = btns.indexOf(document.activeElement as HTMLElement);
        const next = e.key === 'ArrowDown' ? (idx + 1) % btns.length : (idx - 1 + btns.length) % btns.length;
        btns[next]?.focus();
      }
    }}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="ev-delete-recur-dialog" onclick={(e) => e.stopPropagation()} use:focusFirst>
      <h3 class="ev-delete-recur-title">{t('calendar.deleteRecurring')}</h3>
      <p class="ev-delete-recur-desc">{t('calendar.deleteRecurringDesc', { title: pendingDeleteEvent.title })}</p>
      <div class="ev-delete-recur-actions">
        <button class="ev-btn ev-btn-secondary" onclick={() => executeRecurDelete('single')}>{t('calendar.thisEventOnly')}</button>
        <button class="ev-btn ev-btn-secondary" onclick={() => executeRecurDelete('future')}>{t('calendar.thisAndFuture')}</button>
        <button class="ev-btn ev-btn-danger" onclick={() => executeRecurDelete('all')}>{t('calendar.allEvents')}</button>
      </div>
      <button class="ev-btn ev-btn-secondary ev-delete-recur-cancel" onclick={() => { showDeleteRecurDialog = false; pendingDeleteEvent = null; }}>{t('common.cancel')}</button>
    </div>
  </div>
{/if}

<style>
  .calendar-view {
    display: flex;
    flex: 1;
    overflow: hidden;
    gap: 4px;
  }

  /* ── Sidebar ── */
  .calendar-sidebar {
    flex-shrink: 0;
    background-color: var(--bg-primary);
    width: 225px;
    gap: 24px;
    display: flex;
    flex-direction: column;
  }

  /* Mini Calendar */
  .mini-cal {
    justify-items: center;
    background-color: var(--bg-secondary);
    padding-bottom: 8px;
    border-radius: 4px;
  }

  .mini-cal-header {
    display: flex;
    align-items: center;
    width: 100%;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .mini-cal-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .mini-cal-title.active {
    color: var(--accent-active);
  }

  .mini-cal-nav {
    display: flex;
    gap: 2px;
  }

  .mini-cal-btn {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
  }

  .mini-cal-btn:hover {
    background: var(--bg-hover);
  }

  .mini-cal-weekdays {
    display: grid;
    grid-template-columns: repeat(7, 0fr);
    text-align: center;
    margin-bottom: 2px;
  }

  .mini-cal-weekday {
    width: 28px;
    font-size: 10px;
    color: var(--text-secondary);
    padding: 2px;
  }

  .mini-cal-days {
    display: grid;
    grid-template-columns: repeat(7, 0fr);
  }

  .mini-cal-day {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    color: var(--text-primary);
    cursor: pointer;
    position: relative;
    isolation: isolate;
    outline: none;
  }

  .mini-cal-day:hover {
    background: var(--bg-hover);
  }

  .mini-cal-day.other-month {
    color: var(--text-tertiary);
  }

  .mini-cal-day.today {
    color: var(--text-on-accent);
    font-weight: 600;
  }

  .mini-cal-day.today::before {
    content: '';
    position: absolute;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--accent);
    z-index: -1;
  }

  .mini-cal-day.selected {
    background: var(--bg-selected, rgba(0, 120, 212, 0.2));
    color: var(--text-primary);
  }

  .mini-cal-day.week-selected {
    background: var(--bg-selected, rgba(0, 120, 212, 0.2));
    color: var(--text-primary);
  }

  .mini-cal-day.week-selected.today {
    color: var(--text-on-accent);
  }

  .mini-cal-day.week-selected.today::before {
    content: '';
    position: absolute;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--accent);
    z-index: -1;
  }

  .mini-cal-day.has-event:not(.today) {
    border-bottom: 2px solid var(--accent);
  }

  .mini-cal-day:focus{
    z-index: 99;
  }

  /* Pane focus indicators */
  .calendar-sidebar.pane-focused .mini-cal-color {
    background-color: var(--accent-active);
  }

  .mini-cal-color {
    width: 4px;
    min-height: 40px;
    flex-shrink: 0;
  }

  .cal-list-wrapper {
    display: block;
    border-left: 2px solid transparent;
    padding: 0 12px;
  }

  .calendar-list:has(.item-focused) .sidebar-section-title {
    color: var(--accent-active);
  }

  .calendar-item.item-focused {
    background: var(--bg-hover);
  }

  /* Calendar list */
  .sidebar-section-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    margin-bottom: 8px;
    letter-spacing: 0.3px;
  }

  .calendar-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .toggle-switch {
    position: relative;
    width: 32px;
    height: 18px;
    border-radius: 9px;
    border: none;
    padding: 0;
    cursor: pointer;
    background: var(--border);
    flex-shrink: 0;
    transition: background 0.15s;
    outline: none;
  }

  .toggle-switch.on {
    background: var(--toggle-color, var(--accent));
  }
  
  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: white;
    transition: transform 0.15s;
    pointer-events: none;
  }
  
  .toggle-switch.on .toggle-knob {
    transform: translateX(14px);
  }

  .calendar-item-name {
    flex: 1;
  }

  /* ── Main Calendar ── */
  .calendar-main {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    border-left: 2px solid transparent;
    background-color: var(--bg-secondary);
    border-radius: 4px;
  }

  .calendar-main.pane-focused .cal-header-color {
    background-color: var(--accent-active);
  }

    .calendar-main.pane-focused .calendar-header {
        color: var(--accent-active);
    }

  .cal-header-color {
    width: 4px;
    min-height: 40px;
    flex-shrink: 0;
  }

  .month-cell.selected {
    background: var(--bg-selected, rgba(0, 120, 212, 0.2));
  }

  .calendar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border-light);
    flex-shrink: 0;
    height: 40px;
    color: var(--text-secondary);
  }

  .cal-header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .cal-today-btn {
    padding: 4px 12px;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 12px;
    font-weight: 600;
    background: var(--bg-primary);
  }

  .cal-today-btn:hover {
    background: var(--bg-hover);
  }

  .cal-nav-arrows {
    display: flex;
    gap: 1px;
  }

  .cal-nav-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
  }

  .cal-nav-btn:hover {
    background: var(--bg-hover);
  }

  .cal-header-title {
    font-size: 14px;
    font-weight: 600;
  }



  /* ── Week / Day View ── */
  .week-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
  }

  .week-header {
    display: flex;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-light);
    align-items: stretch;
    overflow-y: auto;
    scrollbar-gutter: stable;
  }

  .time-gutter-header {
    width: 60px;
    flex-shrink: 0;
  }

  .day-column-header {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 6px 10px;
    gap: 0;
    border-left: 1px solid var(--border-light);
    border-top: 3px solid transparent;
  }

  .day-column-header.today {
    border-top: 3px solid var(--accent-active);
  }

  .day-weekday {
    font-size: 11px;
    color: var(--text-secondary);
    letter-spacing: 0.3px;
  }

  .day-number {
    font-size: 20px;
    font-weight: 600;
    color: var(--accent-light);
    line-height: 1.2;
  }

  .day-column-header.today .day-weekday,
  .day-column-header.today .day-number {
    color: var(--accent-active);
  }

  /* Month view today circle */
  .today-circle {
    background: var(--accent-active);
    color: var(--text-on-accent) !important;
    font-weight: 600 !important;
  }

  /* All-day row */
  .all-day-row {
    display: flex;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-light);
    min-height: 24px;
    overflow-y: auto;
    scrollbar-gutter: stable;
  }

  .all-day-cell {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 2px 4px;
    align-items: center;
    border-left: 1px solid var(--border-light);
    overflow: hidden;
  }

  .all-day-event {
    font-size: 11px;
    padding: 0 6px;
    height: 20px;
    line-height: 20px;
    width: 100%;
    text-align: start;
    border-radius: 3px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }

  .all-day-event:hover {
    filter: brightness(0.95);
  }

  /* Time grid */
  .time-grid-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-gutter: stable;
  }

  .time-grid {
    position: relative;
    min-width: 100%;
  }

  .time-gutter-col {
    position: absolute;
    left: 0;
    top: 0;
    width: 60px;
    height: 100%;
  }

  .time-label {
    position: absolute;
    width: 60px;
    text-align: right;
    padding-right: 8px;
    font-size: 10px;
    color: var(--text-tertiary);
    transform: translateY(-6px);
    user-select: none;
  }

  .day-columns {
    position: absolute;
    left: 60px;
    right: 0;
    top: 0;
    height: 100%;
    display: flex;
  }

  .hour-line {
    position: absolute;
    left: 0;
    right: 0;
    height: 1px;
    background: var(--border-light);
    pointer-events: none;
  }

  .half-hour-line {
    position: absolute;
    left: 0;
    right: 0;
    height: 1px;
    background: var(--border-light);
    opacity: 0.45;
    pointer-events: none;
  }

  .day-column {
    flex: 1;
    position: relative;
    border-left: 1px solid var(--border-light);
    cursor: crosshair;
  }

  .day-column.today {
    background: rgba(0, 120, 212, 0.03);
  }

  .slot-highlight {
    position: absolute;
    left: 0;
    right: 0;
    background: color-mix(in srgb, var(--accent) 40%, transparent);
    border-left: 4px solid var(--accent-active);
    pointer-events: none;
    z-index: 1;
    opacity: 0.7;
  }

  .non-working-shade {
    position: absolute;
    left: 0;
    right: 0;
    background: rgba(0, 0, 0, 0.025);
    pointer-events: none;
    z-index: 0;
  }

  /* Event blocks */
  .event-block {
    position: absolute;
    border-radius: 4px;
    padding: 3px 6px;
    overflow: visible;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 1px;
    z-index: 1;
    transition: filter 0.1s;
    text-align: left;
  }

  .event-block:hover {
    filter: brightness(0.92);
    z-index: 999;
  }

  .event-title {
    font-size: 11px;
    font-weight: 600;
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .event-location {
    font-size: 10px;
    opacity: 0.7;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Now indicator */
  .now-dashline {
    position: absolute;
    z-index: 3;
    pointer-events: none;
    height: 0;
    border-top: 1px dashed var(--accent-light);
  }

  .now-indicator {
    position: absolute;
    z-index: 3;
    pointer-events: none;
  }

  .now-dot {
    position: absolute;
    left: -4px;
    top: -4px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent-active);
  }

  .now-line {
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: var(--accent-active);
  }

  /* ── Month View ── */
  .month-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
  }

  .month-weekday-headers {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    border-bottom: 1px solid var(--border-light);
    flex-shrink: 0;
  }

  .month-weekday-header {
    padding: 6px 8px;
    font-size: 11px;
    color: var(--text-secondary);
    text-align: left;
    letter-spacing: 0.3px;
    border-left: 1px solid var(--border-light);
  }

  .month-grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    grid-template-rows: repeat(6, 1fr);
    flex: 1;
    overflow: hidden;
  }

  .month-cell {
    border-right: 1px solid var(--border-light);
    border-bottom: 1px solid var(--border-light);
    padding: 4px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
    cursor: pointer;
  }

  .month-cell.other-month {
    background: var(--bg-secondary);
  }

  .month-cell.today {
    border: 1px solid var(--accent-active);
  }

  .month-cell-day {
    font-size: 12px;
    color: var(--text-primary);
    text-align: center;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    margin-bottom: 2px;
    flex-shrink: 0;
  }

  .month-cell-events {
    display: flex;
    flex-direction: column;
    gap: 3px;
    overflow: hidden;
    flex: 1;
  }

  .month-event-pill {
    font-size: 10px;
    padding: 2px 4px;
    border-radius: 2px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
    text-align: left;
  }

  .month-event-pill:hover {
    filter: brightness(0.92);
  }

  .month-event-time {
    font-weight: 600;
    flex-shrink: 0;
  }

  .month-more {
    font-size: 10px;
    color: var(--text-tertiary);
    padding: 1px 4px;
  }

  /* ── Event Detail Overlay ── */
  .event-detail-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.2);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .event-detail-card {
    background: var(--bg-primary);
    border-radius: 8px;
    box-shadow: var(--shadow-lg);
    width: 360px;
    max-height: 80vh;
    overflow-y: auto;
    z-index: 1001;
  }

  .event-detail-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 8px 16px;
    gap: 12px;
  }

  .event-detail-title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    flex: 1;
    line-height: 1.3;
  }

  .event-detail-close {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .event-detail-close:hover {
    background: var(--bg-hover);
  }

  .event-detail-body {
    padding: 0 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .event-detail-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .event-detail-row svg {
    flex-shrink: 0;
  }

  .event-detail-link {
    color: var(--accent);
    cursor: pointer;
  }

  .event-detail-link a {
    color: inherit;
    text-decoration: none;
  }

  .event-detail-link:hover a {
    text-decoration: underline;
  }

  .event-detail-link.disabled {
    color: var(--text-tertiary);
    cursor: default;
    opacity: 0.6;
  }

  .event-detail-link.disabled:hover {
    text-decoration: none;
  }

  .event-detail-desc {
    font-size: 13px;
    color: var(--text-primary);
    line-height: 1.5;
    padding-top: 8px;
    border-top: 1px solid var(--border-light);
    margin-top: 4px;
  }

  .event-detail-attendees {
    padding-top: 8px;
    border-top: 1px solid var(--border-light);
    margin-top: 4px;
  }

  .event-detail-attendees h4 {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    margin-bottom: 6px;
  }

  .event-attendee {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
    font-size: 13px;
    color: var(--text-primary);
  }

  .event-attendee-avatar {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
    object-fit: cover;
  }

  /* ── Event Detail Actions ── */
  .event-detail-actions {
    display: flex;
    gap: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--border-light);
    margin-top: 4px;
  }

  .ev-detail-action-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.1s;
    outline: none;
  }

  .ev-edit-btn {
    background: var(--accent);
  }

  .ev-edit-btn:hover {
    color: #000;
    background: var(--accent-hover);
  }

  .ev-edit-btn:focus {
    color: #000;
    background: var(--accent-active);
  }

  .ev-delete-btn {
    border: 1px solid var(--danger, #d13438);
    color: var(--danger, #d13438);
    background: transparent;
  }

  .ev-delete-btn:hover {
    background: rgba(var(--danger-rgb, 209, 52, 56), 0.2);
  }
  
  .ev-delete-btn:focus {
    background: rgba(var(--danger-rgb, 209, 52, 56), 0.3);
  }

  /* ── Event Edit/Create Modal ── */
  .ev-modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .ev-modal {
    background: var(--bg-primary);
    border-radius: 8px;
    box-shadow: var(--shadow-lg, 0 8px 32px rgba(0, 0, 0, 0.18));
    width: 440px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .ev-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px 12px;
    flex-shrink: 0;
  }

  .ev-modal-title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .ev-modal-close {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .ev-modal-close:hover {
    background: var(--bg-hover);
  }

  .ev-modal-body {
    padding: 0 20px 16px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .ev-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ev-field-half {
    flex: 1;
  }

  .ev-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .ev-input {
    padding: 7px 10px;
    font-size: 13px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-primary);
    color: var(--text-primary);
    transition: border-color 0.15s;
  }

  .ev-input:focus {
    outline: none;
    border-color: var(--accent-active);
  }

  .ev-textarea {
    resize: vertical;
    min-height: 60px;
    font-family: inherit;
  }

  .ev-time-row {
    display: flex;
    gap: 12px;
  }

  .ev-row {
    display: flex;
    align-items: center;
  }

  .ev-checkbox-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .ev-checkbox-label .toggle-switch {
    --toggle-color: var(--accent);
    outline: none;
  }

  .ev-checkbox-label .toggle-switch:focus {
    border: 1px solid var(--accent-active);
  }

  .ev-modal-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--border-light);
    flex-shrink: 0;
  }

  .ev-footer-spacer {
    flex: 1;
  }

  .ev-btn {
    padding: 6px 16px;
    border-radius: 4px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.1s;
  }

  .ev-btn:focus {
    outline: none;
    border: 1px solid var(--accent-active);
  }

  .ev-btn-primary {
    background: var(--accent);
    color: var(--text-primary);
  }

  .ev-btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .ev-btn-primary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .ev-btn-secondary {
    background: transparent;
    color: var(--text-primary);
    border: 1px solid var(--border);
  }

  .ev-btn-secondary:hover {
    background: var(--bg-hover);
  }

  .ev-btn-danger {
    background: transparent;
    color: var(--danger, #d13438);
    border: 1px solid var(--danger, #d13438);
  }

  .ev-btn-danger:hover {
    background: rgba(209, 52, 56, 0.08);
  }

  /* ── Attendees ── */
  .ev-att-search-wrap {
    position: relative;
  }

  .ev-att-search-wrap .ev-input {
    width: 100%;
    box-sizing: border-box;
  }

  .ev-att-dropdown {
    position: fixed;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: var(--shadow-lg);
    z-index: 2000;
    overflow: hidden;
  }

  .ev-att-option {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    text-align: left;
    cursor: pointer;
    color: var(--text-primary);
  }

  .ev-att-option:hover {
    background: var(--bg-hover);
  }

  .ev-att-av {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
    object-fit: cover;
  }

  .ev-att-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .ev-att-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .ev-att-email {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ev-att-list {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin: 4px 0;
  }

  .ev-att-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
  }

  .ev-att-chip-avatar {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
    object-fit: cover;
  }

  .ev-att-chip-name {
    flex: 1;
    font-size: 12px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  
  .ev-att-chip-name-group {
    display: flex;
    align-items: center;
    width: 100%;
    gap: 6px;
    padding: 4px 8px;
    border-radius: 6px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-light);
  }

  .ev-att-required-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-left: 4px;
    flex-shrink: 0;
  }

  .ev-att-required-label {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .toggle-switch-sm {
    width: 26px;
    height: 14px;
    border-radius: 7px;
  }
  .toggle-switch-sm .toggle-knob {
    width: 10px;
    height: 10px;
    top: 2px;
    left: 2px;
  }
  .toggle-switch-sm.on .toggle-knob {
    transform: translateX(12px);
  }

  .ev-att-remove-btn {
    font-size: 14px;
    line-height: 1;
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    color: var(--text-secondary);
    cursor: pointer;
    flex-shrink: 0;
  }

  .ev-att-remove-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* ── Recurrence ── */
  .ev-recur-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .ev-recur-label {
    font-size: 13px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .ev-input-small {
    width: 64px;
    text-align: center;
  }

  .ev-input-freq {
    flex: 1;
  }

  .ev-recur-byday {
    display: flex;
    gap: 24px;
    padding: 4px 0 2px 2px;
  }

  .ev-byday-opt {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .ev-byday-opt input[type="radio"] {
    -webkit-appearance: none;
    appearance: none;
    width: 18px;
    height: 18px;
    border: 2px solid var(--text-secondary);
    border-radius: 50%;
    margin: 0;
    cursor: pointer;
    position: relative;
    flex-shrink: 0;
    transition: border-color 0.15s;
  }
  .ev-byday-opt input[type="radio"]:checked {
    border-color: var(--accent);
  }
  .ev-byday-opt input[type="radio"]:checked::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent-active);
  }

  /* ── Search Results Pane ── */
  .search-results-pane {
    display: flex;
    flex: 1;
    overflow: hidden;
    gap: 2px;
    background-color: var(--border-light);
  }

  .search-list {
    width: 320px;
    min-width: 260px;
    display: flex;
    flex-direction: column;
    background: var(--bg-primary);
  }

  .search-list-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .search-list-header svg {
    flex-shrink: 0;
    color: var(--text-tertiary);
  }

  .search-list-scroll {
    flex: 1;
    overflow-y: auto;
  }

  .search-group-label {
    padding: 10px 16px 4px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-tertiary);
  }

  .search-event-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    cursor: pointer;
    border-bottom: 1px solid var(--border-light, var(--border));
    transition: background 0.1s;
    outline: none;
  }

  .search-event-item.active {
    background: var(--accent-bg, color-mix(in srgb, var(--accent) 25%, transparent));
  }

  .search-event-color {
    width: 4px;
    min-height: 60px;
    flex-shrink: 0;
    opacity: 0.4;
  }

  .search-event-item.active .search-event-color {
    opacity: 1;
  }

  .search-event-item:hover {
    background-color: var(--bg-hover);
  }

  .search-event-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .search-event-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .search-event-time {
    font-size: 11px;
    color: var(--text-secondary);
  }

  .search-event-location {
    font-size: 11px;
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .search-event-badges {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    align-content: flex-start;
    gap: 4px;
    padding-right: 12px;
  }

  .search-event-recur {
    flex-shrink: 0;
    color: var(--text-tertiary);
  }

  .search-hover-actions {
    display: flex;
    align-self: flex-start;
    gap: 2px;
    flex-shrink: 0;
  }

  .search-hover-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 4px;
    color: var(--text-tertiary);
    opacity: 0;
    pointer-events: none;
    transition: background 0.1s, color 0.1s, opacity 0.1s;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .search-event-item:hover .search-hover-btn,
  .search-event-item.active .search-hover-btn {
    opacity: 1;
    pointer-events: auto;
  }

  .search-hover-btn:hover,
  .search-hover-btn:focus {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .search-hover-delete:hover,
  .search-hover-delete:focus {
    color: var(--danger, #d13438);
  }

  .search-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 16px;
    gap: 12px;
    color: var(--text-tertiary);
    font-size: 13px;
  }

  .search-detail {
    flex: 1;
    overflow-y: auto;
    background: var(--bg-primary);
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }

  .search-detail-color {
    width: 4px;
    min-height: 50px;
    flex-shrink: 0;
    opacity: 1;    
  }

  .search-detail-card {
    width: 100%;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    overflow: hidden;
  }

  .search-detail-header {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }

  .search-detail-title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin-left: 20px;
  }

  .search-detail-body {
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .search-detail-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 16px;
    color: var(--text-tertiary);
    font-size: 14px;
  }

  /* ── Delete recurring dialog ── */
  .ev-delete-recur-dialog {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow-lg);
    padding: 20px;
    max-width: 360px;
    width: 90%;
  }

  .ev-delete-recur-title {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .ev-delete-recur-desc {
    margin: 0 0 16px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .ev-delete-recur-actions {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 10px;
  }

  .ev-delete-recur-cancel {
    width: 100%;
  }
</style>
