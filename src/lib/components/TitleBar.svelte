<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import type { Account, NavItem } from '$lib/types';
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
  }

  let { activeNav, folderPaneVisible, onToggleFolderPane, searchQuery, onSearch, accounts, activeAccountId, onSelectAccount, onSearchTab, onSearchEsc }: Props = $props();

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
  const isMac = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent);

  const appWindow = getCurrentWindow();

  async function checkMaximized() {
    isMaximized = await appWindow.isMaximized();
  }

  onMount(() => {
    checkMaximized();
    const unlisten = listen('tauri://resize', () => {
      checkMaximized();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  // Update OS window title (taskbar, Alt+Tab, task manager) when active account changes
  $effect(() => {
    const suffix = activeAccountName ? `: ${activeAccountName}` : '';
    appWindow.setTitle(`Mail${suffix}`).catch(() => {});
  });
</script>

<header class="title-bar" class:macos={isMac}>
  <!-- Left: app brand -->
  <div class="bar-left">
    {#if isMac}<div class="traffic-light-spacer"></div>{/if}
    <div class="app-brand">
        <svg class="app-icon" width="24" height="24" viewBox="0 0 24 24">
            <g fill="currentColor">
	            <path d="M16.37,20.29c-1.31,.69-2.79,1.08-4.37,1.08-5.18,0-9.38-4.2-9.38-9.38S6.82,2.62,12,2.62s9.26,3.83,9.26,9c0,.74-.11,1.38-.45,2.02-.37,.69-.71,1.17-1.34,1.55s-1.41,.6-2.02,.6c-.73,0-1.11-.23-1.26-.41-.35-.44-.39-1.39-.11-2.81l1.01-5.38-.1-.05c-.86-.4-2.02-.62-3.18-.62-1.95,0-3.75,.76-5.07,2.14-.62,.65-1.11,1.41-1.45,2.25-.34,.85-.52,1.74-.52,2.66,0,1.13,.34,2.09,.98,2.79,.6,.66,1.42,1.02,2.31,1.02,1.71,0,3.05-.78,4.08-2.39,.02,.71,.24,1.3,.64,1.72,.41,.43,1.23,.69,2.01,.77,.87,.09,2.11-.07,3.27-.62,1.08-.51,1.77-1.34,2.2-2.14,0,0,.78-1.33,.78-3.23C23.04,5.34,18.15,.87,12,.87S.87,5.85,.87,12s4.98,11.13,11.13,11.13c1.9,0,3.69-.48,5.25-1.32l-.88-1.52Zm-2.19-8.62c-.32,1.82-1.85,3.85-3.42,3.85-1.12,0-1.78-.78-1.78-2.07,0-2.77,2.06-5.11,4.49-5.11,.46,0,.89,.06,1.3,.16l-.59,3.17Z"/>
            </g>
        </svg>
      <span class="app-name">Mail{activeAccountName ? `: ${activeAccountName}` : ''}</span>
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

    {#if !isMac}
      <div class="window-controls">
        <button class="win-btn" tabindex="-1" data-tooltip={t('titleBar.minimize')} data-tooltip-position="bottom" aria-label={t('titleBar.minimize')} onclick={() => appWindow.minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10">
            <line x1="0" y1="5" x2="10" y2="5" stroke="currentColor" stroke-width="1" />
          </svg>
        </button>
        <button class="win-btn" tabindex="-1" data-tooltip={isMaximized ? t('titleBar.restore') : t('titleBar.maximize')} data-tooltip-position="bottom" aria-label={isMaximized ? t('titleBar.restore') : t('titleBar.maximize')} onclick={() => appWindow.toggleMaximize()}>
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
        <button class="win-btn win-close" tabindex="-1" data-tooltip={t('titleBar.close')} data-tooltip-position="bottom" aria-label={t('titleBar.close')} onclick={() => appWindow.close()}>
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
    gap: 12px;
  }

  .app-icon {
    flex-shrink: 0;
    color: var(--accent-active);
  }

  .app-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--accent-active);
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
    max-width: 440px;
    padding: 3px 10px;
    background: var(--bg-primary);
    border-radius: 5px;
    border: 1px solid var(--border-light);
    transition: border-color 0.15s, box-shadow 0.15s;
    height: 26px;
    -webkit-app-region: no-drag;
  }

  .search-box:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
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
    box-shadow: 3px 3px 6px rgba(0, 0, 0, 0.5);
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
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 9px;
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
    background: var(--accent-active);
    color: var(--text-on-accent);
    font-size: 9px;
    font-weight: 700;
    line-height: 14px;
    border-radius: 7px;
    text-align: center;
    pointer-events: none;
    box-sizing: border-box;
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
