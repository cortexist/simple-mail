<script lang="ts">
  import { tick } from 'svelte';
  import type { Email, Folder, NavItem, CalendarViewMode, ComposeMode } from '$lib/types';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    activeView: NavItem;
    email: Email | null;
    currentFolder: string;
    folders: Folder[];
    onCompose: (mode: ComposeMode) => void;
    onMarkRead?: () => void;
    onMarkUnread?: () => void;
    onDelete?: () => void;
    onArchive?: () => void;
    onJunk?: () => void;
    onMove?: (targetFolder: string) => void;
    onPrint?: () => void;
    onToggleHeaders?: () => void;
    showAllHeaders?: boolean;
    onSync?: () => void;
    onUndo?: () => void;
    canUndo?: boolean;
    syncing?: boolean;
    hasDav?: boolean;
    calendarViewMode?: CalendarViewMode;
    onChangeCalendarViewMode?: (mode: CalendarViewMode) => void;
    onNewEvent?: () => void;
    onNewContact?: () => void;
    onEditContact?: () => void;
    onDeleteContact?: () => void;
    contactReadOnly?: boolean;
    contactListMode?: boolean;
    showMoveMenu?: boolean;
    multiSelectCount?: number;
    multiSelectHasUnread?: boolean;
  }

  let {
    activeView,
    email,
    currentFolder,
    folders,
    onCompose,
    onMarkRead,
    onMarkUnread,
    onDelete,
    onArchive,
    onJunk,
    onMove,
    onPrint,
    onToggleHeaders,
    showAllHeaders = false,
    onSync,
    onUndo,
    canUndo = false,
    syncing = false,
    hasDav = false,
    calendarViewMode = 'week',
    onChangeCalendarViewMode,
    onNewEvent,
    onNewContact,
    onEditContact,
    onDeleteContact,
    contactReadOnly,
    contactListMode = false,
    showMoveMenu = $bindable(false),
    multiSelectCount = 0,
    multiSelectHasUnread = false,
  }: Props = $props();

  let moveTargetFolders = $derived(folders.filter((f) => f.id !== currentFolder));
  let isInJunk = $derived(currentFolder === 'junk');

  type CmdBtn = {
    kind: 'btn';
    id: string;
    iconKey: string;
    label?: string;
    tooltip?: string;
    tooltipPosition?: string;
    onClick: () => void;
    disabled?: boolean;
    danger?: boolean;
    active?: boolean;
    primary?: boolean;
    iconOnly?: boolean;
    ariaLabel?: string;
    isMove?: boolean;
    isSync?: boolean;
  };
  type CmdSep = { kind: 'sep'; id: string };
  type CmdCalToggle = { kind: 'caltoggle'; id: string };
  type CmdSpacer = { kind: 'spacer'; id: string };
  type Cmd = CmdBtn | CmdSep | CmdCalToggle | CmdSpacer;

  let commands = $derived.by((): Cmd[] => {
    const left: Cmd[] = [];
    const right: Cmd[] = [];

    if (activeView === 'mail') {
      left.push({ kind: 'btn', id: 'newMail', primary: true, iconKey: 'compose', label: t('compose.newMail'), tooltip: t('commandBar.newMail'), tooltipPosition: 'bottom', onClick: () => onCompose('new') });
      left.push({ kind: 'sep', id: 's1' });
      if (!email && multiSelectCount > 0) {
        if (multiSelectHasUnread) {
          left.push({ kind: 'btn', id: 'markRead', iconKey: 'read', label: t('commandBar.read'), tooltip: t('commandBar.markRead'), tooltipPosition: 'bottom', onClick: () => onMarkRead?.() });
        } else {
          left.push({ kind: 'btn', id: 'markUnread', iconKey: 'unread', label: t('commandBar.unread'), tooltip: t('commandBar.markUnread'), tooltipPosition: 'bottom', onClick: () => onMarkUnread?.() });
        }
        left.push({ kind: 'sep', id: 's2' });
        left.push({ kind: 'btn', id: 'delete', danger: true, iconKey: 'trash', label: t('common.delete'), tooltip: t('commandBar.delete'), tooltipPosition: 'bottom', onClick: () => onDelete?.() });
        left.push({ kind: 'btn', id: 'archive', iconKey: 'archive', label: t('commandBar.archive'), tooltip: t('commandBar.archiveShortcut'), tooltipPosition: 'bottom', onClick: () => onArchive?.() });
        left.push({ kind: 'btn', id: 'junk', iconKey: isInJunk ? 'notJunk' : 'junk', label: isInJunk ? t('commandBar.notJunk') : t('commandBar.junk'), tooltip: isInJunk ? t('commandBar.notJunkTooltip') : t('commandBar.junkTooltip'), tooltipPosition: 'bottom', onClick: () => onJunk?.() });
        left.push({ kind: 'btn', id: 'move', isMove: true, iconKey: 'move', label: t('commandBar.move'), tooltip: t('commandBar.moveTo'), tooltipPosition: 'bottom', onClick: () => {} });
      } else if (email) {
        left.push({ kind: 'btn', id: 'reply', iconKey: 'reply', label: t('compose.reply'), tooltip: t('commandBar.reply'), tooltipPosition: 'bottom', onClick: () => onCompose('reply') });
        left.push({ kind: 'btn', id: 'replyAll', iconKey: 'replyAll', label: t('compose.replyAll'), tooltip: t('commandBar.replyAll'), tooltipPosition: 'bottom', onClick: () => onCompose('replyAll') });
        left.push({ kind: 'btn', id: 'forward', iconKey: 'forward', label: t('compose.forward'), tooltip: t('commandBar.forward'), tooltipPosition: 'bottom', onClick: () => onCompose('forward') });
        if (email.isRead) {
          left.push({ kind: 'btn', id: 'markUnread', iconKey: 'unread', label: t('commandBar.unread'), tooltip: t('commandBar.markUnread'), tooltipPosition: 'bottom', onClick: () => onMarkUnread?.(), disabled: email.folder === 'drafts' });
        } else {
          left.push({ kind: 'btn', id: 'markRead', iconKey: 'read', label: t('commandBar.read'), tooltip: t('commandBar.markRead'), tooltipPosition: 'bottom', onClick: () => onMarkRead?.(), disabled: email.folder === 'drafts' });
        }
        left.push({ kind: 'sep', id: 's2' });
        left.push({ kind: 'btn', id: 'delete', danger: true, iconKey: 'trash', label: t('common.delete'), tooltip: t('commandBar.delete'), tooltipPosition: 'bottom', onClick: () => onDelete?.() });
        left.push({ kind: 'btn', id: 'archive', iconKey: 'archive', label: t('commandBar.archive'), tooltip: t('commandBar.archiveShortcut'), tooltipPosition: 'bottom', onClick: () => onArchive?.() });
        left.push({ kind: 'btn', id: 'junk', iconKey: isInJunk ? 'notJunk' : 'junk', label: isInJunk ? t('commandBar.notJunk') : t('commandBar.junk'), tooltip: isInJunk ? t('commandBar.notJunkTooltip') : t('commandBar.junkTooltip'), tooltipPosition: 'bottom', onClick: () => onJunk?.() });
        left.push({ kind: 'btn', id: 'move', isMove: true, iconKey: 'move', label: t('commandBar.move'), tooltip: t('commandBar.moveTo'), tooltipPosition: 'bottom', onClick: () => {} });
      } else {
        left.push({ kind: 'btn', id: 'reply-d', iconKey: 'replyStub', label: t('compose.reply'), tooltip: t('compose.reply'), tooltipPosition: 'bottom', onClick: () => {}, disabled: true });
        left.push({ kind: 'btn', id: 'replyAll-d', iconKey: 'replyAllStub', label: t('compose.replyAll'), tooltip: t('compose.replyAll'), tooltipPosition: 'bottom', onClick: () => {}, disabled: true });
        left.push({ kind: 'btn', id: 'forward-d', iconKey: 'forwardStub', label: t('compose.forward'), tooltip: t('compose.forward'), tooltipPosition: 'bottom', onClick: () => {}, disabled: true });
        left.push({ kind: 'btn', id: 'delete-d', iconKey: 'trashStub', label: t('common.delete'), tooltip: t('common.delete'), tooltipPosition: 'bottom', onClick: () => {}, disabled: true });
      }
    } else if (activeView === 'calendar') {
      left.push({ kind: 'btn', id: 'newEvent', primary: true, iconKey: 'newEvent', label: t('commandBar.newEvent'), tooltip: t('commandBar.newEventShortcut'), tooltipPosition: 'bottom', onClick: () => onNewEvent?.() });
      left.push({ kind: 'caltoggle', id: 'caltoggle' });
    } else if (activeView === 'contacts') {
      left.push({ kind: 'btn', id: 'newContact', primary: true, iconKey: contactListMode ? 'newContactList' : 'newContact', label: contactListMode ? t('commandBar.newContactList') : t('commandBar.newContact'), tooltip: contactListMode ? t('commandBar.newContactListShortcut') : t('commandBar.newContactShortcut'), tooltipPosition: 'bottom', onClick: () => onNewContact?.() });
      left.push({ kind: 'sep', id: 's1' });
      left.push({ kind: 'btn', id: 'editContact', iconKey: 'edit', label: t('common.edit'), tooltip: contactReadOnly ? t('commandBar.readOnly') : t('commandBar.editContact'), tooltipPosition: 'bottom', onClick: () => onEditContact?.(), disabled: contactReadOnly });
      left.push({ kind: 'btn', id: 'deleteContact', danger: true, iconKey: 'trash', label: t('common.delete'), tooltip: contactReadOnly ? t('commandBar.readOnly') : t('commandBar.delete'), tooltipPosition: 'bottom', onClick: () => onDeleteContact?.(), disabled: contactReadOnly });
    }

    if (canUndo) {
      right.push({ kind: 'btn', id: 'undo', iconKey: 'undo', label: t('common.undo'), tooltip: t('commandBar.undoShortcut'), tooltipPosition: 'bottom', ariaLabel: t('common.undo'), onClick: () => onUndo?.() });
    }
    if (activeView === 'mail') {
      right.push({ kind: 'btn', id: 'headers', iconKey: 'headers', label: t('commandBar.headers'), tooltip: showAllHeaders ? t('commandBar.hideHeaders') : t('commandBar.showHeaders'), tooltipPosition: 'bottom', active: showAllHeaders, ariaLabel: showAllHeaders ? t('commandBar.hideHeaders') : t('commandBar.showHeaders'), onClick: () => onToggleHeaders?.() });
      right.push({ kind: 'btn', id: 'print', iconKey: 'print', label: t('commandBar.print'), tooltip: t('commandBar.printShortcut'), tooltipPosition: 'bottom', ariaLabel: t('commandBar.print'), onClick: () => onPrint?.() });
    }
    if (activeView === 'mail' || ((activeView === 'calendar' || activeView === 'contacts') && hasDav)) {
      right.push({ kind: 'btn', id: 'sync', iconKey: 'sync', label: syncing ? t('commandBar.syncing') : t('commandBar.syncLabel'), tooltip: syncing ? t('commandBar.syncing') : t('commandBar.sync'), tooltipPosition: 'bottom', ariaLabel: syncing ? t('commandBar.syncing') : t('commandBar.syncLabel'), disabled: syncing, isSync: true, onClick: () => onSync?.() });
    }

    return [...left, { kind: 'spacer', id: 'spacer' }, ...right];
  });

  let rightGroupIds = $derived.by(() => {
    const set = new Set<string>();
    let pastSpacer = false;
    for (const c of commands) {
      if (c.kind === 'spacer') { pastSpacer = true; continue; }
      if (pastSpacer && c.kind === 'btn') set.add(c.id);
    }
    return set;
  });

  function cmdGroup(c: Cmd): 'left' | 'right' {
    return c.kind === 'btn' && rightGroupIds.has(c.id) ? 'right' : 'left';
  }

  function isCollapsible(c: Cmd): boolean {
    if (c.kind !== 'btn') return false;
    if (c.primary) return false;
    return true;
  }

  let overflowedIds = $state<Set<string>>(new Set());
  let menuItems = $derived(commands.filter((c): c is CmdBtn => c.kind === 'btn' && overflowedIds.has(c.id)));
  let hasOverflow = $derived(overflowedIds.size > 0);

  let barEl: HTMLElement | null = $state(null);
  let showMoreMenu = $state(false);

  let recalcing = false;
  async function recalc() {
    if (recalcing || !barEl) return;
    recalcing = true;
    try {
      // Phase 1 — show every item, hide More, measure natural width.
      if (overflowedIds.size > 0) overflowedIds = new Set();
      await tick();
      if (!barEl) return;
      // The More button is always laid out (visibility:hidden when unused), so its
      // width is already in scrollWidth — no extra reservation needed.
      const avail = barEl.clientWidth;
      let need = barEl.scrollWidth;
      if (need <= avail) return;

      const els = Array.from(barEl.querySelectorAll<HTMLElement>('[data-collapsible="1"]'));
      const left = els.filter((el) => el.dataset.cmdGroup === 'left');
      const right = els.filter((el) => el.dataset.cmdGroup === 'right');
      // Overflow left group first (rightmost → leftmost), then right group as a fallback.
      const order = [...left.reverse(), ...right.reverse()];
      const newSet = new Set<string>();
      for (const el of order) {
        if (need <= avail) break;
        const id = el.dataset.cmdId;
        if (!id) continue;
        newSet.add(id);
        need -= el.offsetWidth + 1; // gap is 1px
      }
      overflowedIds = newSet;
    } finally {
      recalcing = false;
    }
  }

  $effect(() => {
    void commands;
    if (!barEl) return;
    recalc();
    const ro = new ResizeObserver(() => recalc());
    ro.observe(barEl);
    return () => ro.disconnect();
  });

  function handleWindowClick() {
    showMoveMenu = false;
    showMoreMenu = false;
  }
