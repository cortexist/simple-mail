<script lang="ts">
  import type { NavItem } from '$lib/types';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    selectedItem: NavItem;
    active?: boolean;
    onSelectItem: (item: NavItem) => void;
    onOpenSettings: () => void;
  }

  let { selectedItem, active = false, onSelectItem, onOpenSettings }: Props = $props();

  const navItemIds: { id: NavItem; key: string }[] = [
    { id: 'mail',     key: 'nav.mail'     },
    { id: 'calendar', key: 'nav.calendar' },
    { id: 'contacts', key: 'nav.contacts' },
  ];
</script>

<nav class="nav-rail">
  <div class="nav-items">
    {#each navItemIds as item}
      <button
        class="nav-item"
        class:selected={selectedItem === item.id}
        class:active={active && selectedItem === item.id}
        tabindex="-1"
        onclick={() => onSelectItem(item.id)}
        data-tooltip={t(item.key)}
        data-tooltip-position="right"
      >    
        <span class="nav-icon">
          {#if item.id === 'mail'}
            <svg width="24" height="24" viewBox="0 0 24 24">
              <path fill="currentColor" d="M5.25 4h13.5a3.25 3.25 0 0 1 3.245 3.066L22 7.25v9.5a3.25 3.25 0 0 1-3.066 3.245L18.75 20H5.25a3.25 3.25 0 0 1-3.245-3.066L2 16.75v-9.5a3.25 3.25 0 0 1 3.066-3.245zh13.5zM20.5 9.373l-8.15 4.29a.75.75 0 0 1-.603.043l-.096-.042L3.5 9.374v7.376a1.75 1.75 0 0 0 1.606 1.744l.144.006h13.5a1.75 1.75 0 0 0 1.744-1.607l.006-.143zM18.75 5.5H5.25a1.75 1.75 0 0 0-1.744 1.606L3.5 7.25v.429l8.5 4.474l8.5-4.475V7.25a1.75 1.75 0 0 0-1.607-1.744z"/>
            </svg>
          {:else if item.id === 'calendar'}
            <svg width="24" height="24" viewBox="0 0 24 24">
              <path fill="currentColor" d="M17.75 3A3.25 3.25 0 0 1 21 6.25v11.5A3.25 3.25 0 0 1 17.75 21H6.25A3.25 3.25 0 0 1 3 17.75V6.25A3.25 3.25 0 0 1 6.25 3zm1.75 5.5h-15v9.25c0 .966.784 1.75 1.75 1.75h11.5a1.75 1.75 0 0 0 1.75-1.75zm-11.75 6a1.25 1.25 0 1 1 0 2.5a1.25 1.25 0 0 1 0-2.5m4.25 0a1.25 1.25 0 1 1 0 2.5a1.25 1.25 0 0 1 0-2.5m-4.25-4a1.25 1.25 0 1 1 0 2.5a1.25 1.25 0 0 1 0-2.5m4.25 0a1.25 1.25 0 1 1 0 2.5a1.25 1.25 0 0 1 0-2.5m4.25 0a1.25 1.25 0 1 1 0 2.5a1.25 1.25 0 0 1 0-2.5m1.5-6H6.25A1.75 1.75 0 0 0 4.5 6.25V7h15v-.75a1.75 1.75 0 0 0-1.75-1.75"/>
            </svg>
          {:else if item.id === 'contacts'}
            <svg width="24" height="24" viewBox="0 0 24 24">
              <path fill="currentColor" d="M7.48 10.385c1.136 0 2.068.475 2.853.983c.387.25.774.534 1.12.777c.359.252.695.474 1.031.65c.166.083.363.23.556.22c.048-.002.242-.026.569-.42l.94-1.302c1.126-1.237 3.204-1.218 4.295.105l3.419 4.186c.271.463.3 1.02.097 1.507l-.12.237c-.44.718-1.132 1.867-2.045 2.83c-.906.957-2.139 1.845-3.674 1.846c-1.137 0-2.07-.475-2.854-.983c-.387-.25-.775-.535-1.121-.778a12 12 0 0 0-.777-.51l-.254-.141c-.322-.169-.43-.226-.555-.22c-.05.004-.248.03-.58.433l-.717 1.024v.002c-.99 1.405-3.062 1.555-4.275.405l-3.494-4.208a1.69 1.69 0 0 1-.133-1.968l.376-.609c.422-.67.982-1.498 1.667-2.22c.906-.957 2.14-1.846 3.676-1.846m0 1.485c-.935 0-1.801.544-2.596 1.383c-.734.774-1.299 1.68-1.857 2.583a.21.21 0 0 0 .012.247l3.264 3.96l.108.118c.574.54 1.58.459 2.035-.186l3.924-5.594a4 4 0 0 1-.575-.27c-.425-.223-.825-.49-1.194-.75c-.382-.268-.72-.516-1.076-.747c-.701-.454-1.338-.744-2.045-.744m10.216.474c-.546-.663-1.66-.62-2.144.067l.002.001l-.56.797l.009.006l-3.364 4.796c.264.093.466.213.566.265l.315.176c.308.181.602.381.879.575c.381.268.719.516 1.075.746c.702.455 1.34.744 2.047.744c.934 0 1.8-.543 2.595-1.381c.789-.832 1.404-1.846 1.856-2.584c.04-.093.04-.159-.012-.247zM7 3a3 3 0 1 1 0 6a3 3 0 0 1 0-6m10 0a3 3 0 1 1 0 6a3 3 0 0 1 0-6M7 4.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 0 0 0-3m10 0a1.5 1.5 0 1 0 0 3a1.5 1.5 0 0 0 0-3"/>
            </svg>
          {/if}
        </span>
      </button>
    {/each}
  </div>
  <div class="nav-rail-bottom">
    <button class="titlebar-btn" tabindex="-1" data-tooltip={t('titleBar.settingsShortcut')} data-tooltip-position="right" aria-label={t('titleBar.settingsShortcut')} onclick={onOpenSettings}>
      <svg width="20" height="20" viewBox="0 0 24 24">
        <path fill="currentColor" d="M12.012 2.25c.734.008 1.465.093 2.182.253a.75.75 0 0 1 .582.649l.17 1.527a1.384 1.384 0 0 0 1.927 1.116l1.4-.615a.75.75 0 0 1 .85.174a9.8 9.8 0 0 1 2.205 3.792a.75.75 0 0 1-.272.825l-1.241.916a1.38 1.38 0 0 0 0 2.226l1.243.915a.75.75 0 0 1 .272.826a9.8 9.8 0 0 1-2.204 3.792a.75.75 0 0 1-.849.175l-1.406-.617a1.38 1.38 0 0 0-1.926 1.114l-.17 1.526a.75.75 0 0 1-.571.647a9.5 9.5 0 0 1-4.406 0a.75.75 0 0 1-.572-.647l-.169-1.524a1.382 1.382 0 0 0-1.925-1.11l-1.406.616a.75.75 0 0 1-.85-.175a9.8 9.8 0 0 1-2.203-3.796a.75.75 0 0 1 .272-.826l1.243-.916a1.38 1.38 0 0 0 0-2.226l-1.243-.914a.75.75 0 0 1-.272-.826a9.8 9.8 0 0 1 2.205-3.792a.75.75 0 0 1 .85-.174l1.4.615a1.387 1.387 0 0 0 1.93-1.118l.17-1.526a.75.75 0 0 1 .583-.65q1.074-.238 2.201-.252m0 1.5a9 9 0 0 0-1.354.117l-.11.977A2.886 2.886 0 0 1 6.526 7.17l-.899-.394A8.3 8.3 0 0 0 4.28 9.092l.797.587a2.88 2.88 0 0 1 .001 4.643l-.799.588c.32.842.776 1.626 1.348 2.322l.905-.397a2.882 2.882 0 0 1 4.017 2.318l.109.984c.89.15 1.799.15 2.688 0l.11-.984a2.88 2.88 0 0 1 4.018-2.322l.904.396a8.3 8.3 0 0 0 1.348-2.318l-.798-.588a2.88 2.88 0 0 1-.001-4.643l.797-.587a8.3 8.3 0 0 0-1.348-2.317l-.897.393a2.884 2.884 0 0 1-4.023-2.324l-.109-.976a9 9 0 0 0-1.334-.117M12 8.25a3.75 3.75 0 1 1 0 7.5a3.75 3.75 0 0 1 0-7.5m0 1.5a2.25 2.25 0 1 0 0 4.5a2.25 2.25 0 0 0 0-4.5"/>
      </svg>
    </button>
  </div>
</nav>

<style>
  .nav-rail {
    width: 48px;
    background: var(--bg-tertiary);
    border-right: 1px solid var(--border-light);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: space-between;
    padding: 4px 0 12px 0;
    flex-shrink: 0;
    user-select: none;
  }

  .nav-items {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }

  .nav-item {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 48px;
    height: 40px;
    color: var(--text-secondary);
    transition: background 0.12s ease, color 0.12s ease;
    outline: none;
    border-left: 4px solid transparent;
  }

  .nav-item:hover {
    background: var(--bg-hover);
    border-left: 4px solid var(--border-hover);
    color: var(--text-primary);
  }

  .nav-item.selected {
    color: var(--accent);
    background: var(--bg-selected);
  }

  .nav-item.selected:hover {
    border-left: 4px solid var(--accent);
  }

  .nav-item.active {
    border-left: 4px solid var(--accent-active);
    background: var(--bg-selected);
  }

  .nav-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
  }
</style>
