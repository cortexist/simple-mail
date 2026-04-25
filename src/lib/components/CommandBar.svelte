<script lang="ts">
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

  let moveTargetFolders = $derived(
    folders.filter((f) => f.id !== currentFolder)
  );

  let isInJunk = $derived(currentFolder === 'junk');
</script>

<svelte:window onclick={() => { showMoveMenu = false; }} />

<div class="command-bar" role="toolbar">
  {#if activeView === 'mail'}
    <!-- ── Mail Commands ── -->
    <button class="cmd-btn cmd-new-mail" tabindex="-1" data-tooltip={t('commandBar.newMail')} data-tooltip-position="bottom" onclick={() => onCompose('new')}>
      <svg width="16" height="16" viewBox="0 0 24 24">
        <g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5">
            <path d="M5.076 17C4.089 4.545 12.912 1.012 19.973 2.224c.286 4.128-1.734 5.673-5.58 6.387c.742.776 2.055 1.753 1.913 2.974c-.1.868-.69 1.295-1.87 2.147C11.85 15.6 8.854 16.78 5.076 17"/>
            <path d="M4 22c0-6.5 3.848-9.818 6.5-12"/>
        </g>
      </svg>
      <span>{t('compose.newMail')}</span>
    </button>

    <div class="cmd-separator"></div>

    {#if !email && multiSelectCount > 0}
      <!-- ── Bulk actions: mails are checked but none is active ── -->
      {#if multiSelectHasUnread}
      <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.markRead')} data-tooltip-position="bottom" onclick={() => onMarkRead?.()}>
        <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M13.19 2.34a2.25 2.25 0 0 0-2.38 0L3.06 7.172A2.25 2.25 0 0 0 2 9.083v7.667A3.25 3.25 0 0 0 5.25 20h13.5A3.25 3.25 0 0 0 22 16.75V9.082c0-.776-.4-1.498-1.06-1.909zm-1.587 1.272a.75.75 0 0 1 .794 0l7.242 4.517L12 12.15L4.361 8.13zM3.5 9.371l8.15 4.29a.75.75 0 0 0 .7 0l8.15-4.29v7.379a1.75 1.75 0 0 1-1.75 1.75H5.25a1.75 1.75 0 0 1-1.75-1.75z"/></svg>
        <span>{t('commandBar.read')}</span>
      </button>
      {:else}
      <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.markUnread')} data-tooltip-position="bottom" onclick={() => onMarkUnread?.()}>
        <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M5.25 4h13.5a3.25 3.25 0 0 1 3.245 3.066L22 7.25v9.5a3.25 3.25 0 0 1-3.066 3.245L18.75 20H5.25a3.25 3.25 0 0 1-3.245-3.066L2 16.75v-9.5a3.25 3.25 0 0 1 3.066-3.245zh13.5zM20.5 9.373l-8.15 4.29a.75.75 0 0 1-.603.043l-.096-.042L3.5 9.374v7.376a1.75 1.75 0 0 0 1.606 1.744l.144.006h13.5a1.75 1.75 0 0 0 1.744-1.607l.006-.143zM18.75 5.5H5.25a1.75 1.75 0 0 0-1.744 1.606L3.5 7.25v.429l8.5 4.474l8.5-4.475V7.25a1.75 1.75 0 0 0-1.607-1.744z"/></svg>
        <span>{t('commandBar.unread')}</span>
      </button>
      {/if}

      <div class="cmd-separator"></div>

      <button class="cmd-btn cmd-danger" tabindex="-1" data-tooltip={t('commandBar.delete')} data-tooltip-position="bottom" onclick={() => onDelete?.()}>
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M10 5h4a2 2 0 1 0-4 0M8.5 5a3.5 3.5 0 1 1 7 0h5.75a.75.75 0 0 1 0 1.5h-1.32l-1.17 12.111A3.75 3.75 0 0 1 15.026 22H8.974a3.75 3.75 0 0 1-3.733-3.389L4.07 6.5H2.75a.75.75 0 0 1 0-1.5zm2 4.75a.75.75 0 0 0-1.5 0v7.5a.75.75 0 0 0 1.5 0zM14.25 9a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-1.5 0v-7.5a.75.75 0 0 1 .75-.75m-7.516 9.467a2.25 2.25 0 0 0 2.24 2.033h6.052a2.25 2.25 0 0 0 2.24-2.033L18.424 6.5H5.576z"/>
        </svg>
        <span>{t('common.delete')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.archiveShortcut')} data-tooltip-position="bottom" onclick={() => onArchive?.()}>
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M10.25 11a.75.75 0 0 0 0 1.5h3.5a.75.75 0 0 0 0-1.5zM3 5.25A2.25 2.25 0 0 1 5.25 3h13.5A2.25 2.25 0 0 1 21 5.25v1.5c0 .78-.397 1.467-1 1.871v8.629A3.75 3.75 0 0 1 16.25 21h-8.5A3.75 3.75 0 0 1 4 17.25V8.621A2.25 2.25 0 0 1 3 6.75zM5.5 9v8.25a2.25 2.25 0 0 0 2.25 2.25h8.5a2.25 2.25 0 0 0 2.25-2.25V9zm-.25-4.5a.75.75 0 0 0-.75.75v1.5c0 .414.336.75.75.75h13.5a.75.75 0 0 0 .75-.75v-1.5a.75.75 0 0 0-.75-.75z"/>
        </svg>
        <span>{t('commandBar.archive')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" data-tooltip={isInJunk ? t('commandBar.notJunkTooltip') : t('commandBar.junkTooltip')} data-tooltip-position="bottom" onclick={() => onJunk?.()}>
        {#if isInJunk}
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m-3.146 2.354a.5.5 0 0 0-.708.708L16.293 17.5l-2.647 2.646a.5.5 0 0 0 .708.708L17 18.207l2.646 2.647a.5.5 0 0 0 .708-.708L17.707 17.5l2.647-2.646a.5.5 0 0 0-.708-.708L17 16.793zM3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75z"/>
          </svg>
          <span>{t('commandBar.notJunk')}</span>
        {:else}
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75zM23 17.5a5.5 5.5 0 1 1-11 0a5.5 5.5 0 0 1 11 0m-9.5 0c0 .834.255 1.608.691 2.248l5.557-5.557A4 4 0 0 0 13.5 17.5m4 4a4 4 0 0 0 3.309-6.248l-5.557 5.557a4 4 0 0 0 2.248.691"/>
          </svg>
          <span>{t('commandBar.junk')}</span>
        {/if}
      </button>
      <div class="move-wrapper">
        <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.moveTo')} data-tooltip-position="bottom" onclick={(e) => { e.stopPropagation(); showMoveMenu = !showMoveMenu; }}>
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75zM17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m-3 5a.5.5 0 0 0 0 1h4.793l-1.647 1.646a.5.5 0 0 0 .708.708l2.5-2.5a.5.5 0 0 0 0-.708l-2.5-2.5a.5.5 0 0 0-.708.708L19.293 17z"/>
          </svg>
          <span>{t('commandBar.move')}</span>
        </button>
        {#if showMoveMenu}
          <div class="move-menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
            {#each moveTargetFolders as folder (folder.id)}
              <button class="move-option" tabindex="-1" onclick={() => { onMove?.(folder.id); showMoveMenu = false; }}>
                {folder.name}
              </button>
            {/each}
            {#if moveTargetFolders.length === 0}
              <div class="move-option" style="color: var(--text-tertiary); cursor: default;">{t('commandBar.noFolders')}</div>
            {/if}
          </div>
        {/if}
      </div>

      <!-- TODO: Implement Snooze — temporarily hide email and resurface at a chosen date/time -->

    {:else if email}
      <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.reply')} data-tooltip-position="bottom" onclick={() => onCompose('reply')}>
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M9.28 6.28a.75.75 0 0 0-1.06-1.06l-5 5a.75.75 0 0 0 0 1.06l5 5a.75.75 0 0 0 1.06-1.06L5.56 11.5h7.69a6.25 6.25 0 0 1 6.25 6.25v.5a.75.75 0 0 0 1.5 0v-.5A7.75 7.75 0 0 0 13.25 10H5.56z"/>
        </svg>
        <span>{t('compose.reply')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.replyAll')} data-tooltip-position="bottom" onclick={() => onCompose('replyAll')}>
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M9.28 5.22a.75.75 0 0 1 0 1.06l-4.47 4.47l4.47 4.47a.75.75 0 1 1-1.06 1.06l-5-5a.75.75 0 0 1 0-1.06l5-5a.75.75 0 0 1 1.06 0m4 0a.75.75 0 0 1 0 1.06L9.56 10h3.69A7.75 7.75 0 0 1 21 17.75v.5a.75.75 0 0 1-1.5 0v-.5a6.25 6.25 0 0 0-6.25-6.25H9.56l3.72 3.72a.75.75 0 1 1-1.06 1.06l-5-5a.75.75 0 0 1 0-1.06l5-5a.75.75 0 0 1 1.06 0"/>
        </svg>
        <span>{t('compose.replyAll')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.forward')} data-tooltip-position="bottom" onclick={() => onCompose('forward')}>
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M14.72 6.28a.75.75 0 0 1 1.06-1.06l5 5a.75.75 0 0 1 0 1.06l-5 5a.75.75 0 1 1-1.06-1.06l3.72-3.72h-7.69a6.25 6.25 0 0 0-6.25 6.25v.5a.75.75 0 0 1-1.5 0v-.5A7.75 7.75 0 0 1 10.75 10h7.69z"/>
        </svg>
        <span>{t('compose.forward')}</span>
      </button>
      {#if email.isRead}
        <button
          class="cmd-btn"
          tabindex="-1"
          data-tooltip={t('commandBar.markUnread')}
          data-tooltip-position="bottom"
          onclick={() => onMarkUnread?.()}
          disabled={email.folder === 'drafts'}
        >
        <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M5.25 4h13.5a3.25 3.25 0 0 1 3.245 3.066L22 7.25v9.5a3.25 3.25 0 0 1-3.066 3.245L18.75 20H5.25a3.25 3.25 0 0 1-3.245-3.066L2 16.75v-9.5a3.25 3.25 0 0 1 3.066-3.245zh13.5zM20.5 9.373l-8.15 4.29a.75.75 0 0 1-.603.043l-.096-.042L3.5 9.374v7.376a1.75 1.75 0 0 0 1.606 1.744l.144.006h13.5a1.75 1.75 0 0 0 1.744-1.607l.006-.143zM18.75 5.5H5.25a1.75 1.75 0 0 0-1.744 1.606L3.5 7.25v.429l8.5 4.474l8.5-4.475V7.25a1.75 1.75 0 0 0-1.607-1.744z"/></svg>
        <span>{t('commandBar.unread')}</span>
        </button>
      {:else}
        <button
          class="cmd-btn"
          tabindex="-1"
          data-tooltip={t('commandBar.markRead')}
          data-tooltip-position="bottom"
          onclick={() => onMarkRead?.()}
          disabled={email.folder === 'drafts'}
        >
          <svg width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M13.19 2.34a2.25 2.25 0 0 0-2.38 0L3.06 7.172A2.25 2.25 0 0 0 2 9.083v7.667A3.25 3.25 0 0 0 5.25 20h13.5A3.25 3.25 0 0 0 22 16.75V9.082c0-.776-.4-1.498-1.06-1.909zm-1.587 1.272a.75.75 0 0 1 .794 0l7.242 4.517L12 12.15L4.361 8.13zM3.5 9.371l8.15 4.29a.75.75 0 0 0 .7 0l8.15-4.29v7.379a1.75 1.75 0 0 1-1.75 1.75H5.25a1.75 1.75 0 0 1-1.75-1.75z"/></svg>
          <span>{t('commandBar.read')}</span>
        </button>
      {/if}

      <div class="cmd-separator"></div>

      <button class="cmd-btn cmd-danger" tabindex="-1" data-tooltip={t('commandBar.delete')} data-tooltip-position="bottom" onclick={() => onDelete?.()}>
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M10 5h4a2 2 0 1 0-4 0M8.5 5a3.5 3.5 0 1 1 7 0h5.75a.75.75 0 0 1 0 1.5h-1.32l-1.17 12.111A3.75 3.75 0 0 1 15.026 22H8.974a3.75 3.75 0 0 1-3.733-3.389L4.07 6.5H2.75a.75.75 0 0 1 0-1.5zm2 4.75a.75.75 0 0 0-1.5 0v7.5a.75.75 0 0 0 1.5 0zM14.25 9a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-1.5 0v-7.5a.75.75 0 0 1 .75-.75m-7.516 9.467a2.25 2.25 0 0 0 2.24 2.033h6.052a2.25 2.25 0 0 0 2.24-2.033L18.424 6.5H5.576z"/>
        </svg>
        <span>{t('common.delete')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.archiveShortcut')} data-tooltip-position="bottom" onclick={() => onArchive?.()}>
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M10.25 11a.75.75 0 0 0 0 1.5h3.5a.75.75 0 0 0 0-1.5zM3 5.25A2.25 2.25 0 0 1 5.25 3h13.5A2.25 2.25 0 0 1 21 5.25v1.5c0 .78-.397 1.467-1 1.871v8.629A3.75 3.75 0 0 1 16.25 21h-8.5A3.75 3.75 0 0 1 4 17.25V8.621A2.25 2.25 0 0 1 3 6.75zM5.5 9v8.25a2.25 2.25 0 0 0 2.25 2.25h8.5a2.25 2.25 0 0 0 2.25-2.25V9zm-.25-4.5a.75.75 0 0 0-.75.75v1.5c0 .414.336.75.75.75h13.5a.75.75 0 0 0 .75-.75v-1.5a.75.75 0 0 0-.75-.75z"/>
        </svg>
        <span>{t('commandBar.archive')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" data-tooltip={isInJunk ? t('commandBar.notJunkTooltip') : t('commandBar.junkTooltip')} data-tooltip-position="bottom" onclick={() => onJunk?.()}>
        {#if isInJunk}
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m-3.146 2.354a.5.5 0 0 0-.708.708L16.293 17.5l-2.647 2.646a.5.5 0 0 0 .708.708L17 18.207l2.646 2.647a.5.5 0 0 0 .708-.708L17.707 17.5l2.647-2.646a.5.5 0 0 0-.708-.708L17 16.793zM3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75z"/>
          </svg>
          <span>{t('commandBar.notJunk')}</span>
        {:else}
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75zM23 17.5a5.5 5.5 0 1 1-11 0a5.5 5.5 0 0 1 11 0m-9.5 0c0 .834.255 1.608.691 2.248l5.557-5.557A4 4 0 0 0 13.5 17.5m4 4a4 4 0 0 0 3.309-6.248l-5.557 5.557a4 4 0 0 0 2.248.691"/>
          </svg>
          <span>{t('commandBar.junk')}</span>
        {/if}
      </button>
      <div class="move-wrapper">
        <button class="cmd-btn" tabindex="-1" data-tooltip={t('commandBar.moveTo')} data-tooltip-position="bottom" onclick={(e) => { e.stopPropagation(); showMoveMenu = !showMoveMenu; }}>
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75zM17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m-3 5a.5.5 0 0 0 0 1h4.793l-1.647 1.646a.5.5 0 0 0 .708.708l2.5-2.5a.5.5 0 0 0 0-.708l-2.5-2.5a.5.5 0 0 0-.708.708L19.293 17z"/>
          </svg>
          <span>{t('commandBar.move')}</span>
        </button>
        {#if showMoveMenu}
          <div class="move-menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
            {#each moveTargetFolders as folder (folder.id)}
              <button class="move-option" tabindex="-1" onclick={() => { onMove?.(folder.id); showMoveMenu = false; }}>
                {folder.name}
              </button>
            {/each}
            {#if moveTargetFolders.length === 0}
              <div class="move-option" style="color: var(--text-tertiary); cursor: default;">{t('commandBar.noFolders')}</div>
            {/if}
          </div>
        {/if}
      </div>

    {:else}
      <button class="cmd-btn" tabindex="-1" disabled data-tooltip={t('compose.reply')} data-tooltip-position="bottom">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="9 17 4 12 9 7" />
          <path d="M20 18v-2a4 4 0 00-4-4H4" />
        </svg>
        <span>{t('compose.reply')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" disabled data-tooltip={t('compose.replyAll')} data-tooltip-position="bottom">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="9 17 4 12 9 7" />
          <path d="M20 18v-2a4 4 0 00-4-4H4" />
          <polyline points="13 17 8 12 13 7" />
        </svg>
        <span>{t('compose.replyAll')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" disabled data-tooltip={t('compose.forward')} data-tooltip-position="bottom">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="15 17 20 12 15 7" />
          <path d="M4 18v-2a4 4 0 014-4h12" />
        </svg>
        <span>{t('compose.forward')}</span>
      </button>
      <button class="cmd-btn" tabindex="-1" disabled data-tooltip={t('common.delete')} data-tooltip-position="bottom">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6" />
          <path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2" />
        </svg>
        <span>{t('common.delete')}</span>
      </button>
    {/if}

  {:else if activeView === 'calendar'}
    <!-- ── Calendar Commands ── -->
    <button class="cmd-btn cmd-new-mail" tabindex="-1" data-tooltip={t('commandBar.newEventShortcut')} data-tooltip-position="bottom" onclick={() => onNewEvent?.()}>
      <svg width="16" height="16" viewBox="0 0 24 24">
        <path fill="currentColor" d="M17.5 12a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11m.25-9A3.25 3.25 0 0 1 21 6.25l.001 5.773a6.5 6.5 0 0 0-1.5-.71L19.5 8.5h-15v9.25c0 .966.784 1.75 1.75 1.75h5.064c.172.534.412 1.038.709 1.501L6.25 21A3.25 3.25 0 0 1 3 17.75V6.25A3.25 3.25 0 0 1 6.25 3zm-.25 11l-.09.008a.5.5 0 0 0-.402.402L17 14.5V17h-2.5l-.09.008a.5.5 0 0 0-.402.402L14 17.5l.008.09a.5.5 0 0 0 .402.402l.09.008H17v2.5l.008.09a.5.5 0 0 0 .402.402l.09.008l.09-.008a.5.5 0 0 0 .402-.402L18 20.5V18h2.5l.09-.008a.5.5 0 0 0 .402-.402L21 17.5l-.008-.09a.5.5 0 0 0-.402-.402L20.5 17H18v-2.5l-.008-.09a.5.5 0 0 0-.402-.402zm.25-9.5H6.25A1.75 1.75 0 0 0 4.5 6.25V7h15v-.75a1.75 1.75 0 0 0-1.75-1.75"/>
      </svg>
      <span>{t('commandBar.newEvent')}</span>
    </button>

    <div class="cal-view-toggle">
      <button class="cal-view-btn" class:active={calendarViewMode === 'day'} onclick={() => onChangeCalendarViewMode?.('day')}>{t('commandBar.day')}</button>
      <button class="cal-view-btn" class:active={calendarViewMode === 'week'} onclick={() => onChangeCalendarViewMode?.('week')}>{t('commandBar.week')}</button>
      <button class="cal-view-btn" class:active={calendarViewMode === 'month'} onclick={() => onChangeCalendarViewMode?.('month')}>{t('commandBar.month')}</button>
    </div>

  {:else if activeView === 'contacts'}
    <!-- ── Contacts Commands ── -->
    <button class="cmd-btn cmd-new-mail" tabindex="-1" data-tooltip={contactListMode ? t('commandBar.newContactListShortcut') : t('commandBar.newContactShortcut')} data-tooltip-position="bottom" onclick={() => onNewContact?.()}>
      {#if contactListMode}
        <svg width="16" height="16"viewBox="0 0 24 24">
        <g fill="currentColor">
            <path d="M14.4,13.2c-1,1.5-3.6,5.2-4.1,6l-0.7,1l0,0c-1,1.4-3.1,1.6-4.3,0.4l-3.5-4.2c-0.5-0.6-0.5-1.4-0.1-2l0.4-0.6
            c0.4-0.7,1-1.5,1.7-2.2c0.9-1,2.2-1.8,3.7-1.8c0.8,0,1.3-0.1,4.2,1.8C12.6,12.2,14.4,13.2,14.4,13.2z M7.5,9.7c1.1,0,2,0.5,2.7,1
            M7.4,11.2c-0.9,0-1.8,0.5-2.6,1.4c-0.7,0.8-1.3,1.7-1.9,2.6c-0.1,0.1,0,0.2,0,0.2l3.3,4l0.1,0.1c0.6,0.5,1.6,0.5,2-0.2l3.9-5.6
            c-0.2-0.1-0.4-0.2-0.6-0.3c-0.4-0.2-0.8-0.5-1.2-0.8s-0.7-0.5-1.1-0.7C8.8,11.5,8.1,11.2,7.4,11.2 M6.9,2.4c1.7,0,3,1.3,3,3
            s-1.3,3-3,3s-3-1.3-3-3S5.3,2.4,6.9,2.4 M16.9,2.4c1.7,0,3,1.3,3,3s-1.3,3-3,3s-3-1.3-3-3S15.3,2.4,16.9,2.4 M6.9,3.9
            c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5s1.5-0.7,1.5-1.5S7.8,3.9,6.9,3.9 M16.9,3.9c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5
            s1.5-0.7,1.5-1.5S17.8,3.9,16.9,3.9"/>
            <path d="M17.1,10.4c-3,0-5.5,2.5-5.5,5.5c0,3,2.5,5.5,5.5,5.5c3,0,5.5-2.5,5.5-5.5C22.6,12.8,20.1,10.4,17.1,10.4z M17.8,16.5v3.4
            h-1.3v-3.4h-3.3v-1.2h3.3v-3.4h1.3v3.4h3.3v1.2H17.8z"/>
        </g>
        </svg>    
      {:else}
        <svg width="16" height="16" viewBox="0 0 24 24">
          <g fill="currentColor">
	    <path d="M14.5,13.1c-1,1.5-3.7,5.2-4.2,6l-0.7,1v0c-1,1.4-3.1,1.6-4.4,0.4l-3.6-4.2c-0.5-0.6-0.5-1.4-0.1-2l0.4-0.6
		c0.4-0.7,1-1.5,1.7-2.2c0.9-1,2.3-1.8,3.8-1.8c0.8,0,1.4-0.1,4.3,1.8C12.6,12.2,14.5,13.1,14.5,13.1z M7.4,9.7c1.2,0,2,0.5,2.8,1
		 M7.3,11.1c-1,0-1.9,0.5-2.7,1.4c-0.8,0.8-1.3,1.7-1.9,2.6c-0.1,0.1,0,0.2,0,0.2l3.4,4l0.1,0.1c0.6,0.5,1.6,0.5,2.1-0.2l4-5.6
		c-0.2-0.1-0.4-0.2-0.6-0.3c-0.4-0.2-0.8-0.5-1.2-0.8c-0.4-0.3-0.7-0.5-1.1-0.7C8.7,11.4,8.1,11.1,7.3,11.1"/>
	    <path d="M7.8,2.4c-1.7,0-3,1.3-3,3c0,1.7,1.3,3,3,3s3-1.3,3-3C10.8,3.8,9.5,2.4,7.8,2.4z M7.8,6.9C7,6.9,6.3,6.3,6.3,5.4
		S7,3.9,7.8,3.9s1.5,0.7,1.5,1.5S8.7,6.9,7.8,6.9z"/>
	    <path d="M17.3,10.6c-3,0-5.5,2.5-5.5,5.5c0,3,2.5,5.5,5.5,5.5c3,0,5.5-2.5,5.5-5.5C22.8,13,20.3,10.6,17.3,10.6z M18,16.7v3.4h-1.3
		v-3.4h-3.3v-1.2h3.3v-3.4H18v3.4h3.3v1.2H18z"/>
          </g>
        </svg>
      {/if}
      <span>{contactListMode ? t('commandBar.newContactList') : t('commandBar.newContact')}</span>
    </button>

    <div class="cmd-separator"></div>

    <button class="cmd-btn" tabindex="-1" data-tooltip={contactReadOnly ? t('commandBar.readOnly') : t('commandBar.editContact')} data-tooltip-position="bottom" onclick={() => onEditContact?.()} disabled={contactReadOnly}>
      <svg width="16" height="16" viewBox="0 0 24 24">
        <path fill="currentColor" d="M13.25 4a.75.75 0 0 1 0 1.5h-7A1.75 1.75 0 0 0 4.5 7.25v10.5c0 .966.784 1.75 1.75 1.75h10.5a1.75 1.75 0 0 0 1.75-1.75v-7a.75.75 0 0 1 1.5 0v7A3.25 3.25 0 0 1 16.75 21H6.25A3.25 3.25 0 0 1 3 17.75V7.25A3.25 3.25 0 0 1 6.25 4zm6.47-.78a.75.75 0 1 1 1.06 1.06L10.59 14.47L9 15l.53-1.59z"/>
      </svg>
      <span>{t('common.edit')}</span>
    </button>
    <button class="cmd-btn cmd-danger" tabindex="-1" data-tooltip={contactReadOnly ? t('commandBar.readOnly') : t('commandBar.delete')} data-tooltip-position="bottom" onclick={() => onDeleteContact?.()} disabled={contactReadOnly}>
      <svg width="16" height="16" viewBox="0 0 24 24">
        <path fill="currentColor" d="M10 5h4a2 2 0 1 0-4 0M8.5 5a3.5 3.5 0 1 1 7 0h5.75a.75.75 0 0 1 0 1.5h-1.32l-1.17 12.111A3.75 3.75 0 0 1 15.026 22H8.974a3.75 3.75 0 0 1-3.733-3.389L4.07 6.5H2.75a.75.75 0 0 1 0-1.5zm2 4.75a.75.75 0 0 0-1.5 0v7.5a.75.75 0 0 0 1.5 0zM14.25 9a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-1.5 0v-7.5a.75.75 0 0 1 .75-.75m-7.516 9.467a2.25 2.25 0 0 0 2.24 2.033h6.052a2.25 2.25 0 0 0 2.24-2.033L18.424 6.5H5.576z"/>
      </svg>
      <span>{t('common.delete')}</span>
    </button>
  {/if}

  <div class="cmd-spacer"></div>

  {#if canUndo}
    <button
      class="cmd-btn"
      tabindex="-1"
      data-tooltip={t('commandBar.undoShortcut')}
      data-tooltip-position="bottom"
      aria-label={t('common.undo')}
      onclick={() => onUndo?.()}
    >
      <svg width="16" height="16" viewBox="0 0 24 24">
        <path fill="currentColor" d="M4.5 8.5a.75.75 0 0 1-.75-.75V3.5a.75.75 0 0 1 1.5 0v2.07A9.25 9.25 0 0 1 21.25 12a9.25 9.25 0 0 1-9.25 9.25A9.25 9.25 0 0 1 3.34 15a.75.75 0 1 1 1.32-.72A7.75 7.75 0 0 0 12 19.75a7.75 7.75 0 0 0 0-15.5A7.73 7.73 0 0 0 6.42 7H8.5a.75.75 0 0 1 0 1.5z"/>
      </svg>
      <span>{t('common.undo')}</span>
    </button>
  {/if}

  {#if activeView === 'mail'}
    <button
      class="cmd-btn"
      class:active={showAllHeaders}
      tabindex="-1"
      data-tooltip={showAllHeaders ? t('commandBar.hideHeaders') : t('commandBar.showHeaders')}
      data-tooltip-position="bottom"
      aria-label={showAllHeaders ? t('commandBar.hideHeaders') : t('commandBar.showHeaders')}
      onclick={() => onToggleHeaders?.()}
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <line x1="3" y1="5" x2="21" y2="5"/><line x1="3" y1="9" x2="21" y2="9"/>
        <line x1="3" y1="13" x2="13" y2="13"/><line x1="3" y1="17" x2="9" y2="17"/>
      </svg>
      <span>{t('commandBar.headers')}</span>
    </button>
    <button
      class="cmd-btn"
      tabindex="-1"
      data-tooltip={t('commandBar.printShortcut')}
      data-tooltip-position="bottom"
      aria-label={t('commandBar.print')}
      onclick={() => onPrint?.()}
    >
      <svg width="16" height="16" viewBox="0 0 24 24">
        <path fill="currentColor" d="M18.25 3A2.75 2.75 0 0 1 21 5.75v4a2.75 2.75 0 0 1-2.75 2.75H18v3.25A3.25 3.25 0 0 1 14.75 19h-5.5A3.25 3.25 0 0 1 6 15.75V12.5h-.25A2.75 2.75 0 0 1 3 9.75v-4A2.75 2.75 0 0 1 5.75 3zM16.5 12.5h-9v3.25c0 .966.784 1.75 1.75 1.75h5.5a1.75 1.75 0 0 0 1.75-1.75zm1.75-8H5.75A1.25 1.25 0 0 0 4.5 5.75v4c0 .69.56 1.25 1.25 1.25h.25v-.25A.75.75 0 0 1 6.75 10h10.5a.75.75 0 0 1 .75.75v.25h.25c.69 0 1.25-.56 1.25-1.25v-4c0-.69-.56-1.25-1.25-1.25"/>
      </svg>
      <span>{t('commandBar.print')}</span>
    </button>
  {/if}
  {#if activeView === 'mail' || (activeView === 'calendar' && hasDav) || (activeView === 'contacts' && hasDav)}
    <button
      class="cmd-btn"
      tabindex="-1"
      class:cmd-syncing={syncing}
      data-tooltip={syncing ? t('commandBar.syncing') : t('commandBar.sync')}
      data-tooltip-position="bottom"
      aria-label={syncing ? t('commandBar.syncing') : t('commandBar.syncLabel')}
      disabled={syncing}
      onclick={() => onSync?.()}
    >
      <svg class="sync-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="23 4 23 10 17 10" />
        <polyline points="1 20 1 14 7 14" />
        <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
      </svg>
      <span>{syncing ? t('commandBar.syncing') : t('commandBar.syncLabel')}</span>
    </button>
  {/if}

  <button class="cmd-btn cmd-icon-only" tabindex="-1" data-tooltip={t('commandBar.moreOptions')} data-tooltip-position="bottom-end" aria-label={t('commandBar.moreOptions')}>
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
      <circle cx="5" cy="12" r="1.5" />
      <circle cx="12" cy="12" r="1.5" />
      <circle cx="19" cy="12" r="1.5" />
    </svg>
  </button>
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
  }

  .cmd-syncing .sync-icon {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* ── Move dropdown ── */
  .move-wrapper {
    position: relative;
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
</style>
