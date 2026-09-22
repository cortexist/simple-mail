<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '$lib/stores/safeListen';
  import { onMount } from 'svelte';
  import type { Account, NavItem, WindowChrome } from '$lib/types';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    activeNav: NavItem;
    folderPaneVisible: boolean; 
    onToggleFolderPane: () => void;
    searchQuery: string;
    onSearch: (query: string) => void;
    accounts: Account[];
    activeAccountId: string;
    onSelectAccount: (accountId: string) => void;
    onSearchTab?: () => void;
    onSearchEsc?: () => void;
    /** When non-null, a clickable "i" icon appears next to the search box that
     *  opens a callout explaining why search results are partial. */
    searchInfoText?: string | null;
  }

  let { activeNav, folderPaneVisible, onToggleFolderPane, searchQuery, onSearch, accounts, activeAccountId, onSelectAccount, onSearchTab, onSearchEsc, searchInfoText = null }: Props = $props();

  let searchInputEl = $state<HTMLInputElement | undefined>();

  export function focusSearch() {
    searchInputEl?.focus();
    searchInputEl?.select();
  }

  let activeAccountName = $derived.by(() => {
    const account = accounts.find((a) => a.id === activeAccountId);
    return account?.alias || account?.name || '';
  });

  let isMaximized = $state(false);

  // Lazy: getCurrentWindow() dereferences __TAURI_INTERNALS__.metadata, which
  // doesn't exist in a plain browser (`npm run dev`) — calling it at component
  // init throws during mount and wedges the whole page on "Loading Mail…".
  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  const appWindow = inTauri ? getCurrentWindow() : null;

  // Which controls this environment expects us to draw. The window is
  // undecorated, so the bar is ours and the answer can't come from the
  // toolkit — the backend works it out in src-tauri/src/chrome.rs.
  //
  // Nothing until the answer arrives, and nothing in a plain browser where
  // there is no window to control: an empty corner is better than one that
  // flashes buttons it then takes away.
  const none: WindowChrome = { controls: false, minimize: false, maximize: false, trafficLightInset: false };
  let chrome = $state<WindowChrome>(none);

  async function checkMaximized() {
    if (appWindow) isMaximized = await appWindow.isMaximized();
  }

  onMount(() => {
    // The resize listener is only registered once the backend has answered, so
    // it may outlive an unmount that happens while that call is in flight —
    // hence the flag as well as the handle.
    let disposed = false;
    let unlisten: ReturnType<typeof listen> | null = null;

    (async () => {
      if (!inTauri) return;
      try {
        chrome = await invoke<WindowChrome>('window_chrome');
      } catch (err) {
        // A backend that can't answer leaves the bar plain, which is still a
        // usable window — the compositor can always close it.
        console.error('window_chrome failed:', err);
        return;
      }
      // Nothing here can maximize the window, so there is no such state to
      // follow and no icon that depends on it.
      if (disposed || !chrome.maximize) return;
      checkMaximized();
      unlisten = listen('tauri://resize', () => {
        checkMaximized();
      });
    })();

    return () => {
      disposed = true;
      unlisten?.then((fn) => fn());
    };
  });

  // Update OS window title (taskbar, Alt+Tab, task manager) when active account changes
  $effect(() => {
    const suffix = activeAccountName ? `: ${activeAccountName}` : '';
    appWindow?.setTitle(`Mail${suffix}`).catch(() => {});
  });
</script>