</script>

<svelte:window onclick={handleWindowClick} />

{#snippet icon(key: string)}
  {#if key === 'compose'}
    <svg width="16" height="16" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="M5.076 17C4.089 4.545 12.912 1.012 19.973 2.224c.286 4.128-1.734 5.673-5.58 6.387c.742.776 2.055 1.753 1.913 2.974c-.1.868-.69 1.295-1.87 2.147C11.85 15.6 8.854 16.78 5.076 17"/><path d="M4 22c0-6.5 3.848-9.818 6.5-12"/></g></svg>
  {:else if key === 'read'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M13.19 2.34a2.25 2.25 0 0 0-2.38 0L3.06 7.172A2.25 2.25 0 0 0 2 9.083v7.667A3.25 3.25 0 0 0 5.25 20h13.5A3.25 3.25 0 0 0 22 16.75V9.082c0-.776-.4-1.498-1.06-1.909zm-1.587 1.272a.75.75 0 0 1 .794 0l7.242 4.517L12 12.15L4.361 8.13zM3.5 9.371l8.15 4.29a.75.75 0 0 0 .7 0l8.15-4.29v7.379a1.75 1.75 0 0 1-1.75 1.75H5.25a1.75 1.75 0 0 1-1.75-1.75z"/></svg>
  {:else if key === 'unread'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M5.25 4h13.5a3.25 3.25 0 0 1 3.245 3.066L22 7.25v9.5a3.25 3.25 0 0 1-3.066 3.245L18.75 20H5.25a3.25 3.25 0 0 1-3.245-3.066L2 16.75v-9.5a3.25 3.25 0 0 1 3.066-3.245zh13.5zM20.5 9.373l-8.15 4.29a.75.75 0 0 1-.603.043l-.096-.042L3.5 9.374v7.376a1.75 1.75 0 0 0 1.606 1.744l.144.006h13.5a1.75 1.75 0 0 0 1.744-1.607l.006-.143zM18.75 5.5H5.25a1.75 1.75 0 0 0-1.744 1.606L3.5 7.25v.429l8.5 4.474l8.5-4.475V7.25a1.75 1.75 0 0 0-1.607-1.744z"/></svg>
  {:else if key === 'trash'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M10 5h4a2 2 0 1 0-4 0M8.5 5a3.5 3.5 0 1 1 7 0h5.75a.75.75 0 0 1 0 1.5h-1.32l-1.17 12.111A3.75 3.75 0 0 1 15.026 22H8.974a3.75 3.75 0 0 1-3.733-3.389L4.07 6.5H2.75a.75.75 0 0 1 0-1.5zm2 4.75a.75.75 0 0 0-1.5 0v7.5a.75.75 0 0 0 1.5 0zM14.25 9a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-1.5 0v-7.5a.75.75 0 0 1 .75-.75m-7.516 9.467a2.25 2.25 0 0 0 2.24 2.033h6.052a2.25 2.25 0 0 0 2.24-2.033L18.424 6.5H5.576z"/></svg>
  {:else if key === 'archive'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M10.25 11a.75.75 0 0 0 0 1.5h3.5a.75.75 0 0 0 0-1.5zM3 5.25A2.25 2.25 0 0 1 5.25 3h13.5A2.25 2.25 0 0 1 21 5.25v1.5c0 .78-.397 1.467-1 1.871v8.629A3.75 3.75 0 0 1 16.25 21h-8.5A3.75 3.75 0 0 1 4 17.25V8.621A2.25 2.25 0 0 1 3 6.75zM5.5 9v8.25a2.25 2.25 0 0 0 2.25 2.25h8.5a2.25 2.25 0 0 0 2.25-2.25V9zm-.25-4.5a.75.75 0 0 0-.75.75v1.5c0 .414.336.75.75.75h13.5a.75.75 0 0 0 .75-.75v-1.5a.75.75 0 0 0-.75-.75z"/></svg>
  {:else if key === 'junk'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75zM23 17.5a5.5 5.5 0 1 1-11 0a5.5 5.5 0 0 1 11 0m-9.5 0c0 .834.255 1.608.691 2.248l5.557-5.557A4 4 0 0 0 13.5 17.5m4 4a4 4 0 0 0 3.309-6.248l-5.557 5.557a4 4 0 0 0 2.248.691"/></svg>
  {:else if key === 'notJunk'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m-3.146 2.354a.5.5 0 0 0-.708.708L16.293 17.5l-2.647 2.646a.5.5 0 0 0 .708.708L17 18.207l2.646 2.647a.5.5 0 0 0 .708-.708L17.707 17.5l2.647-2.646a.5.5 0 0 0-.708-.708L17 16.793zM3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75z"/></svg>
  {:else if key === 'move'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75zM17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m-3 5a.5.5 0 0 0 0 1h4.793l-1.647 1.646a.5.5 0 0 0 .708.708l2.5-2.5a.5.5 0 0 0 0-.708l-2.5-2.5a.5.5 0 0 0-.708.708L19.293 17z"/></svg>
  {:else if key === 'reply'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M9.28 6.28a.75.75 0 0 0-1.06-1.06l-5 5a.75.75 0 0 0 0 1.06l5 5a.75.75 0 0 0 1.06-1.06L5.56 11.5h7.69a6.25 6.25 0 0 1 6.25 6.25v.5a.75.75 0 0 0 1.5 0v-.5A7.75 7.75 0 0 0 13.25 10H5.56z"/></svg>
  {:else if key === 'replyAll'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M9.28 5.22a.75.75 0 0 1 0 1.06l-4.47 4.47l4.47 4.47a.75.75 0 1 1-1.06 1.06l-5-5a.75.75 0 0 1 0-1.06l5-5a.75.75 0 0 1 1.06 0m4 0a.75.75 0 0 1 0 1.06L9.56 10h3.69A7.75 7.75 0 0 1 21 17.75v.5a.75.75 0 0 1-1.5 0v-.5a6.25 6.25 0 0 0-6.25-6.25H9.56l3.72 3.72a.75.75 0 1 1-1.06 1.06l-5-5a.75.75 0 0 1 0-1.06l5-5a.75.75 0 0 1 1.06 0"/></svg>
  {:else if key === 'forward'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M14.72 6.28a.75.75 0 0 1 1.06-1.06l5 5a.75.75 0 0 1 0 1.06l-5 5a.75.75 0 1 1-1.06-1.06l3.72-3.72h-7.69a6.25 6.25 0 0 0-6.25 6.25v.5a.75.75 0 0 1-1.5 0v-.5A7.75 7.75 0 0 1 10.75 10h7.69z"/></svg>
  {:else if key === 'replyStub'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 17 4 12 9 7"/><path d="M20 18v-2a4 4 0 00-4-4H4"/></svg>
  {:else if key === 'replyAllStub'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 17 4 12 9 7"/><path d="M20 18v-2a4 4 0 00-4-4H4"/><polyline points="13 17 8 12 13 7"/></svg>
  {:else if key === 'forwardStub'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 17 20 12 15 7"/><path d="M4 18v-2a4 4 0 014-4h12"/></svg>
  {:else if key === 'trashStub'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>
  {:else if key === 'newEvent'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m.25-9A3.25 3.25 0 0 1 21 6.25l.001 5.773a6.5 6.5 0 0 0-1.5-.71L19.5 8.5h-15v9.25c0 .966.784 1.75 1.75 1.75h5.064c.172.534.412 1.038.709 1.501L6.25 21A3.25 3.25 0 0 1 3 17.75V6.25A3.25 3.25 0 0 1 6.25 3zm-.25 11l-.09.008a.5.5 0 0 0-.402.402L17 14.5V17h-2.5l-.09.008a.5.5 0 0 0-.402.402L14 17.5l.008.09a.5.5 0 0 0 .402.402l.09.008H17v2.5l.008.09a.5.5 0 0 0 .402.402l.09.008l.09-.008a.5.5 0 0 0 .402-.402L18 20.5V18h2.5l.09-.008a.5.5 0 0 0 .402-.402L21 17.5l-.008-.09a.5.5 0 0 0-.402-.402L20.5 17H18v-2.5l-.008-.09a.5.5 0 0 0-.402-.402zm.25-9.5H6.25A1.75 1.75 0 0 0 4.5 6.25V7h15v-.75a1.75 1.75 0 0 0-1.75-1.75"/></svg>
  {:else if key === 'newContact'}
    <svg width="16" height="16" viewBox="0 0 24 24"><g fill="currentColor"><path d="M14.5,13.1c-1,1.5-3.7,5.2-4.2,6l-0.7,1v0c-1,1.4-3.1,1.6-4.4,0.4l-3.6-4.2c-0.5-0.6-0.5-1.4-0.1-2l0.4-0.6c0.4-0.7,1-1.5,1.7-2.2c0.9-1,2.3-1.8,3.8-1.8c0.8,0,1.4-0.1,4.3,1.8C12.6,12.2,14.5,13.1,14.5,13.1z M7.4,9.7c1.2,0,2,0.5,2.8,1 M7.3,11.1c-1,0-1.9,0.5-2.7,1.4c-0.8,0.8-1.3,1.7-1.9,2.6c-0.1,0.1,0,0.2,0,0.2l3.4,4l0.1,0.1c0.6,0.5,1.6,0.5,2.1-0.2l4-5.6c-0.2-0.1-0.4-0.2-0.6-0.3c-0.4-0.2-0.8-0.5-1.2-0.8c-0.4-0.3-0.7-0.5-1.1-0.7C8.7,11.4,8.1,11.1,7.3,11.1"/><path d="M7.8,2.4c-1.7,0-3,1.3-3,3c0,1.7,1.3,3,3,3s3-1.3,3-3C10.8,3.8,9.5,2.4,7.8,2.4z M7.8,6.9C7,6.9,6.3,6.3,6.3,5.4S7,3.9,7.8,3.9s1.5,0.7,1.5,1.5S8.7,6.9,7.8,6.9z"/><path d="M17.3,10.6c-3,0-5.5,2.5-5.5,5.5c0,3,2.5,5.5,5.5,5.5c3,0,5.5-2.5,5.5-5.5C22.8,13,20.3,10.6,17.3,10.6z M18,16.7v3.4h-1.3v-3.4h-3.3v-1.2h3.3v-3.4H18v3.4h3.3v1.2H18z"/></g></svg>
  {:else if key === 'newContactList'}
    <svg width="16" height="16" viewBox="0 0 24 24"><g fill="currentColor"><path d="M14.4,13.2c-1,1.5-3.6,5.2-4.1,6l-0.7,1l0,0c-1,1.4-3.1,1.6-4.3,0.4l-3.5-4.2c-0.5-0.6-0.5-1.4-0.1-2l0.4-0.6c0.4-0.7,1-1.5,1.7-2.2c0.9-1,2.2-1.8,3.7-1.8c0.8,0,1.3-0.1,4.2,1.8C12.6,12.2,14.4,13.2,14.4,13.2z M7.5,9.7c1.1,0,2,0.5,2.7,1 M7.4,11.2c-0.9,0-1.8,0.5-2.6,1.4c-0.7,0.8-1.3,1.7-1.9,2.6c-0.1,0.1,0,0.2,0,0.2l3.3,4l0.1,0.1c0.6,0.5,1.6,0.5,2-0.2l3.9-5.6c-0.2-0.1-0.4-0.2-0.6-0.3c-0.4-0.2-0.8-0.5-1.2-0.8s-0.7-0.5-1.1-0.7C8.8,11.5,8.1,11.2,7.4,11.2 M6.9,2.4c1.7,0,3,1.3,3,3s-1.3,3-3,3s-3-1.3-3-3S5.3,2.4,6.9,2.4 M16.9,2.4c1.7,0,3,1.3,3,3s-1.3,3-3,3s-3-1.3-3-3S15.3,2.4,16.9,2.4 M6.9,3.9c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5s1.5-0.7,1.5-1.5S7.8,3.9,6.9,3.9 M16.9,3.9c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5s1.5-0.7,1.5-1.5S17.8,3.9,16.9,3.9"/><path d="M17.1,10.4c-3,0-5.5,2.5-5.5,5.5c0,3,2.5,5.5,5.5,5.5c3,0,5.5-2.5,5.5-5.5C22.6,12.8,20.1,10.4,17.1,10.4z M17.8,16.5v3.4h-1.3v-3.4h-3.3v-1.2h3.3v-3.4h1.3v3.4h3.3v1.2H17.8z"/></g></svg>
  {:else if key === 'edit'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M13.25 4a.75.75 0 0 1 0 1.5h-7A1.75 1.75 0 0 0 4.5 7.25v10.5c0 .966.784 1.75 1.75 1.75h10.5a1.75 1.75 0 0 0 1.75-1.75v-7a.75.75 0 0 1 1.5 0v7A3.25 3.25 0 0 1 16.75 21H6.25A3.25 3.25 0 0 1 3 17.75V7.25A3.25 3.25 0 0 1 6.25 4zm6.47-.78a.75.75 0 1 1 1.06 1.06L10.59 14.47L9 15l.53-1.59z"/></svg>
  {:else if key === 'undo'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M4.5 8.5a.75.75 0 0 1-.75-.75V3.5a.75.75 0 0 1 1.5 0v2.07A9.25 9.25 0 0 1 21.25 12a9.25 9.25 0 0 1-9.25 9.25A9.25 9.25 0 0 1 3.34 15a.75.75 0 1 1 1.32-.72A7.75 7.75 0 0 0 12 19.75a7.75 7.75 0 0 0 0-15.5A7.73 7.73 0 0 0 6.42 7H8.5a.75.75 0 0 1 0 1.5z"/></svg>
  {:else if key === 'headers'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="5" x2="21" y2="5"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="3" y1="13" x2="13" y2="13"/><line x1="3" y1="17" x2="9" y2="17"/></svg>
  {:else if key === 'print'}
    <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M18.25 3A2.75 2.75 0 0 1 21 5.75v4a2.75 2.75 0 0 1-2.75 2.75H18v3.25A3.25 3.25 0 0 1 14.75 19h-5.5A3.25 3.25 0 0 1 6 15.75V12.5h-.25A2.75 2.75 0 0 1 3 9.75v-4A2.75 2.75 0 0 1 5.75 3zM16.5 12.5h-9v3.25c0 .966.784 1.75 1.75 1.75h5.5a1.75 1.75 0 0 0 1.75-1.75zm1.75-8H5.75A1.25 1.25 0 0 0 4.5 5.75v4c0 .69.56 1.25 1.25 1.25h.25v-.25A.75.75 0 0 1 6.75 10h10.5a.75.75 0 0 1 .75.75v.25h.25c.69 0 1.25-.56 1.25-1.25v-4c0-.69-.56-1.25-1.25-1.25"/></svg>
  {:else if key === 'sync'}
    <svg class="sync-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
  {:else if key === 'more'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="19" cy="12" r="1.5"/></svg>
  {/if}
{/snippet}

<div class="command-bar" role="toolbar" bind:this={barEl}>
  {#each commands as c (c.id)}
    {#if c.kind === 'sep'}
      <div class="cmd-separator"></div>
    {:else if c.kind === 'spacer'}
      <div class="more-wrapper">
        <button
          class="cmd-btn cmd-icon-only"
          data-cmd-id="__more"
          tabindex="-1"
          data-tooltip={t('commandBar.moreOptions')}
          data-tooltip-position="bottom"
          aria-label={t('commandBar.moreOptions')}
          disabled={!hasOverflow}
          onclick={(e) => { e.stopPropagation(); showMoreMenu = !showMoreMenu; }}
        >
          {@render icon('more')}
        </button>
        {#if showMoreMenu && hasOverflow}
          <div class="more-menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
            {#each menuItems as c (c.id)}
              {#if c.isMove}
                <div class="more-section">
                  {@render icon(c.iconKey)}
                  <span>{t('commandBar.move')}</span>
                </div>
                {#each moveTargetFolders as folder (folder.id)}
                  <button class="more-item more-subitem" tabindex="-1" onclick={() => { onMove?.(folder.id); showMoreMenu = false; }}>{folder.name}</button>
                {/each}
                {#if moveTargetFolders.length === 0}
                  <div class="more-item" style="color: var(--text-tertiary); cursor: default;">{t('commandBar.noFolders')}</div>
                {/if}
              {:else}
                <button
                  class="more-item"
                  class:cmd-danger={c.danger}
                  class:active={c.active}
                  tabindex="-1"
                  disabled={c.disabled}
                  onclick={() => { c.onClick(); showMoreMenu = false; }}
                >
                  {@render icon(c.iconKey)}
                  <span>{c.label}</span>
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
      <div class="cmd-spacer"></div>
    {:else if c.kind === 'caltoggle'}
      <div class="cal-view-toggle">
        <button class="cal-view-btn" class:active={calendarViewMode === 'day'} onclick={() => onChangeCalendarViewMode?.('day')}>{t('commandBar.day')}</button>
        <button class="cal-view-btn" class:active={calendarViewMode === 'week'} onclick={() => onChangeCalendarViewMode?.('week')}>{t('commandBar.week')}</button>
        <button class="cal-view-btn" class:active={calendarViewMode === 'month'} onclick={() => onChangeCalendarViewMode?.('month')}>{t('commandBar.month')}</button>
      </div>
    {:else if c.kind === 'btn' && c.isMove}
      <div
        class="move-wrapper"
        class:cmd-hidden={overflowedIds.has(c.id)}
        data-cmd-id={c.id}
        data-cmd-group={cmdGroup(c)}
        data-collapsible={isCollapsible(c) ? '1' : null}
      >
        <button class="cmd-btn" tabindex="-1" data-tooltip={c.tooltip} data-tooltip-position={c.tooltipPosition} onclick={(e) => { e.stopPropagation(); showMoveMenu = !showMoveMenu; }}>
          {@render icon(c.iconKey)}
          <span>{c.label}</span>
        </button>
        {#if showMoveMenu}
          <div class="move-menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
            {#each moveTargetFolders as folder (folder.id)}
              <button class="move-option" tabindex="-1" onclick={() => { onMove?.(folder.id); showMoveMenu = false; }}>{folder.name}</button>
            {/each}
            {#if moveTargetFolders.length === 0}
              <div class="move-option" style="color: var(--text-tertiary); cursor: default;">{t('commandBar.noFolders')}</div>
            {/if}
          </div>
        {/if}
      </div>
    {:else if c.kind === 'btn'}
      <button
        class="cmd-btn"
        class:cmd-new-mail={c.primary}
        class:cmd-danger={c.danger}
        class:cmd-icon-only={c.iconOnly}
        class:cmd-syncing={c.isSync && syncing}
        class:active={c.active}
        class:cmd-hidden={overflowedIds.has(c.id)}
        data-cmd-id={c.id}
        data-cmd-group={cmdGroup(c)}
        data-collapsible={isCollapsible(c) ? '1' : null}
        tabindex="-1"
        data-tooltip={c.tooltip}
        data-tooltip-position={c.tooltipPosition}
        aria-label={c.ariaLabel}
        disabled={c.disabled}
        onclick={() => c.onClick()}
      >
        {@render icon(c.iconKey)}
        {#if c.label && !c.iconOnly}<span>{c.label}</span>{/if}
      </button>
    {/if}
  {/each}
</div>

<style>
  .command-bar {
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 4px 12px;
    margin-right: 4px;
    background: var(--bg-secondary);
    flex-shrink: 0;
    height: 38px;
    user-select: none;
    border-radius: 4px;
    min-width: 0;
  }

  .cmd-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 8px;
    border-radius: 4px;
    font-size: 12px;
    color: var(--text-primary);
    transition: background 0.1s;
    white-space: nowrap;
    height: 28px;
    outline: none;
    flex-shrink: 0;
  }

  .cmd-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .cmd-btn:active:not(:disabled) {
    background: var(--border-light);
  }

  .cmd-btn:disabled {
    color: color-mix(in srgb, var(--text-primary) 40%, transparent);
    cursor: default;
  }

  .cmd-new-mail {
    background: var(--accent);
    color: var(--text-on-accent);
    font-weight: 600;
    padding: 5px 12px;
    border-radius: 5px;
  }

  .cmd-new-mail:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .cmd-new-mail:active:not(:disabled) {
    background: var(--accent-active);
  }

  .cmd-danger:hover:not(:disabled) {
    color: var(--danger);
  }

  .cal-view-toggle {
    display: flex;
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
    margin-left: 8px;
    flex-shrink: 0;
  }

  .cal-view-btn {
    padding: 4px 12px;
    font-size: 12px;
    color: var(--text-secondary);
    border-right: 1px solid var(--border);
    background: var(--bg-primary);
    height: 28px;
    outline: none;
  }

  .cal-view-btn:last-child {
    border-right: none;
  }

  .cal-view-btn:hover,
  .cal-view-btn:focus {
    background: var(--bg-hover);
  }

  .cal-view-btn.active {
    color: var(--text-primary);
    font-weight: 600;
  }

  .cmd-separator {
    width: 1px;
    height: 18px;
    background: var(--border-light);
    margin: 0 4px;
    flex-shrink: 0;
  }

  .cmd-spacer {
    flex: 1;
    min-width: 0;
  }

  .cmd-syncing .sync-icon {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .cmd-hidden {
    display: none !important;
  }

  /* ── Move dropdown ── */
  .move-wrapper {
    position: relative;
    flex-shrink: 0;
  }

  .move-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    box-shadow: var(--shadow-md, 0 8px 32px rgba(0, 0, 0, 0.18));
    z-index: 100;
    min-width: 160px;
    max-height: 300px;
    overflow-y: auto;
  }

  .move-option {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 7px 10px;
    font-size: 12.5px;
    color: var(--text-primary);
    border-radius: 4px;
    text-align: left;
    transition: background 0.1s;
  }

  .move-option:hover {
    background: var(--bg-hover);
  }

  /* ── More dropdown ── */
  .more-wrapper {
    position: relative;
    flex-shrink: 0;
  }

  .more-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    box-shadow: var(--shadow-md, 0 8px 32px rgba(0, 0, 0, 0.18));
    z-index: 100;
    min-width: 200px;
    max-height: 380px;
    overflow-y: auto;
    padding: 4px;
  }

  .more-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    font-size: 12.5px;
    color: var(--text-primary);
    border-radius: 4px;
    text-align: left;
    transition: background 0.1s;
  }

  .more-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .more-item:disabled {
    color: color-mix(in srgb, var(--text-primary) 40%, transparent);
    cursor: default;
  }

  .more-item.cmd-danger:hover:not(:disabled) {
    color: var(--danger);
  }

  .more-subitem {
    padding-left: 32px;
  }

  .more-section {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px 4px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-primary);
  }
</style>
