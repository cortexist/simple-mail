<script lang="ts">
  import TitleBar from '$lib/components/TitleBar.svelte';
  import CommandBar from '$lib/components/CommandBar.svelte';
  import NavigationRail from '$lib/components/NavigationRail.svelte';
  import FolderPane from '$lib/components/FolderPane.svelte';
  import MessageList from '$lib/components/MessageList.svelte';
  import ReadingPane from '$lib/components/ReadingPane.svelte';
  import ComposePane from '$lib/components/ComposePane.svelte';
  import CalendarView from '$lib/components/CalendarView.svelte';
  import ContactsView from '$lib/components/ContactsView.svelte';
  import SettingsView from '$lib/components/SettingsView.svelte';
  import FolderModal from '$lib/components/FolderModal.svelte';
  import {
    loadAccounts,
    loadTheme,
    saveTheme,
    loadAccentColor,
    saveAccentColor,
    loadLocale,
    saveLocale,
    updateAccount as persistUpdateAccount,
    addAccount as persistAddAccount,
    deleteAccount as persistDeleteAccount,
    setAccountPositions as persistAccountPositions,
    saveMailSettings,
    saveCalDavSettings,
    saveCardDavSettings,
    markEmailRead,
    updateEmailStarred,
    updateEmailPinned,
    updateEmailFocused,
    deleteEmail,
    emptyFolder,
    moveEmail,
    syncMail,
    syncCalendars,
    syncContacts,
    sendEmail,
    updateEmailReplied,
    fetchEmailBody,
    fetchPreviewsAround,
    openAttachment,
    flushOutbox,
    saveCalendarEvent,
    deleteCalendarEvent,
    saveContactEntry,
    deleteContactEntry,
    getContactLists,
    saveContactList,
    deleteContactList,
    addToSenderBlocklist,
    removeFromSenderBlocklist,
    saveAttachment,
    getOfflineDownloadStatus,
    addIgnoredAddress,
    removeIgnoredAddress,
    getIgnoredAddresses,
    type OutboundEmail,
  } from '$lib/data/dataService';
  import { open as shellOpen } from '@tauri-apps/plugin-shell';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { save as dialogSave } from '@tauri-apps/plugin-dialog';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, onDestroy, untrack } from 'svelte';
  import { requestNotificationPermission, rebuildAlertQueue, clearAlertQueue } from '$lib/eventAlerts';
  import { networkActivity } from '$lib/networkActivity.svelte';
  import type { Email, NavItem, CalendarViewMode, ComposeMode, Theme, Account, CalendarEvent, FullContact, ComposeDraft, ContactList } from '$lib/types';
  import { parseSearchQuery, emailMatchesQuery, buildSearchText } from '$lib/searchQuery';
  import { setLocale, detectLocale, locale, LANGUAGE_NAMES, t } from '$lib/i18n/index.svelte';

  // ── Undo system ──────────────────────────────────────────
  type MailSingleUndo =
    | { view: 'mail'; action: 'markRead';      emailId: string; prevIsRead: boolean }
    | { view: 'mail'; action: 'markUnread';     emailId: string; prevIsRead: boolean }
    | { view: 'mail'; action: 'delete';         email: Email; prevFolder: string }
    | { view: 'mail'; action: 'move';           emailId: string; prevFolder: string }
    | { view: 'mail'; action: 'archive';        emailId: string; prevFolder: string }
    | { view: 'mail'; action: 'junk';           emailId: string; prevFolder: string }
    | { view: 'mail'; action: 'toggleStar';     emailId: string; prevIsStarred: boolean }
    | { view: 'mail'; action: 'togglePin';      emailId: string; prevIsPinned: boolean }
    | { view: 'mail'; action: 'toggleFocused';  emailId: string; prevIsFocused: boolean };

  type UndoEntry =
    | MailSingleUndo
    | { view: 'mail'; action: 'batch'; entries: MailSingleUndo[] }
    | { view: 'calendar'; action: 'editEvent';   prevEvent: CalendarEvent }
    | { view: 'calendar'; action: 'deleteEvent'; prevEvent: CalendarEvent }
    | { view: 'calendar'; action: 'createEvent'; eventId: string }
    | { view: 'contacts'; action: 'editContact';   prevContact: FullContact }
    | { view: 'contacts'; action: 'deleteContact'; prevContact: FullContact }
    | { view: 'contacts'; action: 'createContact'; contactId: string };

  let activeNav = $state<NavItem>('mail');
  let activeAccountId = $state('');
  let activeFolder = $state('inbox');
  let searchQuery = $state('');
  let folderPaneVisible = $state(true);
  let calendarViewMode = $state<CalendarViewMode>('week');
  let composeMode = $state<ComposeMode | null>(null);
  let composeReplyTo = $state<Email | null>(null);
  let composeDraft = $state<ComposeDraft | null>(null);
  let activeDraftId = $state<string | null>(null);
  let dataReady = $state(false);
  let requireAccountSetup = $state(false);
  let startupError = $state<string | null>(null);
  let startupStatus = $state<'loading' | 'ready' | 'needs-account'>('loading');

  // Settings state
  let showSettings = $state(false);
  let theme = $state<Theme>('system');
  let accentColor = $state('blue');
  let accounts = $state<Account[]>([]);

  // Sync state
  let syncing = $state(false);
  let syncErrors = $state<string[]>([]);
  let syncTimerId: ReturnType<typeof setInterval> | null = null;
  let unlistenMailSent: (() => void) | null = null;
  let unlistenOffline: (() => void) | null = null;

  // Search-coverage indicator for the title bar. Populated per active account;
  // null when the account is fully indexed (or we haven't loaded yet).
  let offlineStatus = $state<{ enabled: boolean; totalCount: number; pendingCount: number } | null>(null);

  // Ignored (muted) senders — global, app-wide. Mail list, reading pane, and
  // Contacts view all filter by this set. Lowercased email strings.
  let mutedAddresses = $state<Set<string>>(new Set());

  async function refreshMutedAddresses() {
    try {
      const list = await getIgnoredAddresses();
      mutedAddresses = new Set(list.map((a) => a.toLowerCase()));
    } catch (err) {
      console.error('Failed to read ignored addresses:', err);
    }
  }

  async function toggleMuteAddress(email: string) {
    const key = email.toLowerCase();
    if (mutedAddresses.has(key)) {
      await removeIgnoredAddress(key);
      const next = new Set(mutedAddresses);
      next.delete(key);
      mutedAddresses = next;
    } else {
      await addIgnoredAddress(key);
      mutedAddresses = new Set(mutedAddresses).add(key);
    }
  }

  async function refreshOfflineStatus() {
    if (!activeAccountId) { offlineStatus = null; return; }
    try {
      offlineStatus = await getOfflineDownloadStatus(activeAccountId);
    } catch (err) {
      console.error('Failed to read offline-download status:', err);
      offlineStatus = null;
    }
  }

  // Throttle re-query from the event stream — batches can fire every few
  // hundred ms during active download, we only need ~1 refresh/sec for the UI.
  let offlineStatusTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleOfflineStatusRefresh() {
    if (offlineStatusTimer) return;
    offlineStatusTimer = setTimeout(() => {
      offlineStatusTimer = null;
      refreshOfflineStatus();
    }, 1000);
  }

  // Refresh the offline status whenever the active account changes.
  $effect(() => {
    // Read the id so the effect depends on it.
    activeAccountId;
    refreshOfflineStatus();
  });

  /** Message for the title-bar info callout, or null to hide the icon. */
  let searchInfoText = $derived.by<string | null>(() => {
    if (activeNav !== 'mail') return null;
    if (!offlineStatus) return null;
    if (offlineStatus.totalCount === 0) return null;
    if (!offlineStatus.enabled) {
      return t('titleBar.searchInfoDisabled');
    }
    if (offlineStatus.pendingCount > 0) {
      const indexed = offlineStatus.totalCount - offlineStatus.pendingCount;
      return t('titleBar.searchInfoPending', {
        indexed: String(indexed),
        total: String(offlineStatus.totalCount),
      });
    }
    return null;
  });
  let lastSyncTime = new Map<string, number>(); // account id → last sync timestamp

  // Undo state (single-level per view)
  let undoMail = $state<UndoEntry | null>(null);
  let undoCalendar = $state<UndoEntry | null>(null);
  let undoContacts = $state<UndoEntry | null>(null);
  let toastMessage = $state<string | null>(null);
  let toastTimerId: ReturnType<typeof setTimeout> | null = null;

  function pushUndo(entry: UndoEntry, message: string) {
    if (entry.view === 'mail')     undoMail = entry;
    if (entry.view === 'calendar') undoCalendar = entry;
    if (entry.view === 'contacts') undoContacts = entry;
    if (toastTimerId) clearTimeout(toastTimerId);
    toastMessage = message;
    toastTimerId = setTimeout(() => { toastMessage = null; toastTimerId = null; }, 5000);
  }

  function dismissToast() {
    if (toastTimerId) clearTimeout(toastTimerId);
    toastMessage = null;
    toastTimerId = null;
  }

  function applyMailUndo(entry: MailSingleUndo) {
    switch (entry.action) {
      case 'markRead':
      case 'markUnread':
        emails = emails.map(e => e.id === entry.emailId ? { ...e, isRead: entry.prevIsRead } : e);
        markEmailRead(activeAccountId, entry.emailId, entry.prevIsRead);
        break;
      case 'delete':
        emails = emails.map(e => e.id === entry.email.id ? { ...e, folder: entry.prevFolder } : e);
        moveEmail(activeAccountId, entry.email.id, entry.prevFolder);
        break;
      case 'move':
      case 'archive':
      case 'junk':
        emails = emails.map(e => e.id === entry.emailId ? { ...e, folder: entry.prevFolder } : e);
        moveEmail(activeAccountId, entry.emailId, entry.prevFolder);
        break;
      case 'toggleStar':
        emails = emails.map(e => e.id === entry.emailId ? { ...e, isStarred: entry.prevIsStarred } : e);
        updateEmailStarred(activeAccountId, entry.emailId, entry.prevIsStarred);
        break;
      case 'togglePin':
        emails = emails.map(e => e.id === entry.emailId ? { ...e, isPinned: entry.prevIsPinned } : e);
        updateEmailPinned(entry.emailId, entry.prevIsPinned);
        break;
      case 'toggleFocused':
        emails = emails.map(e => e.id === entry.emailId ? { ...e, isFocused: entry.prevIsFocused } : e);
        updateEmailFocused(entry.emailId, entry.prevIsFocused);
        break;
    }
  }

  async function performUndo() {
    let entry: UndoEntry | null = null;
    if (activeNav === 'mail')     { entry = undoMail;     undoMail = null; }
    if (activeNav === 'calendar') { entry = undoCalendar;  undoCalendar = null; }
    if (activeNav === 'contacts') { entry = undoContacts;  undoContacts = null; }
    if (!entry) return;
    dismissToast();

    switch (entry.action) {
      // ── Mail ──
      case 'markRead':
      case 'markUnread':
      case 'delete':
      case 'move':
      case 'archive':
      case 'junk':
      case 'toggleStar':
      case 'togglePin':
      case 'toggleFocused':
        applyMailUndo(entry);
        break;
      case 'batch':
        // Undo in reverse order so dependencies unwind cleanly
        for (let i = entry.entries.length - 1; i >= 0; i--) {
          applyMailUndo(entry.entries[i]);
        }
        break;
      // ── Calendar ──
      case 'editEvent': {
        const prev = entry.prevEvent;
        accounts = accounts.map(a => a.id === activeAccountId
          ? { ...a, calendarEvents: a.calendarEvents.map(e => e.id === prev.id ? prev : e) }
          : a);
        saveCalendarEvent(activeAccountId, prev.id, prev.title,
          prev.start instanceof Date ? prev.start.toISOString() : String(prev.start),
          prev.end instanceof Date ? prev.end.toISOString() : String(prev.end),
          prev.location, prev.description, prev.isAllDay, prev.calendarId,
          prev.attendees ?? [], prev.recurrence, prev.isOnlineMeeting ?? false, prev.meetingUrl, prev.alertMinutes);
        break;
      }
      case 'deleteEvent': {
        const prev = entry.prevEvent;
        accounts = accounts.map(a => a.id === activeAccountId
          ? { ...a, calendarEvents: [...a.calendarEvents, prev] }
          : a);
        saveCalendarEvent(activeAccountId, prev.id, prev.title,
          prev.start instanceof Date ? prev.start.toISOString() : String(prev.start),
          prev.end instanceof Date ? prev.end.toISOString() : String(prev.end),
          prev.location, prev.description, prev.isAllDay, prev.calendarId,
          prev.attendees ?? [], prev.recurrence, prev.isOnlineMeeting ?? false, prev.meetingUrl, prev.alertMinutes);
        break;
      }
      case 'createEvent':
        accounts = accounts.map(a => a.id === activeAccountId
          ? { ...a, calendarEvents: a.calendarEvents.filter(e => e.id !== entry.eventId) }
          : a);
        deleteCalendarEvent(activeAccountId, entry.eventId);
        break;
      // ── Contacts ──
      case 'editContact': {
        const prev = entry.prevContact;
        accounts = accounts.map(a => a.id === activeAccountId
          ? { ...a, contacts: a.contacts.map(c => c.id === prev.id ? prev : c) }
          : a);
        saveContactEntry(activeAccountId, prev);
        break;
      }
      case 'deleteContact': {
        const prev = entry.prevContact;
        accounts = accounts.map(a => a.id === activeAccountId
          ? { ...a, contacts: [...a.contacts, prev] }
          : a);
        saveContactEntry(activeAccountId, prev);
        break;
      }
      case 'createContact':
        accounts = accounts.map(a => a.id === activeAccountId
          ? { ...a, contacts: a.contacts.filter(c => c.id !== entry.contactId) }
          : a);
        deleteContactEntry(activeAccountId, entry.contactId);
        break;
    }
  }

  const ACCENT_PRESETS: Record<string, { light: { accent: string; hover: string; active: string; light: string; selected: string }; dark: { accent: string; hover: string; active: string; light: string; selected: string } }> = {
    blue:      { light: { accent: '#0078d4', hover: '#106ebe', active: '#005a9e', light: '#9ab1cb', selected: '#c6d8e7' }, dark: { accent: '#247fae', hover: '#4fa9d2', active: '#2eb8ff', light: '#2c6581', selected: '#1a3a4a' } },
    green:     { light: { accent: '#35a37d', hover: '#588b71', active: '#177651', light: '#81ad81', selected: '#ccd5cc' }, dark: { accent: '#218564', hover: '#218362', active: '#5bd4a5', light: '#45966c', selected: '#0e402f' } },
    purple:    { light: { accent: '#6b69d6', hover: '#5a58c4', active: '#4a48b2', light: '#8c8cb9', selected: '#d7d7f0' }, dark: { accent: '#8786d6', hover: '#acabf0', active: '#9998e8', light: '#52527f', selected: '#3d3d8e' } },
    gold:      { light: { accent: '#d29725', hover: '#a98900', active: '#b3862d', light: '#cdbf89', selected: '#f9f3e0' }, dark: { accent: '#b08e1f', hover: '#b6992f', active: '#ffcb13', light: '#7c6526', selected: '#33290d' } },
    magenta:   { light: { accent: '#d0489d', hover: '#a20d6a', active: '#870054', light: '#c992b4', selected: '#f9e8f2' }, dark: { accent: '#d53b80', hover: '#f25da8', active: '#ff75b3', light: '#812355', selected: '#3a0d25' } },
  };

  // Detect OS color-scheme preference
  let osDark = $state(typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches);

  $effect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const handler = (e: MediaQueryListEvent) => { osDark = e.matches; };
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  });

  // Resolve 'system' theme to actual light/dark
  let resolvedTheme = $derived(theme === 'system' ? (osDark ? 'dark' : 'light') : theme);

  // Apply theme + accent color to document
  $effect(() => {
    document.documentElement.setAttribute('data-theme', resolvedTheme);
    const preset = ACCENT_PRESETS[accentColor];
    if (preset) {
      const vars = resolvedTheme === 'dark' ? preset.dark : preset.light;
      const el = document.documentElement.style;
      el.setProperty('--accent', vars.accent);
      el.setProperty('--accent-hover', vars.hover);
      el.setProperty('--accent-active', vars.active);
      el.setProperty('--accent-light', vars.light);
      el.setProperty('--bg-selected', vars.selected);
      el.setProperty('--bg-compose-btn', vars.accent);
    }
  });

  // Load data from SQLite on mount
  let globalKeyHandler: ((e: KeyboardEvent) => void) | null = null;

  onMount(async () => {
    requestNotificationPermission();
    refreshMutedAddresses();

    // Background offline-download worker emits batches of newly-downloaded
    // bodies. Merge them into the visible emails when they belong to the
    // active account (bodies for other accounts live in SQLite and re-hydrate
    // with searchText when the user switches to them).
    unlistenOffline = await listen<{
      accountId: string;
      updates: Array<{ id: string; body: string; preview: string; hasAttachment: boolean; authResults: string }>;
    }>('offline-bodies-updated', (event) => {
      const { accountId, updates } = event.payload;
      // Always refresh the search-coverage indicator — even for inactive
      // accounts, in case the user switches back while downloads continue.
      if (accountId === activeAccountId) scheduleOfflineStatusRefresh();

      if (accountId !== activeAccountId) return;
      const byId = new Map(updates.map((u) => [u.id, u]));
      emails = emails.map((e) => {
        const u = byId.get(e.id);
        if (!u) return e;
        return {
          ...e,
          body: u.body,
          searchText: buildSearchText(u.body),
          preview: u.preview || e.preview,
          authResults: u.authResults || e.authResults,
          hasAttachment: u.hasAttachment,
        };
      });
    });

    globalKeyHandler = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const target = e.target as HTMLElement;
      const isEditing = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA' || target?.isContentEditable;

      // ── Don't intercept shortcuts when a modal/overlay/dialog is open ──
      if (showSettings || requireAccountSetup || folderModal) return;
      if (target instanceof Element && target.closest('[role="alertdialog"]')) return;
      if (target instanceof Element && target.closest('.ev-modal')) return;
      if (document.querySelector('.ev-delete-recur-dialog')) return;
      if (target instanceof Element && target.closest('.ct-modal')) return;

      // ── Compose-pane shortcuts (when composing) ──
      // Ctrl+S, Ctrl+Shift+S, Escape are handled by ComposePane's own keydown handler
      if (composeMode && activeDraftId) {
        if (mod && e.key === 'Enter') { e.preventDefault(); triggerComposeSend(); return; }
        if (e.key === 'Escape' || (mod && e.key === 's')) return; // let ComposePane handle
      }

      // ── Escape: clear search (all views; Calendar result pane auto-dismisses) ──
      if (e.key === 'Escape' && searchQuery.trim()) {
        e.preventDefault();
        searchQuery = '';
        return;
      }

      // ── Escape: clear multi-select checkboxes ──
      if (e.key === 'Escape' && multiSelectCount > 0) {
        e.preventDefault();
        checkedEmailIds = new Set();
        return;
      }

      // ── Block Ctrl+R/Ctrl+Shift+R webview reload (let mail reply handler consume it instead) ──
      if (mod && e.key === 'r') {
        if (activeNav !== 'mail' || !selectedEmail) { e.preventDefault(); return; }
      }

      // ── Find: Ctrl+F ──
      if (mod && e.key === 'f' && !e.shiftKey) { e.preventDefault(); prevSearchFocus = { focusedPane, calInnerPane, selectedEmailId, selectedContactId, activeNav }; titleBarRef?.focusSearch(); return; }

      // ── Print: Ctrl+P ──
      if (mod && e.key === 'p') { e.preventDefault(); handlePrint(); return; }

      // ── Ctrl+F1/F2/F3: Switch view ──
      if (mod && e.key === 'F1') { e.preventDefault(); activeNav = 'mail'; return; }
      if (mod && e.key === 'F2') { e.preventDefault(); activeNav = 'calendar'; return; }
      if (mod && e.key === 'F3') { e.preventDefault(); activeNav = 'contacts'; return; }

      // ── Alt+1..N: Switch account ──
      if (e.altKey && !mod && /^[1-9]$/.test(e.key)) {
        const idx = parseInt(e.key) - 1;
        if (idx < accounts.length) { e.preventDefault(); handleSelectAccount(accounts[idx].id); }
        return;
      }

      // ── Undo: Ctrl+Z (not in text fields — let native undo work there) ──
      if (mod && e.key === 'z' && !e.shiftKey && !isEditing) { e.preventDefault(); performUndo(); return; }

      // ── Ctrl+T: Go to today (calendar) ──
      if (mod && e.key === 't' && activeNav === 'calendar') { e.preventDefault(); calendarViewRef?.goToToday(); return; }

      // ── Ctrl+Shift+N: New contact list (contacts view only) ──
      if (mod && e.shiftKey && (e.key === 'N' || e.key === 'n') && !isEditing && activeNav === 'contacts') {
        e.preventDefault();
        newContactListTrigger++;
        return;
      }

      // ── Ctrl+N: New item based on active view ──
      if (mod && !e.shiftKey && e.key === 'n' && !isEditing) {
        e.preventDefault();
        if (activeNav === 'mail') handleCompose('new');
        else if (activeNav === 'calendar') newEventTrigger++;
        else if (activeNav === 'contacts') newContactTrigger++;
        return;
      }

      // ── Contacts detail pane: trap Tab, allow arrow nav regardless of focus target ──
      if (contactsFocusedPane === 'detail' && activeNav === 'contacts') {
        if (e.key === 'Tab') {
          // Let ContactsView's handleDetailKeydown manage Tab trapping
          return;
        }
        if (e.key === 'ArrowLeft') {
          e.preventDefault();
          e.stopPropagation();
          contactsViewRef?.navigateArrow(e.key);
          return;
        }
      }

      // ── Contacts shortcuts ──
      if (activeNav === 'contacts' && !isEditing) {
        if (e.shiftKey && e.key === 'E') { e.preventDefault(); editContactTrigger++; return; }
        if (e.key === 'Delete' || (mod && e.key === 'd')) { e.preventDefault(); deleteContactTrigger++; return; }
        if (mod && e.key === 'e') { e.preventDefault(); contactsViewRef?.emailSelected(); return; }
        if (mod && e.key === 'm') { e.preventDefault(); contactsViewRef?.meetSelected(); return; }
        if (mod && e.key === 'c') { e.preventDefault(); contactsViewRef?.callSelected(); return; }
        if (e.altKey && e.key === 'f') { e.preventDefault(); contactsViewRef?.toggleSelectedFavorite(); return; }
        if (e.altKey && e.key === 'm') { e.preventDefault(); contactsViewRef?.toggleShowMuted(); return; }
      }

      // ── Calendar search shortcuts ──
      if (activeNav === 'calendar' && !isEditing && searchQuery.trim()) {
        if (mod && e.key === 'e') { e.preventDefault(); calendarViewRef?.editSearchSelectedEvent(); return; }
        if (e.key === 'Delete' || (mod && e.key === 'd')) { e.preventDefault(); calendarViewRef?.deleteSearchSelectedEvent(); return; }
      }

      // ── Shift+J: join online meeting ──
      if (activeNav === 'calendar' && !isEditing && e.shiftKey && e.key === 'J') {
        e.preventDefault(); calendarViewRef?.joinMeeting(); return;
      }

      // ── Tab key: cycle inbox tabs when in messages pane (not during compose) ──
      if (e.key === 'Tab' && !e.shiftKey && !isEditing && activeNav === 'mail' && focusedPane === 'messages' && !mod && !composeMode) {
        e.preventDefault();
        messageListRef?.cycleInboxTab();
        return;
      }

      // ── Tab key: block in calendar search, switch view mode otherwise ──
      if (e.key === 'Tab' && !isEditing && activeNav === 'calendar' && !mod && !document.querySelector('.ev-modal') && searchQuery.trim()) {
        e.preventDefault();
        return;
      }
      if (e.key === 'Tab' && !isEditing && activeNav === 'calendar' && !mod && !document.querySelector('.ev-modal') && !searchQuery.trim()) {
        e.preventDefault();
        const modes: CalendarViewMode[] = ['day', 'week', 'month'];
        const idx = modes.indexOf(calendarViewMode);
        calendarViewMode = modes[e.shiftKey ? (idx - 1 + 3) % 3 : (idx + 1) % 3];
        return;
      }

      // ── Alt+H: toggle all headers in reading pane ──
      if (e.altKey && e.key === 'h' && activeNav === 'mail') { e.preventDefault(); showAllHeaders = !showAllHeaders; return; }

      // ── Alt+S: open Settings ──
      if (e.altKey && e.key === 's') { e.preventDefault(); showSettings = true; return; }

      // ── Ctrl+F5: sync (same as CommandBar Sync button) ──
      if (e.ctrlKey && e.key === 'F5') {
        e.preventDefault();
        if (activeNav === 'calendar') handleSyncCalendars();
        else if (activeNav === 'contacts') handleSyncContacts();
        else handleSyncCurrentAccount();
        return;
      }

      // ── Alt+F: toggle FolderPane visibility ──
      if (e.altKey && e.key === 'f' && activeNav === 'mail' && !isEditing) {
        e.preventDefault();
        folderPaneVisible = !folderPaneVisible;
        if (!folderPaneVisible && focusedPane === 'folders') focusedPane = 'messages';
        return;
      }

      // ── Calendar: Enter/Space/Esc ──
      if (activeNav === 'calendar' && !isEditing && ['Enter', ' ', 'Escape'].includes(e.key)) {
        // Enter/Space in cal-list: toggle the focused calendar item
        if (['Enter', ' '].includes(e.key) && focusedPane === 'cal-sidebar' && calInnerPane === 'cal-list-inner') {
          e.preventDefault(); e.stopPropagation();
          calendarViewRef?.toggleFocusedCalListItem();
          return;
        }

        if (e.key === 'Enter' && focusedPane === 'cal-sidebar' && calInnerPane === 'none') {
          e.preventDefault(); e.stopPropagation();
          calInnerPane = 'cal-mini-inner';
          return;
        }

        if (e.key === 'Enter' && focusedPane === 'cal-main' && calInnerPane === 'cal-main-inner' && !calendarViewRef?.hasDetailModal()) {
          e.preventDefault(); e.stopPropagation();
          calendarViewRef?.navigateMainCal('Enter');
          return;
        }

        // ── Escape: exit calendar inner pane ──
        if (e.key === 'Escape' && !calendarViewRef?.hasDetailModal()) {
            e.preventDefault();
            focusedPane = 'cal-sidebar';
            calInnerPane = 'none';
            return;
        }
      }

      // ── Arrow key navigation (not editing) ──
      if (!isEditing && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) {
        // Rail navigation (shared across mail & contacts)
        if (focusedPane === 'rail') {
          e.preventDefault();
          e.stopPropagation();
          const navItems: NavItem[] = ['mail', 'calendar', 'contacts'];
          if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            const down = e.key === 'ArrowDown';
            const curIdx = navItems.indexOf(activeNav);
            const nextIdx = down ? Math.min(curIdx + 1, navItems.length - 1) : Math.max(curIdx - 1, 0);
            if (curIdx !== nextIdx) {
              activeNav = navItems[nextIdx];
              newEventTrigger = 0; newContactTrigger = 0; newContactListTrigger = 0; editContactTrigger = 0; deleteContactTrigger = 0; contactsInListMode = false;
            }
          } else if (e.key === 'ArrowRight') {
            if (activeNav === 'mail') {
              focusedPane = folderPaneVisible ? 'folders' : 'messages';
              if (!folderPaneVisible && !selectedEmailId && messageVisibleList.length > 0) {
                selectedEmailId = messageVisibleList[0].id;
              }
            }
            else if (activeNav === 'contacts') { focusedPane = 'folders'; contactsRequestFocusPane = 'nav'; }
            else if (activeNav === 'calendar') { focusedPane = 'cal-sidebar'; calInnerPane = 'none'; }
          }
          return;
        }

        // Mail arrow navigation
        if (activeNav === 'mail') {
          e.preventDefault();

          if (e.key === 'ArrowRight' && focusedPane === 'folders') {
            if (messageVisibleList.length === 0) return;
            focusedPane = 'messages';
            if (!selectedEmailId) {
              selectedEmailId = messageVisibleList[0].id;
            }
            return;
          }
          if (e.key === 'ArrowLeft' && focusedPane === 'messages') {
            focusedPane = folderPaneVisible ? 'folders' : 'rail';
            return;
          }
          if (e.key === 'ArrowRight' && focusedPane === 'messages') {
            if (composeMode) {
              focusedPane = 'compose';
              requestAnimationFrame(() => composePaneRef?.focus());
            } else if (selectedEmail) {
              focusedPane = 'reading';
              readingPaneRef?.focus();
            }
            return;
          }
          if (e.key === 'ArrowLeft' && (focusedPane === 'reading' || focusedPane === 'compose')) {
            focusedPane = 'messages';
            return;
          }
          if (e.key === 'ArrowLeft' && focusedPane === 'folders') {
            focusedPane = 'rail';
            return;
          }

          if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            const down = e.key === 'ArrowDown';
            if (focusedPane === 'reading') {
              readingPaneRef?.scroll(down ? 100 : -100);
              return;
            }
            if (focusedPane === 'folders' && folderPaneVisible) {
              const folders = activeAccount?.folders ?? [];
              if (folders.length === 0) return;
              const curIdx = folders.findIndex(f => f.id === activeFolder);
              const nextIdx = down ? Math.min(curIdx + 1, folders.length - 1) : Math.max(curIdx - 1, 0);
              if (curIdx !== nextIdx) handleSelectFolder(folders[nextIdx].id);
            } else if (focusedPane === 'messages') {
              const list = messageVisibleList;
              if (list.length === 0) return;
              const curIdx = list.findIndex(em => em.id === selectedEmailId);
              const nextIdx = down ? Math.min(curIdx + 1, list.length - 1) : Math.max(curIdx - 1, 0);
              if (curIdx !== nextIdx || curIdx === -1) {
                selectedEmailId = list[nextIdx === -1 ? 0 : nextIdx].id;
              }
            }
          }
          return;
        }

        // Calendar arrow navigation
        if (activeNav === 'calendar') {
          // Let the event detail modal handle keys when it's open
          if (calendarViewRef?.hasDetailModal()) return;
          e.preventDefault();
          e.stopPropagation();
          // Inner pane navigation - forward to CalendarView
          if (calInnerPane === 'cal-mini-inner') { calendarViewRef?.navigateMiniCal(e.key); return; }
          if (calInnerPane === 'cal-main-inner' && !searchQuery.trim()) { calendarViewRef?.navigateMainCal(e.key); return; }
          if (focusedPane === 'cal-sidebar') {
            if (e.key === 'ArrowLeft') {
                calInnerPane = 'none';
                focusedPane = 'rail';
            }
            else if (e.key === 'ArrowRight') {
                focusedPane = 'cal-main';
                if (searchQuery.trim()) {
                  calendarViewRef?.focusSearchResults();
                } else {
                  calInnerPane = 'cal-main-inner';
                }
            }
            else if (e.key === 'ArrowDown' && calInnerPane === 'none') {
                calInnerPane = 'cal-list-inner';
                calendarViewRef?.navigateCalList(e.key);
            } 
            else if (['ArrowUp', 'ArrowDown'].includes(e.key) && calInnerPane === 'cal-list-inner') {
                const result = calendarViewRef?.navigateCalList(e.key);
                if (result === 'at-top') calInnerPane = 'none';
            }
          } else if (focusedPane === 'cal-main') {
            if (searchQuery.trim()) {
              const handled = calendarViewRef?.navigateSearchResults(e.key) ?? false;
              if (!handled) {
                focusedPane = 'cal-sidebar';
                calInnerPane = 'cal-list-inner';
                calendarViewRef?.navigateCalList('ArrowDown');
              }
            } else if (e.key === 'ArrowLeft') {
              focusedPane = 'cal-sidebar';
              calInnerPane = 'none';
            } else {
              calInnerPane = 'cal-main-inner';
            }
          }
          return;
        }

        // Contacts arrow navigation
        if (activeNav === 'contacts') {
          e.preventDefault();
          if (e.key === 'ArrowLeft' && contactsFocusedPane === 'nav') {
            focusedPane = 'rail';
            contactsRequestFocusPane = 'none';
            return;
          }
          contactsViewRef?.navigateArrow(e.key);
          return;
        }
      }

      // ── Mail shortcuts (only when in mail view with a selected email) ──
      if (activeNav !== 'mail' || !selectedEmail) return;

      // Enter on a selected draft opens it in compose
      if (e.key === 'Enter' && !mod && selectedEmail.folder === 'drafts' && !isEditing) {
        e.preventDefault(); openDraftInCompose(selectedEmail); return;
      }

      // Skip non-modifier keys when user is typing in an input
      if (isEditing && !mod) return;

      // Delete: Delete or Ctrl+D
      if (e.key === 'Delete' || (mod && e.key === 'd')) { e.preventDefault(); handleDeleteSelectedEmail(); return; }
      // Archive: Backspace
      if (e.key === 'Backspace' && !mod && !e.shiftKey) { e.preventDefault(); handleArchiveSelectedEmail(); return; }
      // Move: Ctrl+Shift+V — toggle move menu in CommandBar
      if (mod && e.shiftKey && e.key === 'V') { e.preventDefault(); showMoveMenu = !showMoveMenu; return; }
      // Reply: Ctrl+R / Reply All: Ctrl+Shift+R
      if (mod && e.key === 'r') { e.preventDefault(); handleCompose(e.shiftKey ? 'replyAll' : 'reply'); return; }
      // Forward: Ctrl+Shift+F
      if (mod && e.shiftKey && e.key === 'F') { e.preventDefault(); handleCompose('forward'); return; }
      // Junk: Ctrl+J
      if (mod && e.key === 'j') { e.preventDefault(); handleJunkSelectedEmail(); return; }
      // Mark as read: Ctrl+Q
      if (mod && e.key === 'q') { e.preventDefault(); handleMarkSelectedRead(); return; }
      // Mark as unread: Ctrl+U
      if (mod && e.key === 'u') { e.preventDefault(); handleMarkSelectedUnread(); return; }
    };
    document.addEventListener('keydown', globalKeyHandler, true);

    // Suppress the default WebView2 context menu (which contains a "Print"
    // entry whose overlay sticks to the top edge of the webview).
    document.addEventListener('contextmenu', (e) => e.preventDefault());

    try {
      const loadedAccounts = await loadAccounts();
      const loadedTheme = await loadTheme();
      const loadedAccent = await loadAccentColor();
      const loadedLocale = await loadLocale();
      setLocale(loadedLocale ?? detectLocale());
      accounts = loadedAccounts;
      theme = loadedTheme;
      accentColor = loadedAccent;
      if (accounts.length > 0) {
        applyAccountState(accounts[0].id, accounts);
        contactLists = await getContactLists(accounts[0].id);
        startupStatus = 'ready';
        // Trigger initial sync for configured accounts (non-blocking)
        handleSyncAll();
        // Set up periodic per-account sync check (every 60s)
        syncTimerId = setInterval(() => handlePerAccountSync(), 60_000);
        // Listen for backend push after a message is sent (sync Sent folder + \Answered flag)
        listen<string>('mail:sent', (event) => {
          handleSyncAccount(event.payload);
        }).then((unlisten) => { unlistenMailSent = unlisten; });
      } else {
        requireAccountSetup = true;
        startupStatus = 'needs-account';
      }
    } catch (error) {
      console.error('Failed to initialize app state', error);
      startupError = error instanceof Error ? error.message : 'Failed to initialize app state';
      accounts = [];
      requireAccountSetup = true;
      startupStatus = 'needs-account';
    } finally {
      if (accounts.length === 0) {
        showSettings = true;
      }
      dataReady = true;
    }
  });

  onDestroy(() => {
    if (syncTimerId) clearInterval(syncTimerId);
    if (unlistenMailSent) unlistenMailSent();
    if (unlistenOffline) unlistenOffline();
    if (globalKeyHandler) document.removeEventListener('keydown', globalKeyHandler, true);
    clearAlertQueue();
  });

  // Rebuild event alert queue whenever calendar events change across any account
  $effect(() => {
    const allEvents = accounts.flatMap((a) => a.calendarEvents);
    rebuildAlertQueue(allEvents);
  });

  let activeAccount = $derived(accounts.find((a) => a.id === activeAccountId) ?? null);
  let emails = $state<Email[]>([]);
  let calendarCategories = $state<any[]>([]);
  let contactLists = $state<ContactList[]>([]);

  // Keep accounts[activeAccountId].emails in sync with local mutations (for unread badge etc.)
  $effect(() => {
    const id = activeAccountId;
    const snapshot = emails;
    untrack(() => {
      accounts = accounts.map((a) => (a.id === id ? { ...a, emails: snapshot } : a));
    });
  });
  let selectedEmailId = $state<string>('');
  let checkedEmailIds = $state<Set<string>>(new Set());
  let multiSelectCount = $derived(checkedEmailIds.size);
  // True if at least one checked email is unread. Drives CommandBar's
  // Read/Unread toggle: any unread → show "Mark as read"; all read → show "Mark as unread".
  let multiSelectHasUnread = $derived(
    emails.some((e) => checkedEmailIds.has(e.id) && !e.isRead)
  );

  // Checked drives active:
  //   exactly 1 checked → that mail is active (shown in ReadingPane)
  //   more than 1 checked → no active mail
  //   0 checked → leave active as-is (user can click a row to activate)
  $effect(() => {
    const size = checkedEmailIds.size;
    if (size > 1) {
      if (selectedEmailId !== '') selectedEmailId = '';
    } else if (size === 1) {
      const onlyId = checkedEmailIds.values().next().value!;
      if (selectedEmailId !== onlyId) selectedEmailId = onlyId;
    }
  });
  let selectedContactId = $state<string>('');
  let selectedListId = $state<string>('');
  let showMoveMenu = $state(false);
  let showAllHeaders = $state(false);
  let focusedPane = $state<'rail' | 'folders' | 'messages' | 'reading' | 'compose' | 'cal-sidebar' | 'cal-main'>('rail');
  let calInnerPane = $state<'none'| 'cal-list-inner' | 'cal-mini-inner' | 'cal-main-inner'>('none');
  let messageVisibleList = $state<Email[]>([]);
  let contactVisibleList = $state<FullContact[]>([]);

  function isAccountFullyConfigured(account: Account) {
    const settings = account.serverSettings;
    return Boolean(
      account.name.trim() &&
      account.email.trim() &&
      account.initials.trim() &&
      settings?.incomingServer?.trim() &&
      settings?.incomingUsername?.trim() &&
      settings?.incomingPassword?.trim() &&
      settings?.smtpServer?.trim() &&
      settings?.smtpUsername?.trim() &&
      settings?.smtpPassword?.trim() &&
      Number.isFinite(settings?.incomingPort) &&
      Number(settings?.incomingPort) > 0 &&
      Number.isFinite(settings?.smtpPort) &&
      Number(settings?.smtpPort) > 0
    );
  }

  function applyAccountState(accountId: string, nextAccounts = accounts) {
    const acct = nextAccounts.find((a) => a.id === accountId);
    if (!acct) {
      activeAccountId = '';
      emails = [];
      calendarCategories = [];
      selectedEmailId = '';
      return;
    }

    activeAccountId = acct.id;
    emails = [...acct.emails];
    calendarCategories = [...acct.calendarCategories];
    activeFolder = 'inbox';
    const inboxEmails = acct.emails.filter((e) => e.folder === 'inbox');
    selectedEmailId = inboxEmails[0]?.id ?? '';
  }

  async function closeAppWindow() {
    try {
      await getCurrentWindow().close();
    } catch {
      if (typeof window !== 'undefined') {
        window.close();
      }
    }
  }

  async function handleSelectAccount(accountId: string) {
    if (accountId === activeAccountId) return;
    applyAccountState(accountId);
    searchQuery = '';
    composeMode = null;
    composeReplyTo = null;
    activeDraftId = null;
    composeDraft = null;
    focusedPane = 'rail';
    contactLists = await getContactLists(accountId);
  }

  // ── Sync ──────────────────────────────────────────────

  async function handleSyncAll() {
    if (syncing) return;
    syncing = true;
    syncErrors = [];
    const errors: string[] = [];

    for (const acct of accounts) {
      if (!isAccountFullyConfigured(acct)) continue;
      try {
        const result = await syncMail(acct.id);
        if (result?.errors?.length) {
          errors.push(...result.errors.map((e) => `${acct.email}: ${e}`));
        }
      } catch (e) {
        errors.push(`${acct.email}: ${e instanceof Error ? e.message : String(e)}`);
      }
      // Sync calendars if CalDAV configured
      if (acct.calDavSettings?.url) {
        try { await syncCalendars(acct.id); } catch { /* best-effort */ }
      }
      // Sync contacts if CardDAV configured
      if (acct.cardDavSettings?.url) {
        try { await syncContacts(acct.id); } catch { /* best-effort */ }
      }
      // Also flush any pending outbox items
      try {
        await flushOutbox(acct.id);
      } catch { /* best-effort */ }
      lastSyncTime.set(acct.id, Date.now());
    }

    // Reload all accounts to pick up new/deleted/updated emails
    try {
      const refreshed = await loadAccounts();
      const prevActiveId = activeAccountId;
      const prevFolder = activeFolder;
      const prevSelectedId = selectedEmailId;
      accounts = refreshed;
      // Restore view state
      const acct = refreshed.find((a) => a.id === prevActiveId);
      if (acct) {
        activeAccountId = acct.id;
        emails = [...acct.emails];
        calendarCategories = [...acct.calendarCategories];
        activeFolder = prevFolder;
        // Keep selected email if it still exists, else pick first in folder
        const folderMails = acct.emails.filter((e) => e.folder === prevFolder);
        if (folderMails.some((e) => e.id === prevSelectedId)) {
          selectedEmailId = prevSelectedId;
        } else {
          selectedEmailId = folderMails[0]?.id ?? '';
        }
      } else if (refreshed.length > 0) {
        applyAccountState(refreshed[0].id, refreshed);
      }
    } catch (e) {
      errors.push(`Reload failed: ${e instanceof Error ? e.message : String(e)}`);
    }

    syncErrors = errors;
    syncing = false;
  }

  async function handleSyncAccount(accountId: string) {
    if (syncing) return;
    syncing = true;
    try {
      await syncMail(accountId);
      lastSyncTime.set(accountId, Date.now());
      const refreshed = await loadAccounts();
      const prevActiveId = activeAccountId;
      const prevFolder = activeFolder;
      const prevSelectedId = selectedEmailId;
      accounts = refreshed;
      const acct = refreshed.find((a) => a.id === prevActiveId);
      if (acct) {
        activeAccountId = acct.id;
        emails = [...acct.emails];
        calendarCategories = [...acct.calendarCategories];
        activeFolder = prevFolder;
        const folderMails = acct.emails.filter((e) => e.folder === prevFolder);
        selectedEmailId = folderMails.some((e) => e.id === prevSelectedId)
          ? prevSelectedId
          : folderMails[0]?.id ?? '';
      }
    } catch (e) {
      console.error('Post-send sync failed:', e);
    } finally {
      syncing = false;
    }
  }

  async function handlePerAccountSync() {
    if (syncing) return;
    const now = Date.now();
    const due = accounts.filter(acct => {
      if (!isAccountFullyConfigured(acct)) return false;
      const intervalMs = (acct.serverSettings?.syncIntervalMinutes ?? 5) * 60_000;
      const last = lastSyncTime.get(acct.id) ?? 0;
      return now - last >= intervalMs;
    });
    if (due.length === 0) return;

    syncing = true;
    syncErrors = [];
    const errors: string[] = [];

    for (const acct of due) {
      try {
        const result = await syncMail(acct.id);
        if (result?.errors?.length) {
          errors.push(...result.errors.map((e: string) => `${acct.email}: ${e}`));
        }
      } catch (e) {
        errors.push(`${acct.email}: ${e instanceof Error ? e.message : String(e)}`);
      }
      if (acct.calDavSettings?.url) {
        try { await syncCalendars(acct.id); } catch { /* best-effort */ }
      }
      if (acct.cardDavSettings?.url) {
        try { await syncContacts(acct.id); } catch { /* best-effort */ }
      }
      try { await flushOutbox(acct.id); } catch { /* best-effort */ }
      lastSyncTime.set(acct.id, Date.now());
    }

    // Reload all accounts to pick up changes
    try {
      const refreshed = await loadAccounts();
      const prevActiveId = activeAccountId;
      const prevFolder = activeFolder;
      const prevSelectedId = selectedEmailId;
      accounts = refreshed;
      const acct = refreshed.find((a) => a.id === prevActiveId);
      if (acct) {
        activeAccountId = acct.id;
        emails = [...acct.emails];
        calendarCategories = [...acct.calendarCategories];
        activeFolder = prevFolder;
        const folderMails = acct.emails.filter((e) => e.folder === prevFolder);
        if (folderMails.some((e) => e.id === prevSelectedId)) {
          selectedEmailId = prevSelectedId;
        } else {
          selectedEmailId = folderMails[0]?.id ?? '';
        }
      } else if (refreshed.length > 0) {
        applyAccountState(refreshed[0].id, refreshed);
      }
    } catch (e) {
      errors.push(`Reload failed: ${e instanceof Error ? e.message : String(e)}`);
    }

    syncErrors = errors;
    syncing = false;
  }

  async function handleSyncCurrentAccount() {
    if (syncing || !activeAccount) return;
    if (!isAccountFullyConfigured(activeAccount)) return;
    syncing = true;
    syncErrors = [];
    const errors: string[] = [];

    const mailPromise = (async () => {
      try {
        const result = await syncMail(activeAccount.id);
        if (result?.errors?.length) errors.push(...result.errors);
      } catch (e) {
        errors.push(e instanceof Error ? e.message : String(e));
      }
    })();
    const calPromise = activeAccount.calDavSettings?.url
      ? syncCalendars(activeAccount.id).catch(() => { /* best-effort */ })
      : Promise.resolve();
    const contactsPromise = activeAccount.cardDavSettings?.url
      ? syncContacts(activeAccount.id).catch(() => { /* best-effort */ })
      : Promise.resolve();
    const outboxPromise = flushOutbox(activeAccount.id).catch(() => { /* best-effort */ });
    await Promise.all([mailPromise, calPromise, contactsPromise, outboxPromise]);

    // Reload
    try {
      const refreshed = await loadAccounts();
      const prevFolder = activeFolder;
      const prevSelectedId = selectedEmailId;
      accounts = refreshed;
      const acct = refreshed.find((a) => a.id === activeAccountId);
      if (acct) {
        emails = [...acct.emails];
        calendarCategories = [...acct.calendarCategories];
        activeFolder = prevFolder;
        const folderMails = acct.emails.filter((e) => e.folder === prevFolder);
        selectedEmailId = folderMails.some((e) => e.id === prevSelectedId)
          ? prevSelectedId
          : folderMails[0]?.id ?? '';
      }
    } catch (e) {
      errors.push(`Reload failed: ${e instanceof Error ? e.message : String(e)}`);
    }
    syncErrors = errors;
    syncing = false;
  }

  async function handleSyncCalendars() {
    if (syncing || !activeAccount?.calDavSettings?.url) return;
    syncing = true;
    try {
      await syncCalendars(activeAccount.id);
      const refreshed = await loadAccounts();
      accounts = refreshed;
      const acct = refreshed.find((a) => a.id === activeAccountId);
      if (acct) calendarCategories = [...acct.calendarCategories];
    } catch (e) {
      syncErrors = [e instanceof Error ? e.message : String(e)];
    }
    syncing = false;
  }

  async function handleSyncContacts() {
    if (syncing || !activeAccount?.cardDavSettings?.url) return;
    syncing = true;
    try {
      await syncContacts(activeAccount.id);
      const refreshed = await loadAccounts();
      accounts = refreshed;
    } catch (e) {
      syncErrors = [e instanceof Error ? e.message : String(e)];
    }
    syncing = false;
  }

  // Ignored senders are hidden from the message list (drafts/sent stay
  // visible even if you've muted yourself — the filter only applies to the
  // sender on received mail).
  let folderEmails = $derived(emails.filter((e) =>
    e.folder === activeFolder &&
    !e.isSending &&
    !(e.folder !== 'sent' && e.folder !== 'drafts' && mutedAddresses.has(e.from.email.toLowerCase()))
  ));

  let parsedSearchClauses = $derived(parseSearchQuery(searchQuery));
  let filteredEmails = $derived(
    parsedSearchClauses.length === 0
      ? folderEmails
      : folderEmails.filter((e) => emailMatchesQuery(e, parsedSearchClauses))
  );

  let selectedEmail = $derived(emails.find((e) => e.id === selectedEmailId) ?? null);

  let unreadCounts = $derived(
    emails.reduce(
      (acc, e) => {
        if (e.folder === 'drafts' && !e.isSending) {
          acc[e.folder] = (acc[e.folder] || 0) + 1;
        } else if (!e.isRead) {
          acc[e.folder] = (acc[e.folder] || 0) + 1;
        }
        return acc;
      },
      {} as Record<string, number>
    )
  );

  let folderEmailCounts = $derived(
    emails.reduce(
      (acc, e) => { acc[e.folder] = (acc[e.folder] || 0) + 1; return acc; },
      {} as Record<string, number>
    )
  );

  // Track body-fetch in-flight to avoid duplicates
  let fetchingBodyId = $state<string | null>(null);

  function handleSelectEmail(email: Email) {
    // Clicking a row activates only that mail — clear any checked set so the
    // checked/active invariant (0 checked ⇒ 0–1 active, N checked ⇒ N active) holds.
    if (checkedEmailIds.size > 0) checkedEmailIds = new Set();
    selectedEmailId = email.id;

    // Close compose when selecting a different email (unless it's the active draft)
    if (composeMode && activeDraftId && activeDraftId !== email.id) {
      composeMode = null;
      composeReplyTo = null;
      composeDraft = null;
      activeDraftId = null;
    }

    if (!email.isRead) {
      emails = emails.map((e) => (e.id === email.id ? { ...e, isRead: true } : e));
      markEmailRead(activeAccountId, email.id, true);
    }

    // Lazy-load full body if not yet fetched
    if (!email.body && activeAccountId && email.id !== fetchingBodyId) {
      fetchingBodyId = email.id;
      fetchEmailBody(activeAccountId, email.id)
        .then((result) => {
          if (result) {
            emails = emails.map((e) =>
              e.id === email.id
                ? {
                    ...e,
                    body: result.body,
                    searchText: buildSearchText(result.body),
                    preview: e.preview || result.preview,
                    authResults: result.authResults || e.authResults,
                    hasAttachment: result.hasAttachment ?? e.hasAttachment,
                    attachments: result.attachments ?? e.attachments,
                  }
                : e
            );
          }
        })
        .catch((err) => console.error('Failed to fetch email body:', err))
        .finally(() => {
          if (fetchingBodyId === email.id) fetchingBodyId = null;
        });
    }
  }

  // Track in-flight preview-window fetches to avoid hammering IMAP while the
  // user scrolls fast. Keyed by anchor email id.
  const previewFetchesInFlight = new Set<string>();

  async function handlePreviewMissing(email: Email) {
    if (!activeAccountId || previewFetchesInFlight.has(email.id)) return;
    previewFetchesInFlight.add(email.id);
    try {
      const updates = await fetchPreviewsAround(activeAccountId, email.id, 20, 10);
      if (!updates.length) return;
      const byId = new Map(updates.map(u => [u.id, u]));
      emails = emails.map((e) => {
        const u = byId.get(e.id);
        if (!u) return e;
        const nextBody = u.body || e.body;
        return {
          ...e,
          body: nextBody,
          searchText: u.body ? buildSearchText(u.body) : e.searchText,
          preview: u.preview || e.preview,
          authResults: u.authResults ?? e.authResults,
          hasAttachment: u.hasAttachment ?? e.hasAttachment,
          attachments: u.attachments ?? e.attachments,
        };
      });
    } catch (err) {
      console.error('Failed to prefetch previews:', err);
    } finally {
      previewFetchesInFlight.delete(email.id);
    }
  }

  function handleLabelClick(label: string) {
    // Route to operator-aware search. Quote labels that contain whitespace.
    const value = /\s/.test(label) ? `"${label}"` : label;
    searchQuery = `label:${value}`;
  }

  async function handleOpenAttachment(attachmentIndex: number) {
    if (!selectedEmail || !activeAccountId) return;
    try {
      await openAttachment(activeAccountId, selectedEmail.id, attachmentIndex);
    } catch (err) {
      console.error('Failed to open attachment:', err);
    }
  }

  async function handleSaveAttachment(attachmentIndex: number, filename: string) {
    if (!selectedEmail || !activeAccountId) return;
    try {
      const savePath = await dialogSave({ defaultPath: filename });
      if (!savePath) return; // user cancelled
      await saveAttachment(activeAccountId, selectedEmail.id, attachmentIndex, savePath);
    } catch (err) {
      console.error('Failed to save attachment:', err);
    }
  }

  function handleSelectFolder(folderId: string) {
    activeFolder = folderId;
    const fe = emails.filter((e) => e.folder === folderId);

    // When switching folders, close any open compose (draft stays saved in emails array)
    if (composeMode && activeDraftId) {
      composeMode = null;
      composeReplyTo = null;
      composeDraft = null;
      activeDraftId = null;
    }

    if (fe.length > 0) {
      selectedEmailId = fe[0].id;
    } else {
      selectedEmailId = '';
    }
  }

  /**
   * Return the emails a bulk-capable CommandBar action should target.
   * Any checked mails win over the active mail; when nothing is checked,
   * actions apply to the active mail (shown in ReadingPane).
   */
  function getActionTargets(): Email[] {
    if (checkedEmailIds.size > 0) {
      return emails.filter((e) => checkedEmailIds.has(e.id));
    }
    return selectedEmail ? [selectedEmail] : [];
  }

  function clearBatchSelection() {
    if (checkedEmailIds.size > 0) checkedEmailIds = new Set();
  }

  function handleMarkSelectedRead() {
    if (!activeAccountId) return;
    const targets = getActionTargets().filter((e) => e.folder !== 'drafts' && !e.isRead);
    if (targets.length === 0) return;
    const entries: MailSingleUndo[] = targets.map((e) => ({
      view: 'mail', action: 'markRead', emailId: e.id, prevIsRead: e.isRead,
    }));
    const ids = new Set(targets.map((e) => e.id));
    emails = emails.map((e) => (ids.has(e.id) ? { ...e, isRead: true } : e));
    targets.forEach((e) => markEmailRead(activeAccountId, e.id, true));
    if (entries.length === 1) {
      pushUndo(entries[0], 'Marked as read — Ctrl+Z to undo');
    } else {
      pushUndo({ view: 'mail', action: 'batch', entries }, `Marked ${entries.length} as read — Ctrl+Z to undo`);
    }
    clearBatchSelection();
  }

  function handleMarkSelectedUnread() {
    if (!activeAccountId) return;
    const targets = getActionTargets().filter((e) => e.folder !== 'drafts' && e.isRead);
    if (targets.length === 0) return;
    const entries: MailSingleUndo[] = targets.map((e) => ({
      view: 'mail', action: 'markUnread', emailId: e.id, prevIsRead: e.isRead,
    }));
    const ids = new Set(targets.map((e) => e.id));
    emails = emails.map((e) => (ids.has(e.id) ? { ...e, isRead: false } : e));
    targets.forEach((e) => markEmailRead(activeAccountId, e.id, false));
    if (entries.length === 1) {
      pushUndo(entries[0], 'Marked as unread — Ctrl+Z to undo');
    } else {
      pushUndo({ view: 'mail', action: 'batch', entries }, `Marked ${entries.length} as unread — Ctrl+Z to undo`);
    }
    clearBatchSelection();
  }

  function handleDeleteSelectedEmail() {
    if (!activeAccountId) return;
    const targets = getActionTargets();
    if (targets.length === 0) return;
    if (targets.length === 1) {
      handleDeleteEmail(targets[0]);
      clearBatchSelection();
      return;
    }
    // Permanent-delete items (already in 'deleted' folder) are NOT undoable,
    // so only move-to-trash items go into the batch undo entry.
    const undoable = targets.filter((e) => e.folder !== 'deleted');
    const permanent = targets.filter((e) => e.folder === 'deleted');
    if (permanent.length > 0) {
      undoMail = null; dismissToast();
    }
    const undoableIds = new Set(undoable.map((e) => e.id));
    const permanentIds = new Set(permanent.map((e) => e.id));
    emails = emails
      .filter((e) => !permanentIds.has(e.id))
      .map((e) => (undoableIds.has(e.id) ? { ...e, folder: 'deleted' } : e));
    targets.forEach((e) => deleteEmail(activeAccountId, e.id));
    if (selectedEmailId && (undoableIds.has(selectedEmailId) || permanentIds.has(selectedEmailId))) {
      selectedEmailId = '';
    }
    if (undoable.length > 0) {
      const entries: MailSingleUndo[] = undoable.map((e) => ({
        view: 'mail', action: 'delete', email: { ...e }, prevFolder: e.folder,
      }));
      pushUndo({ view: 'mail', action: 'batch', entries }, `Deleted ${targets.length} — Ctrl+Z to undo`);
    }
    clearBatchSelection();
  }

  function handleDeleteEmail(email: Email) {
    if (!activeAccountId) return;
    if (email.folder === 'deleted') {
      // Permanent delete — NOT undoable; clear undo slot
      undoMail = null; dismissToast();
      emails = emails.filter((e) => e.id !== email.id);
    } else {
      pushUndo({ view: 'mail', action: 'delete', email: { ...email }, prevFolder: email.folder }, 'Deleted — Ctrl+Z to undo');
      emails = emails.map((e) => (e.id === email.id ? { ...e, folder: 'deleted' } : e));
    }
    if (selectedEmailId === email.id) selectedEmailId = '';
    deleteEmail(activeAccountId, email.id);
  }

  function handleMoveEmail(targetFolder: string) {
    if (!activeAccountId) return;
    const targets = getActionTargets().filter((e) => e.folder !== targetFolder);
    if (targets.length === 0) return;
    const entries: MailSingleUndo[] = targets.map((e) => ({
      view: 'mail', action: 'move', emailId: e.id, prevFolder: e.folder,
    }));
    const ids = new Set(targets.map((e) => e.id));
    emails = emails.map((e) => (ids.has(e.id) ? { ...e, folder: targetFolder } : e));
    if (selectedEmailId && ids.has(selectedEmailId)) selectedEmailId = '';
    targets.forEach((e) => moveEmail(activeAccountId, e.id, targetFolder));
    if (entries.length === 1) {
      pushUndo(entries[0], 'Moved — Ctrl+Z to undo');
    } else {
      pushUndo({ view: 'mail', action: 'batch', entries }, `Moved ${entries.length} — Ctrl+Z to undo`);
    }
    clearBatchSelection();
  }

  function handleArchiveSelectedEmail() {
    if (!activeAccountId) return;
    const targets = getActionTargets().filter((e) => e.folder !== 'archive');
    if (targets.length === 0) return;
    const entries: MailSingleUndo[] = targets.map((e) => ({
      view: 'mail', action: 'archive', emailId: e.id, prevFolder: e.folder,
    }));
    const ids = new Set(targets.map((e) => e.id));
    emails = emails.map((e) => (ids.has(e.id) ? { ...e, folder: 'archive' } : e));
    if (selectedEmailId && ids.has(selectedEmailId)) selectedEmailId = '';
    targets.forEach((e) => moveEmail(activeAccountId, e.id, 'archive'));
    if (entries.length === 1) {
      pushUndo(entries[0], 'Archived — Ctrl+Z to undo');
    } else {
      pushUndo({ view: 'mail', action: 'batch', entries }, `Archived ${entries.length} — Ctrl+Z to undo`);
    }
    clearBatchSelection();
  }

  function handleJunkSelectedEmail() {
    if (!activeAccountId) return;
    const targets = getActionTargets();
    if (targets.length === 0) return;
    // Batch rule: if *any* target is not in junk, move everything to junk;
    // otherwise (all already in junk) move everything back to inbox. This
    // matches the single-item toggle behavior in the predominant direction.
    const anyNonJunk = targets.some((e) => e.folder !== 'junk');
    const target: string = anyNonJunk ? 'junk' : 'inbox';
    const toMove = targets.filter((e) => e.folder !== target);
    if (toMove.length === 0) return;
    const entries: MailSingleUndo[] = toMove.map((e) => ({
      view: 'mail', action: 'junk', emailId: e.id, prevFolder: e.folder,
    }));
    const ids = new Set(toMove.map((e) => e.id));
    emails = emails.map((e) => (ids.has(e.id) ? { ...e, folder: target } : e));
    if (selectedEmailId && ids.has(selectedEmailId)) selectedEmailId = '';
    toMove.forEach((e) => moveEmail(activeAccountId, e.id, target));
    // Update sender blocklist per-email
    toMove.forEach((e) => {
      if (target === 'junk') {
        addToSenderBlocklist(activeAccountId, e.from.email, 'block');
      } else {
        removeFromSenderBlocklist(activeAccountId, e.from.email);
      }
    });
    const label = target === 'junk' ? 'Moved to Junk' : 'Moved to Inbox';
    if (entries.length === 1) {
      pushUndo(entries[0], `${label} — Ctrl+Z to undo`);
    } else {
      pushUndo({ view: 'mail', action: 'batch', entries }, `${label} ${entries.length} — Ctrl+Z to undo`);
    }
    clearBatchSelection();
  }

  function handleToggleStar(email: Email) {
    pushUndo({ view: 'mail', action: 'toggleStar', emailId: email.id, prevIsStarred: email.isStarred }, `${email.isStarred ? 'Unstarred' : 'Starred'} — Ctrl+Z to undo`);
    const newVal = !email.isStarred;
    emails = emails.map((e) => (e.id === email.id ? { ...e, isStarred: newVal } : e));
    updateEmailStarred(activeAccountId, email.id, newVal);
  }

  function handleTogglePin(email: Email) {
    pushUndo({ view: 'mail', action: 'togglePin', emailId: email.id, prevIsPinned: email.isPinned }, `${email.isPinned ? 'Unpinned' : 'Pinned'} — Ctrl+Z to undo`);
    const newVal = !email.isPinned;
    emails = emails.map((e) => (e.id === email.id ? { ...e, isPinned: newVal } : e));
    updateEmailPinned(email.id, newVal);
  }

  function handleToggleFocused(email: Email) {
    pushUndo({ view: 'mail', action: 'toggleFocused', emailId: email.id, prevIsFocused: email.isFocused ?? true }, 'Focus toggled — Ctrl+Z to undo');
    const newVal = !(email.isFocused ?? true);
    emails = emails.map((e) => (e.id === email.id ? { ...e, isFocused: newVal } : e));
    updateEmailFocused(email.id, newVal);
  }

  /** Strip HTML tags to produce a plain-text preview */
  function stripHtmlPreview(html: string): string {
    const tmp = document.createElement('div');
    tmp.innerHTML = html;
    return (tmp.textContent || tmp.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 150);
  }

  /** Build a ComposeDraft from an Email (for drafts without draftData) */
  function buildDraftFromEmail(email: Email): ComposeDraft {
    return {
      to: email.to.map((c) => `${c.name} <${c.email}>`).join('; '),
      cc: '',
      bcc: '',
      subject: email.subject,
      body: email.body,
      showCc: false,
      showBcc: false,
      attachments: [],
    };
  }

  /** Create a new draft Email and add it to the emails array */
  function createDraftEmail(): string {
    const acct = activeAccount;
    if (!acct) {
      throw new Error('No active account selected');
    }
    const draftId = crypto.randomUUID();
    const draftEmail: Email = {
      id: draftId,
      from: { name: acct.alias || acct.name, email: acct.email, initials: acct.initials, color: acct.color },
      to: [],
      subject: '',
      preview: '',
      body: '',
      date: new Date(),
      isRead: true,
      isStarred: false,
      isPinned: false,
      hasAttachment: false,
      folder: 'drafts',
      isFocused: true,
    };
    emails = [...emails, draftEmail];
    return draftId;
  }

  /** Open a draft email in the ComposePane */
  function openDraftInCompose(email: Email) {
    activeDraftId = email.id;
    composeMode = 'new';
    composeReplyTo = null;
    composeDraft = email.draftData ?? buildDraftFromEmail(email);
  }

  /** Update the draft email in the emails array from compose state */
  function handleDraftChange(d: ComposeDraft) {
    composeDraft = d;
    if (!activeDraftId) return;

    // Parse recipients from "Name <email>" format.
    // Commas inside "Last, First <email>" must not be treated as separators.
    const parseRecipients = (str: string): { name: string; email: string; initials: string; color: string }[] => {
      if (!str.trim()) return [];
      const semiParts = str.split(';');
      const parts: string[] = [];
      for (const seg of semiParts) {
        if (seg.includes('<') && seg.includes('>')) {
          parts.push(seg);
        } else {
          parts.push(...seg.split(','));
        }
      }
      return parts.map((s) => s.trim()).filter(Boolean).map((s) => {
        const match = s.match(/^(.*?)\s*<(.+)>$/);
        const name = match ? match[1].trim() : s;
        const email = match ? match[2] : s;
        const initials = name.split(' ').map((w) => w[0]?.toUpperCase() || '').join('').slice(0, 2) || '??';
        return { name, email, initials, color: '#666' };
      });
    };

    emails = emails.map((e) => {
      if (e.id !== activeDraftId) return e;
      return {
        ...e,
        to: parseRecipients(d.to),
        subject: d.subject || '(No subject)',
        preview: stripHtmlPreview(d.body),
        body: d.body,
        searchText: buildSearchText(d.body),
        hasAttachment: d.attachments.length > 0,
        date: new Date(),
        draftData: d,
      };
    });
  }

  function handlePrint() {
    // Temporarily replace the URL so the print footer doesn't show "localhost:2040"
    const originalUrl = location.href;
    try {
      history.replaceState(null, '', '/');
    } catch { /* ignore */ }
    window.print();
    try {
      history.replaceState(null, '', originalUrl);
    } catch { /* ignore */ }
  }

  function handleEmailContact(email: string, name: string) {
    activeNav = 'mail';
    const draftId = createDraftEmail();
    activeDraftId = draftId;
    composeMode = 'new';
    composeReplyTo = null;
    composeDraft = {
      to: name ? `${name} <${email}>` : email,
      cc: '', bcc: '', subject: '', body: '',
      showCc: false, showBcc: false, attachments: [],
    };
    focusedPane = 'compose';
  }

  function handleCompose(mode: ComposeMode) {
    // If already composing, the current draft is already saved in emails array via handleDraftChange
    // Just create a new draft
    const draftId = createDraftEmail();
    activeDraftId = draftId;
    composeMode = mode;
    composeReplyTo = mode === 'new' ? null : selectedEmail;
    composeDraft = null; // ComposePane will init from mode/replyTo
    focusedPane = 'compose';

    // If we're in the drafts folder, select the new draft
    if (activeFolder === 'drafts') {
      selectedEmailId = draftId;
    }
  }

  function triggerComposeSend() {
    if (composeDraft) {
      handleComposeSend(composeDraft);
    }
  }

  async function handleComposeSend(draft: { to: string; cc: string; bcc: string; subject: string; body: string; attachments: { name: string; path: string; size: number }[] }) {
    if (!activeAccountId) return;

    // Parse semicolon-separated recipient strings into email addresses.
    // Commas inside "Last, First <email>" must not be treated as separators.
    const parseAddresses = (str: string): string[] => {
      const semiParts = str.split(';');
      const parts: string[] = [];
      for (const seg of semiParts) {
        if (seg.includes('<') && seg.includes('>')) {
          parts.push(seg);
        } else {
          parts.push(...seg.split(','));
        }
      }
      return parts.map((s) => {
        const match = s.trim().match(/<(.+)>/);
        return match ? match[1] : s.trim();
      }).filter(Boolean);
    };

    const outbound: OutboundEmail = {
      to: parseAddresses(draft.to),
      cc: parseAddresses(draft.cc),
      bcc: parseAddresses(draft.bcc),
      subject: draft.subject,
      bodyHtml: draft.body,
      attachments: draft.attachments.map((a) => ({ name: a.name, path: a.path })),
    };

    // Capture state before closing the compose pane
    const draftId = activeDraftId;
    const mode = composeMode;
    const replyTo = composeReplyTo;
    const accountId = activeAccountId;

    // Mark draft as in-flight — hides it from the drafts list immediately
    if (draftId) {
      emails = emails.map((e) => e.id === draftId ? { ...e, isSending: true } : e);
    }

    // Send is irreversible — clear undo
    undoMail = null; dismissToast();

    // Close the compose pane right away so the UI is responsive and the
    // send button cannot be clicked a second time
    composeMode = null;
    composeReplyTo = null;
    composeDraft = null;
    activeDraftId = null;
    focusedPane = 'messages';

    // Select the next visible email in the current folder
    const fe = emails.filter((e) => e.folder === activeFolder && !e.isSending);
    selectedEmailId = fe[0]?.id ?? '';

    // Send in the background — draft stays hidden (isSending) while in-flight
    try {
      await sendEmail(accountId, outbound);
    } catch (e) {
      console.error('Send failed:', e);
      syncErrors = [...syncErrors, `Send failed: ${e}`];
      // Restore draft so it reappears in Drafts and the user can retry
      if (draftId) {
        emails = emails.map((em) => em.id === draftId ? { ...em, isSending: false } : em);
      }
      return;
    }

    // Mark the original email as replied (\Answered) when replying
    if ((mode === 'reply' || mode === 'replyAll') && replyTo) {
      const repliedId = replyTo.id;
      emails = emails.map((e) => e.id === repliedId ? { ...e, isReplied: true } : e);
      updateEmailReplied(accountId, repliedId, true);
    }

    // Remove the sent draft from the emails array (IMAP sync will fetch the Sent copy)
    if (draftId) {
      emails = emails.filter((e) => e.id !== draftId);
    }
  }

  function handleComposeDiscard() {
    // Remember the email we were replying to / forwarding before clearing state
    const previousEmailId = composeReplyTo?.id;

    // Remove the draft email from the emails array
    if (activeDraftId) {
      emails = emails.filter((e) => e.id !== activeDraftId);
    }
    composeMode = null;
    composeReplyTo = null;
    composeDraft = null;
    activeDraftId = null;
    focusedPane = 'messages';

    // Restore selection to the original email, or fall back to first in folder
    if (previousEmailId && emails.some((e) => e.id === previousEmailId)) {
      selectedEmailId = previousEmailId;
    } else {
      const fe = emails.filter((e) => e.folder === activeFolder);
      selectedEmailId = fe[0]?.id ?? '';
    }
  }

  function handleComposeSaveDraft() {
    // Draft is already saved via onDraftChange — just close the compose pane
    composeMode = null;
    composeReplyTo = null;
    composeDraft = null;
    activeDraftId = null;
    focusedPane = 'messages';
  }

  // Settings handlers
  function handleOpenSettings() {
    showSettings = true;
  }

  async function handleCloseSettings() {
    if (requireAccountSetup) {
      if (!accounts.some(isAccountFullyConfigured)) {
        await closeAppWindow();
        return;
      }
      requireAccountSetup = false;
    }

    showSettings = false;
  }

  function handleChangeTheme(newTheme: Theme) {
    theme = newTheme;
    saveTheme(newTheme);
  }

  function handleChangeAccentColor(colorId: string) {
    accentColor = colorId;
    saveAccentColor(colorId);
  }

  function handleChangeLocale(code: string) {
    setLocale(code);
    saveLocale(code);
  }

  async function handleUpdateAccount(updated: Account) {
    accounts = accounts.map(a => a.id === updated.id ? updated : a);
    await persistUpdateAccount(updated);
    if (updated.serverSettings) {
      await saveMailSettings(updated.id, updated.serverSettings);
    }
    if (updated.calDavSettings) {
      await saveCalDavSettings(updated.id, updated.calDavSettings);
    }
    if (updated.cardDavSettings) {
      await saveCardDavSettings(updated.id, updated.cardDavSettings);
    }
  }

  function handleDeleteAccount(accountId: string) {
    if (accounts.length <= 1) return;
    accounts = accounts.filter(a => a.id !== accountId);
    persistDeleteAccount(accountId);
    if (activeAccountId === accountId) {
      handleSelectAccount(accounts[0].id);
    }
  }

  async function handleReorderAccounts(orderedIds: string[]) {
    const byId = new Map(accounts.map(a => [a.id, a]));
    const reordered = orderedIds.map(id => byId.get(id)).filter((a): a is Account => !!a);
    if (reordered.length !== accounts.length) return;
    accounts = reordered;
    await persistAccountPositions(orderedIds);
  }

  async function handleAddAccount(data: { name: string; email: string; initials: string; color: string; avatarUrl?: string }) {
    const newId = crypto.randomUUID();
    const newAccount: Account = {
      id: newId,
      name: data.name,
      email: data.email,
      initials: data.initials,
      color: data.color,
      avatarUrl: data.avatarUrl,
      folders: [
        { id: 'inbox', name: 'Inbox', icon: 'inbox', isFavorite: true, isSystem: true },
        { id: 'sent', name: 'Sent Items', icon: 'sent', isFavorite: true, isSystem: true },
        { id: 'drafts', name: 'Drafts', icon: 'drafts', isFavorite: false, isSystem: true },
        { id: 'archive', name: 'Archive', icon: 'archive', isFavorite: false, isSystem: true },
        { id: 'deleted', name: 'Deleted Items', icon: 'trash', isFavorite: false, isSystem: true },
        { id: 'junk', name: 'Junk Email', icon: 'junk', isFavorite: false, isSystem: true },
      ],
      emails: [],
      calendarEvents: [],
      calendarCategories: [],
      contacts: [],
    };
    const nextAccounts = [...accounts, newAccount];
    accounts = nextAccounts;
    await persistAddAccount({ id: newId, name: data.name, email: data.email, initials: data.initials, color: data.color, avatarUrl: data.avatarUrl });
    applyAccountState(newId, nextAccounts);
    return newAccount;
  }

  async function handleCompleteRequiredSetup() {
    const active = accounts.find((account) => account.id === activeAccountId);
    if (!active || !isAccountFullyConfigured(active)) {
      return;
    }
    requireAccountSetup = false;
    showSettings = false;
  }

  function updateActiveAccountFolders(updater: (folders: Account['folders']) => Account['folders']) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    if (!acct) return;
    const updated = { ...acct, folders: updater(acct.folders) };
    accounts = accounts.map((a) => (a.id === updated.id ? updated : a));
  }

  function handleCreateFolder() {
    folderModal = { kind: 'create' };
  }

  function commitCreateFolder(name: string) {
    folderModal = null;

    const acct = accounts.find((a) => a.id === activeAccountId);
    if (!acct) return;

    const baseId = name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'folder';

    const existingIds = new Set(acct.folders.map((f) => f.id));
    let id = baseId;
    let suffix = 2;
    while (existingIds.has(id)) {
      id = `${baseId}-${suffix++}`;
    }

    updateActiveAccountFolders((folders) => [
      ...folders,
      { id, name, icon: 'folder', isFavorite: false, isSystem: false },
    ]);

    activeFolder = id;
    selectedEmailId = '';
  }

  function handleRenameFolder(folderId: string) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    const folder = acct?.folders.find((f) => f.id === folderId);
    if (!folder || folder.isSystem !== false) return;

    folderModal = { kind: 'rename', folderId, currentName: folder.name };
  }

  function commitRenameFolder(folderId: string, nextName: string) {
    folderModal = null;

    updateActiveAccountFolders((folders) =>
      folders.map((f) => (f.id === folderId ? { ...f, name: nextName } : f))
    );
  }

  function handleDeleteFolder(folderId: string) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    const folder = acct?.folders.find((f) => f.id === folderId);
    if (!folder || folder.isSystem !== false) return;

    folderModal = { kind: 'delete', folderId, folderName: folder.name };
  }

  function commitDeleteFolder(folderId: string) {
    folderModal = null;

    updateActiveAccountFolders((folders) => folders.filter((f) => f.id !== folderId));
    emails = emails.map((e) => (e.folder === folderId ? { ...e, folder: 'inbox' } : e));

    if (activeFolder === folderId) {
      handleSelectFolder('inbox');
    }
  }

  function handleToggleFolderFavorite(folderId: string) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    const folder = acct?.folders.find((f) => f.id === folderId);
    if (!folder) return;

    updateActiveAccountFolders((folders) =>
      folders.map((f) => (f.id === folderId ? { ...f, isFavorite: !f.isFavorite } : f))
    );
  }

  function handleEmptyFolder(folderId: string) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    const folder = acct?.folders.find((f) => f.id === folderId);
    if (!folder) return;
    folderModal = { kind: 'empty', folderId, folderName: folder.name };
  }

  async function commitEmptyFolder(folderId: string) {
    folderModal = null;
    const acct = accounts.find((a) => a.id === activeAccountId);
    if (!acct) return;

    // Optimistic: remove emails locally
    emails = emails.filter((e) => !(e.folder === folderId));
    if (selectedEmail?.folder === folderId) {
      selectedEmail = null;
    }

    try {
      await emptyFolder(acct.id, folderId);
    } catch (err) {
      console.error('empty_folder failed:', err);
    }
  }

  // Calendar event handlers
  async function handleSaveEvent(event: CalendarEvent) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    if (!acct) return;

    // Optimistic local update
    const existing = acct.calendarEvents.findIndex((e) => e.id === event.id);
    if (existing >= 0) {
      pushUndo({ view: 'calendar', action: 'editEvent', prevEvent: { ...acct.calendarEvents[existing] } }, 'Event updated — Ctrl+Z to undo');
    } else {
      pushUndo({ view: 'calendar', action: 'createEvent', eventId: event.id }, 'Event created — Ctrl+Z to undo');
    }
    let updatedEvents: CalendarEvent[];
    if (existing >= 0) {
      updatedEvents = acct.calendarEvents.map((e) => (e.id === event.id ? event : e));
    } else {
      updatedEvents = [...acct.calendarEvents, event];
    }
    const updated = { ...acct, calendarEvents: updatedEvents };
    accounts = accounts.map((a) => (a.id === updated.id ? updated : a));

    // Push to local DB (and CalDAV server if configured)
    try {
      const localId = await saveCalendarEvent(
        acct.id, event.id, event.title,
        event.start.toISOString(), event.end.toISOString(),
        event.location, event.description,
        event.isAllDay, event.calendarId,
        event.attendees ?? [],
        event.recurrence,
        event.isOnlineMeeting ?? false,
        event.meetingUrl,
        event.alertMinutes,
      );
      // If a new ID was assigned, update local state + undo entry
      if (localId && localId !== event.id) {
        const withNewId = { ...event, id: localId };
        accounts = accounts.map((a) =>
          a.id === acct.id
            ? { ...a, calendarEvents: a.calendarEvents.map((e) => (e.id === event.id ? withNewId : e)) }
            : a
        );
        if (undoCalendar?.action === 'createEvent' && undoCalendar.eventId === event.id) {
          undoCalendar = { ...undoCalendar, eventId: localId };
        }
      }
    } catch (e) {
      console.error('Failed to save event:', e);
    }
  }

  async function handleDeleteEvent(eventId: string, instanceDate?: string, deleteMode?: 'single' | 'future' | 'all') {
    const acct = accounts.find((a) => a.id === activeAccountId);
    if (!acct) return;

    const event = acct.calendarEvents.find((e) => e.id === eventId);
    if (!event) return;

    if (deleteMode === 'single' && instanceDate && event.recurrence) {
      // Add exception date — this is an edit, not a delete
      const prevEvent = { ...event, recurrence: event.recurrence ? { ...event.recurrence } : undefined };
      pushUndo({ view: 'calendar', action: 'editEvent', prevEvent }, 'Occurrence removed — Ctrl+Z to undo');
      const exdates = [...(event.recurrence.exdates ?? []), instanceDate];
      const updatedEvent = { ...event, recurrence: { ...event.recurrence, exdates } };
      accounts = accounts.map((a) =>
        a.id === acct.id ? { ...a, calendarEvents: a.calendarEvents.map((e) => e.id === eventId ? updatedEvent : e) } : a
      );
      try {
        await saveCalendarEvent(
          acct.id, event.id, event.title,
          event.start.toISOString(), event.end.toISOString(),
          event.location, event.description,
          event.isAllDay, event.calendarId,
          event.attendees ?? [],
          updatedEvent.recurrence,
          event.isOnlineMeeting ?? false, event.meetingUrl,
          event.alertMinutes,
        );
      } catch (e) { console.error('Failed to save event with exdate:', e); }
      return;
    }

    if (deleteMode === 'future' && instanceDate && event.recurrence) {
      // Truncate series: set endDate to day before this instance
      const d = new Date(instanceDate + 'T00:00:00');
      d.setDate(d.getDate() - 1);
      const endDate = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
      const prevEvent = { ...event, recurrence: event.recurrence ? { ...event.recurrence } : undefined };
      pushUndo({ view: 'calendar', action: 'editEvent', prevEvent }, 'Future occurrences removed — Ctrl+Z to undo');
      const updatedEvent = { ...event, recurrence: { ...event.recurrence, endDate } };
      accounts = accounts.map((a) =>
        a.id === acct.id ? { ...a, calendarEvents: a.calendarEvents.map((e) => e.id === eventId ? updatedEvent : e) } : a
      );
      try {
        await saveCalendarEvent(
          acct.id, event.id, event.title,
          event.start.toISOString(), event.end.toISOString(),
          event.location, event.description,
          event.isAllDay, event.calendarId,
          event.attendees ?? [],
          updatedEvent.recurrence,
          event.isOnlineMeeting ?? false, event.meetingUrl,
          event.alertMinutes,
        );
      } catch (e) { console.error('Failed to save event with truncated recurrence:', e); }
      return;
    }

    // Mode "all" or non-recurring: delete entire event
    pushUndo({ view: 'calendar', action: 'deleteEvent', prevEvent: { ...event } }, 'Event deleted — Ctrl+Z to undo');
    const updated = { ...acct, calendarEvents: acct.calendarEvents.filter((e) => e.id !== eventId) };
    accounts = accounts.map((a) => (a.id === updated.id ? updated : a));
    try {
      await deleteCalendarEvent(acct.id, eventId);
    } catch (e) {
      console.error('Failed to delete event:', e);
    }
  }

  // Contact handlers
  async function handleSaveContact(contact: FullContact) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    if (!acct) return;

    // Optimistic local update
    const existing = acct.contacts.findIndex((c) => c.id === contact.id);
    if (existing >= 0) {
      pushUndo({ view: 'contacts', action: 'editContact', prevContact: { ...acct.contacts[existing] } }, 'Contact updated — Ctrl+Z to undo');
    } else {
      pushUndo({ view: 'contacts', action: 'createContact', contactId: contact.id }, 'Contact created — Ctrl+Z to undo');
    }
    let updatedContacts: FullContact[];
    if (existing >= 0) {
      updatedContacts = acct.contacts.map((c) => (c.id === contact.id ? contact : c));
    } else {
      updatedContacts = [...acct.contacts, contact];
    }
    const updated = { ...acct, contacts: updatedContacts };
    accounts = accounts.map((a) => (a.id === updated.id ? updated : a));

    // Push to CardDAV server + local DB
    if (acct.cardDavSettings?.url) {
      try {
        const localId = await saveContactEntry(acct.id, contact);
        // If a new ID was assigned, update local state + undo entry
        if (localId && localId !== contact.id) {
          const withNewId = { ...contact, id: localId };
          accounts = accounts.map((a) =>
            a.id === acct.id
              ? { ...a, contacts: a.contacts.map((c) => (c.id === contact.id ? withNewId : c)) }
              : a
          );
          if (undoContacts?.action === 'createContact' && undoContacts.contactId === contact.id) {
            undoContacts = { ...undoContacts, contactId: localId };
          }
        }
      } catch (e) {
        console.error('Failed to save contact to CardDAV:', e);
      }
    }
  }

  async function handleDeleteContact(contactId: string) {
    const acct = accounts.find((a) => a.id === activeAccountId);
    if (!acct) return;

    const contactToDelete = acct.contacts.find((c) => c.id === contactId);
    if (contactToDelete) {
      pushUndo({ view: 'contacts', action: 'deleteContact', prevContact: { ...contactToDelete } }, 'Contact deleted — Ctrl+Z to undo');
    }

    // Optimistic local removal
    const updated = { ...acct, contacts: acct.contacts.filter((c) => c.id !== contactId) };
    accounts = accounts.map((a) => (a.id === updated.id ? updated : a));

    // Delete from CardDAV server + local DB
    if (acct.cardDavSettings?.url) {
      try {
        await deleteContactEntry(acct.id, contactId);
      } catch (e) {
        console.error('Failed to delete contact from CardDAV:', e);
      }
    }
  }

  async function handleSaveContactList(list: ContactList) {
    const existing = contactLists.findIndex(l => l.id === list.id);
    if (existing >= 0) {
      contactLists = contactLists.map(l => l.id === list.id ? list : l);
    } else {
      contactLists = [...contactLists, list];
    }
    try {
      await saveContactList(activeAccountId, list.id, list.name, list.members);
    } catch (e) {
      console.error('Failed to save contact list:', e);
    }
  }

  async function handleDeleteContactList(listId: string) {
    contactLists = contactLists.filter(l => l.id !== listId);
    try {
      await deleteContactList(listId);
    } catch (e) {
      console.error('Failed to delete contact list:', e);
    }
  }

  // Trigger counters — bumped to signal child components to open modals
  let newEventTrigger = $state(0);
  let prefillMeeting = $state<{ title: string; attendees: import('$lib/types').EventAttendee[] } | null>(null);
  let newContactTrigger = $state(0);
  let newContactListTrigger = $state(0);
  let editContactTrigger = $state(0);
  let deleteContactTrigger = $state(0);
  let selectedContactReadOnly = $state(false);
  let contactsInListMode = $state(false);
  let contactsFocusedPane = $state<'nav' | 'list' | 'detail' | null>('list');
  let contactsRequestFocusPane = $state<'nav' | 'list' | 'detail' | 'none' | null>(null);
  let contactsViewRef = $state<{ navigateArrow: (key: string) => void; emailSelected: () => void; meetSelected: () => void; callSelected: () => void; toggleSelectedFavorite: () => void; toggleShowMuted: () => void } | null>(null);
  let calendarViewRef = $state<{ navigateCalList: (key: string) => 'at-top' | 'at-bottom' | 'moved'; navigateMiniCal: (key: string) => void; navigateMainCal: (key: string) => void; goToToday: () => void; focusSearchResults: () => void; navigateSearchResults: (key: string) => boolean; toggleFocusedCalListItem: () => boolean; editSearchSelectedEvent: () => void; deleteSearchSelectedEvent: () => void; joinMeeting: () => boolean; hasDetailModal: () => boolean } | null>(null);
  let messageListRef = $state<{ cycleInboxTab: () => boolean } | null>(null);
  let readingPaneRef = $state<{ focus: () => void; scroll: (delta: number) => void; toggleHeaders: () => void } | null>(null);
  let composePaneRef = $state<{ focus: () => void } | null>(null);
  let titleBarRef = $state<{ focusSearch: () => void } | null>(null);

  let prevSearchFocus = $state<{ focusedPane: typeof focusedPane; calInnerPane: typeof calInnerPane; selectedEmailId: string; selectedContactId: string; activeNav: typeof activeNav } | null>(null);

  function handleSearchEsc() {
    if (!prevSearchFocus || prevSearchFocus.activeNav !== activeNav) { prevSearchFocus = null; return; }
    const prev = prevSearchFocus;
    prevSearchFocus = null;
    focusedPane = prev.focusedPane;
    calInnerPane = prev.calInnerPane;
    selectedEmailId = prev.selectedEmailId;
    selectedContactId = prev.selectedContactId;
    if (prev.activeNav === 'contacts') contactsRequestFocusPane = 'list';
  }

  function handleSearchTab() {
    if (activeNav === 'mail') {
      focusedPane = 'messages';
      if (messageVisibleList.length > 0) {
        selectedEmailId = messageVisibleList[0].id;
      } else {
        selectedEmailId = '';
      }
    } else if (activeNav === 'calendar') {
      focusedPane = 'cal-main';
      calInnerPane = 'none';
      calendarViewRef?.focusSearchResults();
    } else if (activeNav === 'contacts') {
      focusedPane = 'folders';
      contactsRequestFocusPane = 'list';
      if (contactVisibleList.length > 0) {
        selectedContactId = contactVisibleList[0].id;
      } else {
        selectedContactId = '';
      }
    }
  }

  // Folder modal state
  type FolderModalState =
    | { kind: 'create' }
    | { kind: 'rename'; folderId: string; currentName: string }
    | { kind: 'delete'; folderId: string; folderName: string }
    | { kind: 'empty'; folderId: string; folderName: string }
    | null;
  let folderModal = $state<FolderModalState>(null);