<header class="title-bar">
  <!-- Left: app brand -->
  <div class="bar-left">
    {#if chrome.trafficLightInset}<div class="traffic-light-spacer"></div>{/if}
    <div class="app-brand">
      <svg class="app-icon" width="24" height="24" viewBox="0 0 512 512">
        <path fill="currentColor" d="M270.64,334.31c-8.17,8.17-15.84,15.73-23.04,22.68-7.2,6.96-14.4,13.8-21.6,20.52-4.8,4.32-9.61,8.28-14.4,11.88-4.8,3.6-9.97,6.6-15.48,9-5.52,2.41-11.16,3.85-16.92,4.32-5.76,.48-11.77-.72-18-3.6-12.49-5.28-20.88-13.56-25.2-24.84-4.32-11.27-6.48-23.15-6.48-35.64-.48-22.56,4.07-43.2,13.68-61.92,9.6-18.72,20.63-36.72,33.12-54,13.92-19.19,30.48-34.44,49.68-45.72,19.19-11.27,39.12-21.71,59.76-31.32,6.23,1.44,12.6,2.64,19.08,3.6,6.48,.97,12.35,2.77,17.64,5.4,5.28,2.64,9.83,6.37,13.68,11.16,3.84,4.8,6.48,11.52,7.92,20.16,6.72-.96,11.4,.25,14.04,3.6,2.63,3.36,4.92,8.64,6.84,15.84,3.35,12,3.35,22.68,0,32.04-3.36,9.36-6.96,18.84-10.8,28.44l-33.12,91.44,5.76,5.04c7.2-6.23,14.76-11.99,22.68-17.28,7.92-5.28,14.51-11.52,19.8-18.72,11.04-16.31,21.96-32.76,32.76-49.32,10.8-16.56,19.8-33.95,27-52.2,6.72-15.36,9.6-31.68,8.64-48.96-.97-17.28-5.29-33.59-12.96-48.96-6.73-13.92-16.56-23.4-29.52-28.44s-26.4-8.03-40.32-9c-24.96-.96-48.13,3.24-69.48,12.6-21.36,9.36-41.4,21.24-60.12,35.64-33.6,25.92-60.73,55.8-81.36,89.64-20.64,33.84-35.04,71.89-43.2,114.12-3.36,15.84-4.08,31.68-2.16,47.52,1.91,15.84,8.39,30.49,19.44,43.92,6.72,8.17,13.92,14.4,21.6,18.72,7.67,4.32,15.84,7.09,24.48,8.28,8.64,1.2,17.52,1.33,26.64,.36,9.11-.96,18.24-2.4,27.36-4.32,13.43-2.88,26.64-5.64,39.6-8.28,12.96-2.63,26.16-4.68,39.6-6.12-10.56,9.61-23.52,17.89-38.88,24.84-15.37,6.96-31.44,12.36-48.24,16.2-16.81,3.85-33.24,6.12-49.32,6.84-16.09,.72-30.13-.6-42.12-3.96-31.68-8.16-54.48-23.76-68.4-46.8-13.93-23.04-19.2-49.68-15.84-79.92,7.67-73.44,29.76-137.16,66.24-191.16,36.47-54,88.31-97.08,155.52-129.24,16.8-8.16,34.31-13.32,52.56-15.48,18.24-2.16,36-1.68,53.28,1.44,17.28,3.13,33.48,8.76,48.6,16.92,15.12,8.17,27.72,18.48,37.8,30.96,13.92,16.81,22.92,34.92,27,54.36,4.07,19.44,6.12,40.21,6.12,62.28,0,16.81-2.16,32.4-6.48,46.8-4.32,14.4-9.97,28.2-16.92,41.4-6.96,13.21-15.01,25.81-24.12,37.8-9.12,12-18.48,24.01-28.08,36-8.17,10.08-17.4,19.21-27.72,27.36-10.33,8.17-21,16.32-32.04,24.48-10.08,7.2-19.2,10.92-27.36,11.16-8.17,.25-15.48-1.91-21.96-6.48-6.48-4.56-12-10.91-16.56-19.08-4.57-8.16-7.81-17.03-9.72-26.64-.97-3.84-1.2-7.92-.72-12.24,.47-4.32,.72-9.36,.72-15.12Zm-72.72-3.6c10.08-7.67,20.27-15.36,30.6-23.04,10.32-7.67,20.27-15.72,29.88-24.12,9.6-8.39,18.6-17.39,27-27,8.39-9.6,15.72-20.4,21.96-32.4-9.12-5.76-16.45-8.75-21.96-9-5.52-.24-12.36,2.52-20.52,8.28-18.25,13.44-32.65,29.28-43.2,47.52-10.56,18.25-18.48,38.16-23.76,59.76Z"/>
      </svg>
      <span class="app-name">{activeAccountName ? `${activeAccountName}` : ''}</span>
    </div>
  </div>

  <!-- Center: search -->
  <div class="bar-center">
    <div class="search-box">
      <svg class="search-icon" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="11" cy="11" r="8" />
        <line x1="21" y1="21" x2="16.65" y2="16.65" />
      </svg>
      <input
        bind:this={searchInputEl}
        type="text"
        class="search-input"
        tabindex="-1"
        placeholder={t('titleBar.searchPlaceholder')}
        value={searchQuery}
        oninput={(e) => onSearch(e.currentTarget.value)}
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            onSearch('');
            searchInputEl?.blur();
            onSearchEsc?.();
          } else if (e.key === 'Tab' || e.key === 'ArrowDown') {
            e.preventDefault();
            searchInputEl?.blur();
            onSearchTab?.();
          }
        }}
      />
      {#if searchQuery}
        <button
          class="search-clear"
          tabindex="-1"
          aria-label="{t('titleBar.clearFind')}"
          data-tooltip={t('titleBar.clearFind')}
          data-tooltip-position="bottom"
          onclick={() => onSearch('')}
        >
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      {/if}
      {#if searchInfoText}
        <span
          class="search-info"
          role="img"
          aria-label={searchInfoText}
          data-tooltip={searchInfoText}
          data-tooltip-position="bottom"
          data-tooltip-wide
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="16" x2="12" y2="12" />
            <line x1="12" y1="8" x2="12.01" y2="8" />
          </svg>
        </span>
      {/if}
    </div>
  </div>

  <!-- Right: settings, account, window controls -->
  <div class="bar-right">
    <div class="account-switcher">
      {#each accounts as account (account.id)}
        {@const unreadCount = account.emails.filter((e) => !e.isRead && e.folder !== 'sent' && e.folder !== 'drafts' && e.folder !== 'archive' && e.folder !== 'deleted' && e.folder !== 'junk').length}
        <button
          class="account-btn"
          class:active={account.id === activeAccountId}
          tabindex="-1"
          data-tooltip="{account.name} ({account.email}){unreadCount ? ` · ${t('titleBar.unread', { count: unreadCount })}` : ''}"
          data-tooltip-position="bottom"
          onclick={() => onSelectAccount(account.id)}
        >
          <span class="account-avatar" style="background: {account.color}">
            {#if account.avatarUrl}
              <img class="account-avatar-img" src={account.avatarUrl} alt={account.name} />
            {:else}
              {account.initials}
            {/if}
          </span>
          {#if unreadCount > 0}
            <span class="unread-badge">{unreadCount > 99 ? '99+' : unreadCount}</span>
          {/if}
        </button>
      {/each}
    </div>  

    <!-- ── Mail Commands ── -->
    <button
      class="titlebar-btn"
      class:visible={activeNav === 'mail'}
      class:active={folderPaneVisible}
      tabindex="-1"
      data-tooltip={folderPaneVisible ? t('commandBar.hideFolderPane') : t('commandBar.showFolderPane')}
      data-tooltip-position="bottom"
      aria-label={folderPaneVisible ? t('commandBar.hideFolderPane') : t('commandBar.showFolderPane')}
      onclick={onToggleFolderPane}
    >
      {#if folderPaneVisible}
        <svg width="20" height="20" viewBox="0 0 16 16"><path fill="currentColor" d="M1 3.5v9C1 13.879 2.122 15 3.5 15h9c1.378 0 2.5-1.121 2.5-2.5v-9C15 2.122 13.878 1 12.5 1h-9A2.503 2.503 0 0 0 1 3.5M12.5 14H7V2h5.5c.827 0 1.5.673 1.5 1.5v9c0 .827-.673 1.5-1.5 1.5M2 3.5C2 2.673 2.673 2 3.5 2H6v12H3.5c-.827 0-1.5-.673-1.5-1.5z"/></svg>
      {:else }
        <svg width="20" height="20" viewBox="0 0 16 16"><path fill="currentColor" d="M12.5 1A2.5 2.5 0 0 1 15 3.5v9a2.5 2.5 0 0 1-2.5 2.5h-9A2.5 2.5 0 0 1 1 12.5v-9A2.5 2.5 0 0 1 3.5 1zm0 13a1.5 1.5 0 0 0 1.5-1.5v-9A1.5 1.5 0 0 0 12.5 2H7v12z"/></svg>
      {/if}
    </button>

    <!-- Only the controls that do something here are drawn — see
         src-tauri/src/chrome.rs for who gets what. -->
    {#if chrome.controls}
      <div class="window-controls">
        {#if chrome.minimize}
        <button class="win-btn" tabindex="-1" data-tooltip={t('titleBar.minimize')} data-tooltip-position="bottom" aria-label={t('titleBar.minimize')} onclick={() => appWindow?.minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10">
            <line x1="0" y1="5" x2="10" y2="5" stroke="currentColor" stroke-width="1" />
          </svg>
        </button>
        {/if}
        {#if chrome.maximize}
        <button class="win-btn" tabindex="-1" data-tooltip={isMaximized ? t('titleBar.restore') : t('titleBar.maximize')} data-tooltip-position="bottom" aria-label={isMaximized ? t('titleBar.restore') : t('titleBar.maximize')} onclick={() => appWindow?.toggleMaximize()}>
          {#if isMaximized}
            <svg width="10" height="10" viewBox="0 0 10 10">
              <g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1">
                 <rect x=".64" y="2.51" width="6.64" height="6.64" rx="1" ry="1" />
                 <path d="M2.72,2.51V1.4c0-.31,.25-.55,.55-.55h5.53c.31,0,.55,.25,.55,.55V6.94c0,.31-.25,.55-.55,.55h-1.52"/>
             </g>
            </svg>
          {:else}
            <svg width="10" height="10" viewBox="0 0 10 10">
              <rect x="1" y="1" width="8" height="8" stroke="currentColor" stroke-width="1" fill="none" />
            </svg>
          {/if}
        </button>
        {/if}
        <!-- Close is unconditional: every window manager can close a window. -->
        <button class="win-btn win-close" tabindex="-1" data-tooltip={t('titleBar.close')} data-tooltip-position="bottom" aria-label={t('titleBar.close')} onclick={() => appWindow?.close()}>
          <svg width="10" height="10" viewBox="0 0 10 10">
            <line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" stroke-width="1.2" />
            <line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1.2" />
          </svg>
        </button>
      </div>
    {/if}
  </div>
</header>

<style>
  .title-bar {
    height: 40px;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    padding: 0;
    flex-shrink: 0;
    -webkit-app-region: drag;
    user-select: none;
    border-bottom: 1px solid var(--border-light);
  }

  /* ── Left zone ── */
  .bar-left {
    display: flex;
    align-items: center;
    padding: 0 10px;
    flex-shrink: 0;
  }

  .app-brand {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .app-icon {
    flex-shrink: 0;
    color: var(--accent);  
  }

  .app-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--accent-text);
  }

  /* ── Center zone: search ── */
  .bar-center {
    flex: 1;
    display: flex;
    justify-content: center;
    padding: 0 12px;
    min-width: 0;
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    max-width: 380px;
    padding: 3px 10px;
    background: var(--bg-primary);
    border-radius: 5px;
    border: 1px solid var(--border-light);
    transition: border-color 0.15s, box-shadow 0.15s;
    height: 26px;
    -webkit-app-region: no-drag;
  }

  .search-box:focus-within {
    border-color: var(--accent-text);
    box-shadow: 0 0 0 1px var(--accent-text);
  }

  .search-icon {
    flex-shrink: 0;
    color: var(--text-tertiary);
  }

  .search-input {
    flex: 1;
    font-size: 12px;
    line-height: 1;
    padding: 0;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-tertiary);
  }

  .search-clear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    color: var(--text-tertiary);
    transition: background 0.1s, color 0.1s;
  }

  .search-clear:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .search-info {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin-left: 2px;
    color: var(--text-tertiary);
  }
  .search-info:hover {
    color: var(--accent-text);
  }

  /* ── Right zone ── */
  .bar-right {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-right: 0;
    flex-shrink: 0;
    -webkit-app-region: no-drag;
  }

  .titlebar-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 4px;
    color: var(--text-secondary);
    transition: background 0.1s, color 0.1s;
    outline: none;
    visibility: hidden;
  }

  .titlebar-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .titlebar-btn.visible {
    visibility:visible;
  }

  .account-switcher {
    display: flex;
    align-items: center;
    gap: 2px;
    margin: 0 16px;
  }

  .account-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    transition: transform 0.1s;
    padding: 0;
    position: relative;
    outline: none;
  }

  .account-btn.active {
    box-shadow: var(--shadow-md, 0 4px 16px rgba(0, 0, 0, 0.18));
  }

   .account-btn.active .unread-badge {
     opacity: 1;
   }

  .account-btn > .account-avatar {
    opacity: 0.2;
    transition: opacity 0.15s;
  }

  .account-btn:hover > .account-avatar {
    opacity: 0.75;
  }

  .account-btn.active > .account-avatar {
    opacity: 1;
  }

  .account-avatar {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 600;
    color: white;
    overflow: hidden;
  }

  .account-avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .unread-badge {
    position: absolute;
    bottom: 0;
    right: -2px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    background: var(--accent);
    color: var(--text-on-accent);
    font-size: 9px;
    font-weight: 700;
    line-height: 14px;
    border-radius: 7px;
    text-align: center;
    pointer-events: none;
    box-sizing: border-box;
    opacity: 0.6;
  }

  /* ── Window controls ── */
  .window-controls {
    display: flex;
    align-items: stretch;
    height: 40px;
    gap: 6px;
  }

  .win-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    width: 40px;
    color: var(--text-primary);
    transition: background 0.1s;
    outline: none;
  }

  .win-btn:hover {
    background: var(--bg-hover);
  }

  .win-close:hover {
    background: #e81123;
    color: white;
  }

  /* ── macOS: space for native traffic light buttons ── */
  .traffic-light-spacer {
    width: 70px;
    flex-shrink: 0;
  }
</style>
