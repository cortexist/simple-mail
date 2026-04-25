<script lang="ts">
  import type { Folder } from '$lib/types';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    folders: Folder[];
    activeFolder: string;
    onSelectFolder: (id: string) => void;
    unreadCounts: Record<string, number>;
    onCreateFolder?: () => void;
    onRenameFolder?: (id: string) => void;
    onDeleteFolder?: (id: string) => void;
    onToggleFolderFavorite?: (id: string) => void;
    onEmptyFolder?: (id: string) => void;
    folderEmailCounts?: Record<string, number>;
    active?: boolean;
  }

  let {
    folders,
    activeFolder: selectedFolder,
    onSelectFolder,
    unreadCounts,
    onCreateFolder,
    onRenameFolder,
    onDeleteFolder,
    onToggleFolderFavorite,
    onEmptyFolder,
    folderEmailCounts = {},
    active = false,
  }: Props = $props();

  let sortedFolders = $derived([
    ...folders.filter(f => f.isFavorite),
    ...folders.filter(f => !f.isFavorite),
  ]);

  type MenuState =
    | { kind: 'root-folders'; x: number; y: number }
    | { kind: 'folder'; folderId: string; x: number; y: number }
    | null;

  let menu = $state<MenuState>(null);
  let menuFolderId = $derived(menu && menu.kind === 'folder' ? menu.folderId : null);
  let menuFolder = $derived(menuFolderId ? folders.find((f) => f.id === menuFolderId) ?? null : null);

  function openRootMenu(ev: MouseEvent) {
    ev.stopPropagation();
    const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { kind: 'root-folders', x: rect.left, y: rect.bottom + 4 };
  }

  function openFolderMenu(ev: MouseEvent | KeyboardEvent, folderId: string) {
    ev.stopPropagation();
    const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { kind: 'folder', folderId, x: rect.left, y: rect.bottom + 4 };
  }

  function closeMenu() {
    menu = null;
  }

  function doCreateFolder() {
    onCreateFolder?.();
    closeMenu();
  }

  function doRenameFolder(folderId: string) {
    onRenameFolder?.(folderId);
    closeMenu();
  }

  function doDeleteFolder(folderId: string) {
    onDeleteFolder?.(folderId);
    closeMenu();
  }

  function doEmptyFolder(folderId: string) {
    onEmptyFolder?.(folderId);
    closeMenu();
  }

  function doToggleFavorite(folderId: string) {
    onToggleFolderFavorite?.(folderId);
    closeMenu();
  }

  const SYSTEM_FOLDER_KEYS: Record<string, string> = {
    inbox: 'folders.inbox',
    sent: 'folders.sent',
    drafts: 'folders.drafts',
    deleted: 'folders.deleted',
    junk: 'folders.junk',
    archive: 'folders.archive',
  };

  function folderDisplayName(folder: Folder): string {
    const key = SYSTEM_FOLDER_KEYS[folder.id];
    return key ? t(key) : folder.name;
  }

  $effect(() => {
    if (!menu) return;
    const onWindowClick = () => closeMenu();
    const onEsc = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') closeMenu();
    };
    window.addEventListener('click', onWindowClick);
    window.addEventListener('keydown', onEsc);
    return () => {
      window.removeEventListener('click', onWindowClick);
      window.removeEventListener('keydown', onEsc);
    };
  });
</script>