</script>

{#if startupStatus === 'loading'}
  <div class="startup-empty-state">
    <p>Loading Mail…</p>
  </div>
{:else}
{#if activeAccount}
<div class="app-layout">
  <TitleBar
    bind:this={titleBarRef}
    {activeNav}
    {folderPaneVisible}
    onToggleFolderPane={() => (folderPaneVisible = !folderPaneVisible)}
    {searchQuery}
    onSearch={(q) => (searchQuery = q)}
    {accounts}
    {activeAccountId}
    onSelectAccount={handleSelectAccount}
    onSearchTab={handleSearchTab}
    onSearchEsc={handleSearchEsc}
    searchInfoText={searchInfoText}
  />

  <div class="app-body">
    <NavigationRail 
      activeItem={activeNav} 
      focused={focusedPane === 'rail'} 
      onSelectItem={(item) => { activeNav = item; newEventTrigger = 0; newContactTrigger = 0; newContactListTrigger = 0; editContactTrigger = 0; deleteContactTrigger = 0; contactsInListMode = false; focusedPane = 'rail'; contactsRequestFocusPane = 'none'; calInnerPane = 'none'; }} 
      onOpenSettings={handleOpenSettings}
      />

    <div class="app-main">
      <CommandBar
        activeView={activeNav}
        email={selectedEmail}
        currentFolder={activeFolder}
        folders={activeAccount?.folders ?? []}
        onCompose={handleCompose}
        onMarkRead={handleMarkSelectedRead}
        onMarkUnread={handleMarkSelectedUnread}
        onDelete={handleDeleteSelectedEmail}
        onArchive={handleArchiveSelectedEmail}
        onJunk={handleJunkSelectedEmail}
        onMove={handleMoveEmail}
        onPrint={handlePrint}
        {showAllHeaders}
        onToggleHeaders={() => (showAllHeaders = !showAllHeaders)}
        onUndo={performUndo}
        canUndo={activeNav === 'mail' ? !!undoMail : activeNav === 'calendar' ? !!undoCalendar : activeNav === 'contacts' ? !!undoContacts : false}
        onSync={activeNav === 'calendar' ? handleSyncCalendars : activeNav === 'contacts' ? handleSyncContacts : handleSyncCurrentAccount}
        {syncing}
        hasDav={activeNav === 'calendar' ? !!activeAccount?.calDavSettings?.url : !!activeAccount?.cardDavSettings?.url}
        {calendarViewMode}
        onChangeCalendarViewMode={(mode) => (calendarViewMode = mode)}
        onNewEvent={() => (newEventTrigger++)}
        onNewContact={() => { if (activeNav === 'contacts' && contactsInListMode) newContactListTrigger++; else newContactTrigger++; }}
        onEditContact={() => (editContactTrigger++)}
        onDeleteContact={() => (deleteContactTrigger++)}
        contactReadOnly={activeNav === 'contacts' && selectedContactReadOnly}
        contactListMode={activeNav === 'contacts' && contactsInListMode}
        bind:showMoveMenu
        {multiSelectCount}
        {multiSelectHasUnread}
      />

      <div class="network-indicator" class:active={networkActivity.count > 0} aria-hidden="true"></div>

      {#if syncErrors.length > 0}
        <div class="sync-error-banner">
          {#each syncErrors as err}
            <p>{err}</p>
          {/each}
          <button class="sync-error-dismiss" onclick={() => (syncErrors = [])}>Dismiss</button>
        </div>
      {/if}

      <div class="app-content">
        {#if activeNav === 'mail'}
          <div style="display:contents">
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div style="display:contents" onmousedown={() => (focusedPane = 'folders')}>
            {#if folderPaneVisible}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <FolderPane folders={activeAccount.folders}
                {activeFolder}
                onSelectFolder={handleSelectFolder}
                {unreadCounts}
                onCreateFolder={handleCreateFolder}
                onRenameFolder={handleRenameFolder}
                onDeleteFolder={handleDeleteFolder}
                onToggleFolderFavorite={handleToggleFolderFavorite}
                onEmptyFolder={handleEmptyFolder}
                {folderEmailCounts}             
                focused={focusedPane === 'folders'}
                />
            {/if}
            </div>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div style="display:contents" onmousedown={() => (focusedPane = 'messages')}>
                <MessageList
                bind:this={messageListRef}
                emails={filteredEmails}
                {selectedEmail}
                currentFolder={activeFolder}
                folderName={activeAccount?.folders.find((f) => f.id === activeFolder)?.name ?? activeFolder}
                onSelectEmail={handleSelectEmail}
                onOpenDraft={(email) => openDraftInCompose(email)}
                onToggleStar={handleToggleStar}
                onTogglePin={handleTogglePin}
                onToggleFocused={handleToggleFocused}
                onDeleteEmail={handleDeleteEmail}
                onClearSelection={() => (selectedEmailId = '')}
                onPreviewMissing={handlePreviewMissing}
                bind:visibleList={messageVisibleList}
                focused={focusedPane === 'messages'}
                bind:checkedIds={checkedEmailIds}
            />
            </div>
          </div>
          {#if composeMode}
            {#key activeDraftId}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div style="display:contents" onmousedown={() => (focusedPane = 'compose')}><ComposePane
              bind:this={composePaneRef}
              mode={composeMode}
              replyTo={composeReplyTo}
              draft={composeDraft}
              accountId={activeAccountId}
              signature={activeAccount?.signature ?? ''}
              onDraftChange={handleDraftChange}
              onSend={handleComposeSend}
              onDiscard={handleComposeDiscard}
              onSaveDraft={handleComposeSaveDraft}
            /></div>
            {/key}
          {:else}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div style="display:contents" onmousedown={() => (focusedPane = 'reading')}><ReadingPane bind:this={readingPaneRef} email={selectedEmail} loadingBody={fetchingBodyId === selectedEmailId} darkMode={resolvedTheme === 'dark'} isJunk={activeFolder === 'junk'} focused={focusedPane === 'reading'} onFocus={() => (focusedPane = 'reading')} onOpenAttachment={handleOpenAttachment} onSaveAttachment={handleSaveAttachment} bind:showAllHeaders {multiSelectCount} onLabelClick={handleLabelClick} /></div>
          {/if}
        {:else if activeNav === 'calendar'}
          <CalendarView
            bind:this={calendarViewRef}
            events={activeAccount.calendarEvents}
            categories={calendarCategories}
            viewMode={calendarViewMode}
            contacts={activeAccount.contacts}
            onSaveEvent={handleSaveEvent}
            onDeleteEvent={handleDeleteEvent}
            requestNewEvent={newEventTrigger}
            onResetNewEvent={() => (newEventTrigger = 0)}
            {prefillMeeting}
            onResetPrefillMeeting={() => (prefillMeeting = null)}
            {searchQuery}
            onClearSearch={() => (searchQuery = '')}
            calFocusedPane={focusedPane === 'cal-sidebar' || focusedPane === 'cal-main' ? focusedPane : 'none'}
            {calInnerPane}
            onFocusPaneRequest={(pane) => {
              if (pane === 'cal-mini-inner') {
                focusedPane = 'cal-sidebar';
                calInnerPane = 'cal-mini-inner';
              }
              else if (pane === 'cal-list-inner') {
                focusedPane = 'cal-sidebar';
                calInnerPane = 'cal-list-inner';
              }
              else if (pane === 'cal-main') {
                focusedPane = 'cal-main';
                calInnerPane = 'cal-main-inner';
              }
              else calInnerPane = 'none';
            }}
          />
        {:else if activeNav === 'contacts'}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div style="display:contents" role="region" onmousedown={() => { if (focusedPane === 'rail') focusedPane = 'folders'; }}>
            <ContactsView
            bind:this={contactsViewRef}
            contacts={activeAccount.contacts}
            {mutedAddresses}
            onToggleMute={toggleMuteAddress}
            onSaveContact={handleSaveContact}
            onDeleteContact={handleDeleteContact}
            onEmailContact={handleEmailContact}
            requestNewContact={newContactTrigger}
            requestNewContactList={newContactListTrigger}
            requestEditContact={editContactTrigger}
            requestDeleteContact={deleteContactTrigger}
            onResetContactTriggers={() => { newContactTrigger = 0; newContactListTrigger = 0; editContactTrigger = 0; deleteContactTrigger = 0; }}
            onSelectedContactChange={(c) => { selectedContactReadOnly = c?.isReadOnly ?? false; }}
            {contactLists}
            onSaveContactList={handleSaveContactList}
            onDeleteContactList={handleDeleteContactList}
            onListModeChange={(v) => (contactsInListMode = v)}
            onMeetContact={(contact) => {
              prefillMeeting = {
                title: 'Meeting',
                attendees: [{ name: contact.name, email: contact.email, initials: contact.initials, color: contact.color, role: 'required' }],
              };
              activeNav = 'calendar';
            }}
            onCallContact={(phone) => { shellOpen(`tel:${phone}`).catch(() => {}); }}
            onToggleFavorite={(contactId, isFavorite) => {
              const acct = accounts.find((a) => a.id === activeAccountId);
              if (!acct) return;
              accounts = accounts.map((a) => a.id === acct.id ? { ...a, contacts: a.contacts.map((c) => c.id === contactId ? { ...c, isFavorite } : c) } : a);
            }}
            onFocusPaneChange={(p) => { contactsFocusedPane = p; contactsRequestFocusPane = null; }}
            requestFocusPane={contactsRequestFocusPane}
            {searchQuery}
            selectedContactId={selectedContactId}
            onSelectedContactIdChange={(id) => (selectedContactId = id)}
            bind:filteredContactsList={contactVisibleList}
            selectedListId={selectedListId}
            onSelectedListIdChange={(id) => (selectedListId = id)}
          /></div>
        {/if}
      </div>
    </div>
  </div>
</div>
{/if}

{#if showSettings || requireAccountSetup || accounts.length === 0}
  <SettingsView
    {theme}
    {accentColor}
    {accounts}
    locale={locale()}
    languageNames={LANGUAGE_NAMES}
    initialTab={requireAccountSetup ? 'accounts' : 'general'}
    requireAccount={requireAccountSetup}
    onChangeTheme={handleChangeTheme}
    onChangeAccentColor={handleChangeAccentColor}
    onChangeLocale={handleChangeLocale}
    onUpdateAccount={handleUpdateAccount}
    onDeleteAccount={handleDeleteAccount}
    onAddAccount={handleAddAccount}
    onReorderAccounts={handleReorderAccounts}
    onCompleteRequiredSetup={handleCompleteRequiredSetup}
    onClose={handleCloseSettings}
  />
{/if}

{#if !activeAccount && !showSettings && !requireAccountSetup}
  <div class="startup-empty-state">
    <p>{startupError ?? 'No account is available.'}</p>
    <button class="startup-action" onclick={() => { requireAccountSetup = true; showSettings = true; }}>Open account setup</button>
  </div>
{/if}
{/if}

{#if folderModal?.kind === 'create'}
  <FolderModal
    kind="prompt"
    title="Create new folder"
    placeholder="Folder name"
    confirmLabel="Create"
    onSubmit={(name) => commitCreateFolder(name)}
    onCancel={() => (folderModal = null)}
  />
{:else if folderModal?.kind === 'rename'}
  {@const renameId = folderModal.folderId}
  <FolderModal
    kind="prompt"
    title="Rename folder"
    placeholder="Folder name"
    initialValue={folderModal.currentName}
    confirmLabel="Rename"
    onSubmit={(name) => commitRenameFolder(renameId, name)}
    onCancel={() => (folderModal = null)}
  />
{:else if folderModal?.kind === 'delete'}
  {@const deleteId = folderModal.folderId}
  {@const deleteName = folderModal.folderName}
  <FolderModal
    kind="confirm"
    title="Delete folder"
    message={`Are you sure you want to delete "${deleteName}"? Emails in this folder will be moved to Inbox.`}
    confirmLabel="Delete"
    dangerConfirm={true}
    onSubmit={() => commitDeleteFolder(deleteId)}
    onCancel={() => (folderModal = null)}
  />
{:else if folderModal?.kind === 'empty'}
  {@const emptyId = folderModal.folderId}
  {@const emptyName = folderModal.folderName}
  <FolderModal
    kind="confirm"
    title="Empty folder"
    message={`Are you sure you want to permanently delete all emails in "${emptyName}"? This cannot be undone.`}
    confirmLabel="Empty"
    dangerConfirm={true}
    onSubmit={() => commitEmptyFolder(emptyId)}
    onCancel={() => (folderModal = null)}
  />
{/if}

{#if toastMessage}
  <div class="undo-toast" role="status">
    <span>{toastMessage}</span>
    <button class="undo-toast-btn" onclick={() => { performUndo(); }}>Undo</button>
    <button class="undo-toast-dismiss" onclick={dismissToast} aria-label="Dismiss">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
      </svg>
    </button>
  </div>
{/if}

<style>
  .app-layout {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    background: var(--bg-primary);
  }

  .app-body {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .app-main {
    display: flex;
    flex-direction: column;
    flex: 1;
    gap: 1px;
    min-width: 0;
    padding: 4px 0 5px 4px;
    background-color: var(--bg-primary);
  }

  .network-indicator {
    height: 2px;
    overflow: hidden;
    position: relative;
    flex-shrink: 0;
  }

  .network-indicator.active::before {
    content: '';
    position: absolute;
    inset: 0;
    background-image: linear-gradient(90deg, transparent 0%, var(--accent) 50%, transparent 100%);
    background-size: 50% 100%;
    background-repeat: repeat;
    animation: network-slide 1.2s linear infinite;
  }

  @keyframes network-slide {
    0%   { background-position: 0 0; }
    100% { background-position: 100% 0; }
  }

  .sync-error-banner {
    background: #fde7e9;
    border-bottom: 1px solid #d13438;
    color: #a4262c;
    padding: 6px 12px;
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .sync-error-banner p {
    margin: 0;
  }

  .sync-error-dismiss {
    margin-left: auto;
    background: none;
    border: 1px solid #a4262c;
    color: #a4262c;
    border-radius: 4px;
    padding: 2px 8px;
    cursor: pointer;
    font-size: 11px;
  }

  :global([data-theme="dark"]) .sync-error-banner {
    background: #442726;
    border-color: #d13438;
    color: #f1bbbc;
  }

  :global([data-theme="dark"]) .sync-error-dismiss {
    border-color: #f1bbbc;
    color: #f1bbbc;
  }

  .app-content {
    display: flex;
    flex: 1;
    overflow: hidden;
    gap: 4px;
  }

  .startup-empty-state {
    height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    background: var(--bg-primary);
    color: var(--text-secondary);
  }

  .startup-action {
    border: 1px solid var(--border-light);
    background: var(--bg-secondary);
    color: var(--text-primary);
    padding: 10px 16px;
    border-radius: 8px;
    cursor: pointer;
    font: inherit;
  }

  .startup-action:hover {
    background: var(--bg-hover);
  }

  /* ── Undo Toast ── */
  .undo-toast {
    position: fixed;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-tertiary, #333);
    color: var(--text-primary);
    padding: 10px 16px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 12px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
    z-index: 9999;
    font-size: 13px;
    animation: toast-slide-up 0.2s ease-out;
  }

  .undo-toast-btn {
    color: var(--accent);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    padding: 2px 8px;
    border-radius: 4px;
  }

  .undo-toast-btn:hover {
    background: var(--bg-hover);
  }

  .undo-toast-dismiss {
    color: var(--text-tertiary);
    cursor: pointer;
    display: flex;
    align-items: center;
    padding: 2px;
    border-radius: 4px;
  }

  .undo-toast-dismiss:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  @keyframes toast-slide-up {
    from { opacity: 0; transform: translateX(-50%) translateY(10px); }
    to   { opacity: 1; transform: translateX(-50%) translateY(0); }
  }
</style>