{#snippet folderIcon(icon: string)}
  {#if icon === 'inbox'}
    <svg width="24" height="24" viewBox="0 0 24 24">
      <path fill="currentColor" d="M6.25 3h11.5a3.25 3.25 0 0 1 3.245 3.066L21 6.25v11.5a3.25 3.25 0 0 1-3.066 3.245L17.75 21H6.25a3.25 3.25 0 0 1-3.245-3.066L3 17.75V6.25a3.25 3.25 0 0 1 3.066-3.245zh11.5zM4.5 14.5v3.25a1.75 1.75 0 0 0 1.606 1.744l.144.006h11.5a1.75 1.75 0 0 0 1.744-1.607l.006-.143V14.5h-3.825a3.75 3.75 0 0 1-3.475 2.995l-.2.005a3.75 3.75 0 0 1-3.632-2.812l-.043-.188zv3.25zm13.25-10H6.25a1.75 1.75 0 0 0-1.744 1.606L4.5 6.25V13H9a.75.75 0 0 1 .743.648l.007.102a2.25 2.25 0 0 0 4.495.154l.005-.154a.75.75 0 0 1 .648-.743L15 13h4.5V6.25a1.75 1.75 0 0 0-1.607-1.744z"/>
    </svg>
  {:else if icon === 'sent'}
    <svg width="24" height="24" viewBox="0 0 24 24">
      <path fill="currentColor" d="M5.694 12L2.299 3.272a.75.75 0 0 1 .942-.982l.093.039l18 9a.75.75 0 0 1 .097 1.284l-.097.058l-18 9c-.583.291-1.217-.245-1.065-.848l.03-.095zL2.299 3.272zM4.402 4.54l2.61 6.71h6.627a.75.75 0 0 1 .743.648l.007.102a.75.75 0 0 1-.649.743l-.101.007H7.01l-2.609 6.71L19.322 12z"/>
    </svg>
  {:else if icon === 'drafts'}
    <svg width="24" height="24" viewBox="0 0 24 24">
        <g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5">
            <path d="M5.076 17C4.089 4.545 12.912 1.012 19.973 2.224c.286 4.128-1.734 5.673-5.58 6.387c.742.776 2.055 1.753 1.913 2.974c-.1.868-.69 1.295-1.87 2.147C11.85 15.6 8.854 16.78 5.076 17"/>
            <path d="M4 22c0-6.5 3.848-9.818 6.5-12"/>
        </g>
    </svg>
  {:else if icon === 'deleted'}
    <svg width="24" height="24" viewBox="0 0 24 24">
      <path fill="currentColor" d="M10 5h4a2 2 0 1 0-4 0M8.5 5a3.5 3.5 0 1 1 7 0h5.75a.75.75 0 0 1 0 1.5h-1.32l-1.17 12.111A3.75 3.75 0 0 1 15.026 22H8.974a3.75 3.75 0 0 1-3.733-3.389L4.07 6.5H2.75a.75.75 0 0 1 0-1.5zm2 4.75a.75.75 0 0 0-1.5 0v7.5a.75.75 0 0 0 1.5 0zM14.25 9a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-1.5 0v-7.5a.75.75 0 0 1 .75-.75m-7.516 9.467a2.25 2.25 0 0 0 2.24 2.033h6.052a2.25 2.25 0 0 0 2.24-2.033L18.424 6.5H5.576z"/>
    </svg>
  {:else if icon === 'junk'}
    <svg width="24" height="24" viewBox="0 0 24 24">
      <path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v4.06a6.5 6.5 0 0 0-1.5-1.078V8.75A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659H3.5v8.25c0 .966.784 1.75 1.75 1.75h6.063c.173.534.412 1.037.709 1.5H5.25A3.25 3.25 0 0 1 2 17.75zM23 17.5a5.5 5.5 0 1 1-11 0a5.5 5.5 0 0 1 11 0m-9.5 0c0 .834.255 1.608.691 2.248l5.557-5.557A4 4 0 0 0 13.5 17.5m4 4a4 4 0 0 0 3.309-6.248l-5.557 5.557a4 4 0 0 0 2.248.691"/>
    </svg>
  {:else if icon === 'archive'}
    <svg width="24" height="24" viewBox="0 0 24 24">
      <path fill="currentColor" d="M10.25 11a.75.75 0 0 0 0 1.5h3.5a.75.75 0 0 0 0-1.5zM3 5.25A2.25 2.25 0 0 1 5.25 3h13.5A2.25 2.25 0 0 1 21 5.25v1.5c0 .78-.397 1.467-1 1.871v8.629A3.75 3.75 0 0 1 16.25 21h-8.5A3.75 3.75 0 0 1 4 17.25V8.621A2.25 2.25 0 0 1 3 6.75zM5.5 9v8.25a2.25 2.25 0 0 0 2.25 2.25h8.5a2.25 2.25 0 0 0 2.25-2.25V9zm-.25-4.5a.75.75 0 0 0-.75.75v1.5c0 .414.336.75.75.75h13.5a.75.75 0 0 0 .75-.75v-1.5a.75.75 0 0 0-.75-.75z"/>
    </svg>
  {:else}
    <svg width="24" height="24" viewBox="0 0 24 24">
      <path fill="currentColor" d="M3.5 6.25V8h4.629a.75.75 0 0 0 .53-.22l1.53-1.53l-1.53-1.53a.75.75 0 0 0-.53-.22H5.25A1.75 1.75 0 0 0 3.5 6.25m-1.5 0A3.25 3.25 0 0 1 5.25 3h2.879a2.25 2.25 0 0 1 1.59.659L11.562 5.5h7.189A3.25 3.25 0 0 1 22 8.75v9A3.25 3.25 0 0 1 18.75 21H5.25A3.25 3.25 0 0 1 2 17.75zM3.5 9.5v8.25c0 .966.784 1.75 1.75 1.75h13.5a1.75 1.75 0 0 0 1.75-1.75v-9A1.75 1.75 0 0 0 18.75 7h-7.19L9.72 8.841a2.25 2.25 0 0 1-1.591.659z"/>
    </svg>
  {/if}
{/snippet}

{#snippet folderRow(folder: Folder)}
  <button
    class="folder-item"
    class:selected={selectedFolder === folder.id}
    onclick={() => onSelectFolder(folder.id)}
  >
    <span class="folder-icon">
      {@render folderIcon(folder.icon)}
    </span>
    <span class="folder-name">{folderDisplayName(folder)}</span>
    {#if (unreadCounts[folder.id] ?? 0) > 0}
      <span class="unread-badge">{unreadCounts[folder.id]}</span>
    {/if}
    <span class="folder-pin" class:folder-pin-hidden={!folder.isFavorite} aria-hidden={!folder.isFavorite} aria-label={t('folders.pinned')}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="m16.243 2.932l4.825 4.826a2.75 2.75 0 0 1-.715 4.404l-4.87 2.435a.75.75 0 0 0-.374.426l-1.44 4.166a1.25 1.25 0 0 1-2.065.476L8.5 16.561L4.06 21H3v-1.062L7.44 15.5l-3.105-3.104a1.25 1.25 0 0 1 .476-2.066l4.166-1.439a.75.75 0 0 0 .426-.374l2.435-4.87a2.75 2.75 0 0 1 4.405-.715m3.765 5.886l-4.826-4.825a1.25 1.25 0 0 0-2.002.324l-2.435 4.871a2.25 2.25 0 0 1-1.278 1.12l-3.789 1.31l6.705 6.704l1.308-3.788a2.25 2.25 0 0 1 1.12-1.278l4.872-2.436a1.25 1.25 0 0 0 .325-2.002"/>
      </svg>
    </span>
    <span
      class="folder-actions-btn"
      role="button"
      tabindex="-1"
      aria-label={t('folders.folderActions')}
      onclick={(ev) => openFolderMenu(ev, folder.id)}
      onkeydown={(ev) => {
        if (ev.key === 'Enter' || ev.key === ' ') {
          ev.preventDefault();
          openFolderMenu(ev, folder.id);
        }
      }}
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
        <circle cx="5" cy="12" r="1.8" />
        <circle cx="12" cy="12" r="1.8" />
        <circle cx="19" cy="12" r="1.8" />
      </svg>
    </span>
  </button>
{/snippet}

<aside class="folder-pane" class:active>
  <!-- Folders -->
  <div class="folder-section">
    <div class="section-header section-header-with-actions">
      <div class="section-title">
        <span>{t('folders.folders')}</span>
      </div>
      <button
        class="folder-actions-btn root-actions-btn"
        tabindex="-1"
        aria-label={t('folders.folderActions')}
        onclick={openRootMenu}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
          <circle cx="5" cy="12" r="1.8" />
          <circle cx="12" cy="12" r="1.8" />
          <circle cx="19" cy="12" r="1.8" />
        </svg>
      </button>
    </div>
    {#each sortedFolders as folder}
      {@render folderRow(folder)}
    {/each}
  </div>

  {#if menu}
    <div
      class="context-menu"
      style="left: {menu.x}px; top: {menu.y}px;"
    >
      {#if menu.kind === 'root-folders'}
        <button class="menu-item" onclick={doCreateFolder}>{t('folders.createNew')}</button>
      {:else if menuFolder}
        {#if menuFolder.isSystem === false}
          <button class="menu-item" onclick={() => doRenameFolder(menuFolder.id)}>{t('folders.rename')}</button>
          <button class="menu-item" onclick={() => doDeleteFolder(menuFolder.id)}>{t('common.delete')}</button>
        {/if}
        {#if (menuFolder.id === 'deleted' || menuFolder.id === 'junk') && (folderEmailCounts[menuFolder.id] ?? 0) > 0}
          <button class="menu-item" onclick={() => doEmptyFolder(menuFolder.id)}>{t('folders.emptyFolder')}</button>
        {/if}
        <button class="menu-item" onclick={() => doToggleFavorite(menuFolder.id)}>
          {menuFolder.isFavorite ? t('folders.unpin') : t('folders.pin')}
        </button>
      {/if}
    </div>
  {/if}
</aside>

<style>
  .folder-pane {
    width: 15%;
    height: 100%;
    min-width: 180px;
    max-width: 270px;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border-light);
    border-radius: 4px;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    flex-shrink: 0;
    user-select: none;
  }

  /* ── Sections ── */
  .section-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px 8px 20px;
    font-size: 13px;
    font-weight: 600;
    border-bottom: 1px solid var(--border-light);
    color: var(--text-primary);
    letter-spacing: 0.03em;
  }

  .section-header-with-actions {
    justify-content: space-between;
    gap: 0;
  }

  .section-title {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  /* ── Folder Items ── */
  .folder-item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 8px 6px 16px;
    font-size: 13px;
    color: var(--text-primary);
    border-left: 4px solid transparent;
    border-radius: 0;
    transition: background 0.1s ease;
    text-align: left;
    position: relative;
  }

  .folder-item:hover {
    background: var(--bg-hover);
    border-left-color: var(--border-hover);
  }

  .folder-item.selected {
    background: var(--bg-selected);
    font-weight: 600;
  }

  .folder-item.selected:hover {
    border-left-color: var(--accent);
  }

  .folder-pane.active .folder-item.selected:not(:hover) {
    border-left-color: var(--accent-active);
  }

  .folder-item:focus {
    outline: none;
  }

  .folder-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    opacity: 0.75;
  }

  .folder-item.selected .folder-icon {
    opacity: 1;
  }

  .folder-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .unread-badge {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-active);
    min-width: 16px;
    text-align: right;
    flex-shrink: 0;
  }

  .folder-pin {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--accent-active);
    flex-shrink: 0;
  }

  .folder-item.selected .folder-pin {
    opacity: 1;
  }

  .folder-pin-hidden {
    visibility: hidden;
  }

  .folder-actions-btn {
    display: inline-flex;
    visibility: hidden;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    color: var(--text-secondary);
    border-radius: 4px;
    flex-shrink: 0;
  }

  .folder-item > .folder-actions-btn {
    position: absolute;
    right: 16px;
    top: 50%;
    transform: translateY(-50%);
  }

  .folder-actions-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .root-actions-btn {
    margin-right: 2px;
    visibility: visible;
  }

  .folder-item:hover .folder-actions-btn {
    visibility: visible;
  }

  .folder-item:hover .unread-badge,
  .folder-item:hover .folder-pin {
    visibility: hidden;
  }

  .context-menu {
    position: fixed;
    min-width: 180px;
    background: var(--bg-primary);
    border: 2px solid var(--border-primary);
    border-radius: 6px;
    box-shadow: var(--shadow-md, 0 8px 32px rgba(0, 0, 0, 0.18));
    z-index: 1000;
  }

  .menu-item {
    width: 100%;
    text-align: left;
    border-radius: 4px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--text-primary);
  }

  .menu-item:hover {
    background: var(--bg-hover);
  }


</style>
