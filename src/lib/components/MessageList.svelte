<script lang="ts">
  import type { Email } from '$lib/types';
  import { formatDate } from '$lib/utils';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    emails: Email[];
    selectedEmail: Email | null;
    currentFolder: string;
    folderName: string;
    onSelectEmail: (email: Email) => void;
    onOpenDraft?: (email: Email) => void;
    onToggleStar?: (email: Email) => void;
    onTogglePin?: (email: Email) => void;
    onToggleFocused?: (email: Email) => void;
    onDeleteEmail?: (email: Email) => void;
    onClearSelection?: () => void;
    /** Fired when a row with an empty preview scrolls into the viewport.
     *  Parent uses this to fetch previews for the anchor plus nearby rows. */
    onPreviewMissing?: (email: Email) => void;
    visibleList?: Email[];
    active?: boolean;
    checkedIds?: Set<string>;
  }

  let { emails, selectedEmail, currentFolder, folderName, onSelectEmail, onOpenDraft, onToggleStar, onTogglePin, onToggleFocused, onDeleteEmail, onClearSelection, onPreviewMissing, visibleList = $bindable([]), active = false, checkedIds = $bindable(new Set<string>()) }: Props = $props();

  let activeTab = $state<'focused' | 'other'>('focused');
  let filterSort = $state<'date' | 'starred' | 'unread'>('date');
  let showFilterMenu = $state(false);
  let hoveredEmailId = $state<string | null>(null);

  let isInbox = $derived(currentFolder === 'inbox');
  let isSent = $derived(currentFolder === 'sent');

  let focusedEmails = $derived(emails.filter((e) => e.isFocused !== false));
  let otherEmails = $derived(emails.filter((e) => e.isFocused === false));
  let focusedUnread = $derived(focusedEmails.filter((e) => !e.isRead).length);
  let otherUnread = $derived(otherEmails.filter((e) => !e.isRead).length);

  // Auto-select the non-empty tab when one is empty
  $effect(() => {
    if (!isInbox) return;
    if (focusedEmails.length === 0 && otherEmails.length > 0) {
      activeTab = 'other';
    } else if (otherEmails.length === 0 && focusedEmails.length > 0) {
      activeTab = 'focused';
    }
  });

  /** Switch between Focused/Other tabs — called from global key handler */
  export function cycleInboxTab(): boolean {
    if (!isInbox) return false;
    const target = activeTab === 'focused' ? 'other' : 'focused';
    const targetHasEmails = target === 'focused' ? focusedEmails.length > 0 : otherEmails.length > 0;
    if (!targetHasEmails) return false;
    activeTab = target;
    onClearSelection?.();
    return true;
  }

  let visibleEmails = $derived.by(() => {
    let list = isInbox
      ? (activeTab === 'focused' ? focusedEmails : otherEmails)
      : emails;
    if (filterSort === 'starred') {
      list = list.filter(e => e.isStarred);
    } else if (filterSort === 'unread') {
      list = list.filter(e => !e.isRead);
    }
    list = [...list].sort((a, b) => {
      if (a.isPinned !== b.isPinned) return a.isPinned ? -1 : 1;
      return new Date(b.date).getTime() - new Date(a.date).getTime();
    });
    return list;
  });

  // ── Multi-select ──
  let multiActive = $derived(checkedIds.size > 1);
  let allChecked = $derived(visibleEmails.length > 0 && visibleEmails.every(e => checkedIds.has(e.id)));

  function toggleCheck(e: Event, emailId: string) {
    e.stopPropagation();
    const next = new Set(checkedIds);
    if (next.has(emailId)) next.delete(emailId);
    else next.add(emailId);
    checkedIds = next;
  }

  function toggleAll() {
    if (allChecked) {
      checkedIds = new Set();
    } else {
      checkedIds = new Set(visibleEmails.map(e => e.id));
    }
  }

  export function clearChecked() {
    checkedIds = new Set();
  }

  // Clear reading pane if selected email isn't in the current visible list
  $effect(() => {
    if (selectedEmail && !visibleEmails.some((e) => e.id === selectedEmail!.id)) {
      onClearSelection?.();
    }
  });

  // Expose the visible (sorted+filtered) list to parent for keyboard navigation
  $effect(() => { visibleList = visibleEmails; });

  function handleAction(e: MouseEvent, action: () => void) {
    e.stopPropagation();
    action();
  }

  // Scroll selected email into view when selection changes via keyboard
  let emailItemsEl: HTMLDivElement | undefined = $state();
  $effect(() => {
    if (!selectedEmail || !emailItemsEl) return;
    const el = emailItemsEl.querySelector('.email-item.selected') as HTMLElement | null;
    if (el) el.scrollIntoView({ block: 'nearest' });
  });

  // ── Viewport-triggered preview prefetch ──
  // When a row with no preview becomes visible, notify the parent with that
  // row as the anchor. The parent batches one IMAP round trip covering the
  // anchor plus a window around it, so scrolling slowly loads in groups and
  // a fast Ctrl+End / jump keeps the next visible rows primed too.
  const firedAnchors = new Set<string>(); // anchors already requested this session
  let pendingAnchor: Email | null = null;
  let pendingTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleFetch(email: Email) {
    if (firedAnchors.has(email.id)) return;
    pendingAnchor = email;
    if (pendingTimer) return;
    pendingTimer = setTimeout(() => {
      pendingTimer = null;
      const target = pendingAnchor;
      pendingAnchor = null;
      if (!target || firedAnchors.has(target.id)) return;
      firedAnchors.add(target.id);
      onPreviewMissing?.(target);
    }, 150);
  }

  /** Svelte action: observe a row and trigger a prefetch when it enters view. */
  function observePreview(node: HTMLElement, email: Email) {
    let observer: IntersectionObserver | null = null;
    let currentEmail = email;

    const setup = (e: Email) => {
      observer?.disconnect();
      observer = null;
      // Only observe rows that actually need a preview.
      if (e.preview && e.preview.length > 0) return;
      if (firedAnchors.has(e.id)) return;
      if (!('IntersectionObserver' in window)) {
        scheduleFetch(e);
        return;
      }
      observer = new IntersectionObserver((entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            scheduleFetch(currentEmail);
            observer?.disconnect();
            observer = null;
            break;
          }
        }
      }, { root: emailItemsEl ?? null, rootMargin: '200px 0px', threshold: 0.01 });
      observer.observe(node);
    };

    setup(currentEmail);

    return {
      update(next: Email) {
        const changed = next.id !== currentEmail.id || (!currentEmail.preview && !!next.preview);
        currentEmail = next;
        if (changed) setup(next);
      },
      destroy() {
        observer?.disconnect();
      },
    };
  }
</script>

<svelte:window onclick={() => { showFilterMenu = false; }} />

<section class="message-list">
  <!-- Focused / Other Tabs + Filter -->
  <div class="tab-bar">
    {#if multiActive}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <label class="select-all-check" onclick={(e) => e.stopPropagation()}>
        <input type="checkbox" checked={allChecked} onchange={toggleAll} />
        <span class="check-count">{t('messageList.selectedCount', { count: checkedIds.size })}</span>
      </label>
    {:else if isInbox}
    <div class="tab-button">
      <button
        class="tab"
        class:active={activeTab === 'focused'}
        onclick={() => { activeTab = 'focused'; if (selectedEmail && selectedEmail.isFocused === false) onClearSelection?.(); }}
      >
        {t('messageList.priority')}
      </button>
      {#if focusedUnread > 0}
          {#if activeTab == 'focused'}
          <span class="tab-badge active">{focusedUnread}</span>
          {:else}
          <span class="tab-badge">{focusedUnread}</span>
          {/if}
      {/if}
    </div>
    <div class="tab-button">
      <button
        class="tab"
        class:active={activeTab === 'other'}
        onclick={() => { activeTab = 'other'; if (selectedEmail && (selectedEmail.isFocused ?? true) !== false) onClearSelection?.(); }}
      >
        {t('messageList.regular')}
      </button>
      {#if otherUnread > 0}
        {#if activeTab == 'other'}
        <span class="tab-badge active">{otherUnread}</span>
        {:else}
        <span class="tab-badge">{otherUnread}</span>
        {/if}
      {/if}
    </div>
    {:else}
      <span class="folder-title">{folderName}</span>
    {/if}
    <div class="tab-spacer"></div>
    <div class="filter-wrapper">
      <button class="filter-btn" class:filter-active={filterSort !== 'date'} data-tooltip="Filter" aria-label="Filter" onclick={(e) => { e.stopPropagation(); showFilterMenu = !showFilterMenu; }}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="1" /><circle cx="12" cy="5" r="1" /><circle cx="12" cy="19" r="1" />
        </svg>
      </button>
      {#if showFilterMenu}
        <div class="filter-menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
          <button class="filter-option" class:active={filterSort === 'date'} onclick={() => { filterSort = 'date'; showFilterMenu = false; }}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3" y="4" width="18" height="18" rx="2" /><line x1="16" y1="2" x2="16" y2="6" /><line x1="8" y1="2" x2="8" y2="6" /><line x1="3" y1="10" x2="21" y2="10" />
            </svg>
            {t('messageList.allByDate')}
          </button>
          <button class="filter-option" class:active={filterSort === 'starred'} onclick={() => { filterSort = 'starred'; showFilterMenu = false; }}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
            </svg>
            {t('messageList.starredOnly')}
          </button>
          <button class="filter-option" class:active={filterSort === 'unread'} onclick={() => { filterSort = 'unread'; showFilterMenu = false; }}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2" y="4" width="20" height="16" rx="2" /><circle cx="18" cy="6" r="4" fill="currentColor" stroke="none" />
            </svg>
            {t('messageList.unreadOnly')}
          </button>
        </div>
      {/if}
    </div>
  </div>

  <!-- Email Items -->
  <div class="email-items" class:active bind:this={emailItemsEl}>
    {#each visibleEmails as email (email.id)}
      <div
        use:observePreview={email}
        class="email-item"
        class:selected={selectedEmail?.id === email.id}
        class:unread={!email.isRead}
        class:pinned={email.isPinned}
        role="button"
        tabindex="0"
        onclick={() => onSelectEmail(email)}
        ondblclick={() => { if (email.folder === 'drafts') onOpenDraft?.(email); }}
        onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectEmail(email); } }}
        onmouseenter={() => (hoveredEmailId = email.id)}
        onmouseleave={() => (hoveredEmailId = null)}
      >
        <!-- Avatar / Checkbox (hidden for Sent folder) -->
        {#if !isSent}
          {#if multiActive}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <label class="avatar-checkbox" onclick={(e) => e.stopPropagation()}>
              <input type="checkbox" checked={checkedIds.has(email.id)} onchange={(e) => toggleCheck(e, email.id)} />
            </label>
          {:else}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <span class="avatar-wrapper" role="button" tabindex="-1" onclick={(e) => toggleCheck(e, email.id)}>
              <span class="avatar-face" style="background-color: {email.from.color}">
                {#if email.from.photoUrl}
                  <img class="avatar-img" src={email.from.photoUrl} alt={email.from.name} />
                {:else}
                  {email.from.initials}
                {/if}
              </span>
              <span class="avatar-check">
                <input type="checkbox" checked={checkedIds.has(email.id)} tabindex="-1" onclick={(e) => e.stopPropagation()} onchange={(e) => toggleCheck(e, email.id)} />
              </span>
            </span>
          {/if}
        {/if}

        <!-- Content -->
        <div class="email-content">
          <div class="email-top-row">
            {#if email.folder === 'drafts'}
              <span class="sender-name draft-sender">
                <span class="draft-tag">[Draft]</span>
                {#if email.to.length > 0}
                  {email.to.map(c => c.name || c.email).join(', ')}
                {:else}
                  (No recipients)
                {/if}
              </span>
            {:else if isSent}
              <span class="sender-name">
                {#if email.to.length > 0}
                  {email.to.map(c => c.name || c.email).join(', ')}
                {:else}
                  (No recipients)
                {/if}
              </span>
            {:else}
              <span class="sender-name">{email.from.name || email.from.email}</span>
            {/if}
            <div class="hover-actions">
                <button class="hover-btn" tabindex="-1" class:active={email.isStarred} aria-label="Star" data-tooltip={email.isStarred ? t('messageList.unstar') : t('messageList.star')} data-tooltip-position="bottom" onclick={(e) => handleAction(e, () => onToggleStar?.(email))}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill={email.isStarred ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="1.5">
                    <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
                  </svg>
                </button>
                <button class="hover-btn" tabindex="-1" class:active={email.isPinned} aria-label="Pin" data-tooltip={email.isPinned ? t('messageList.unpin') : t('messageList.pin')} data-tooltip-position="bottom" onclick={(e) => handleAction(e, () => onTogglePin?.(email))}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill='none' stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m16.243 2.932l4.825 4.826a2.75 2.75 0 0 1-.715 4.404l-4.87 2.435a.75.75 0 0 0-.374.426l-1.44 4.166a1.25 1.25 0 0 1-2.065.476L8.5 16.561L4.06 21H3v-1.062L7.44 15.5l-3.105-3.104a1.25 1.25 0 0 1 .476-2.066l4.166-1.439a.75.75 0 0 0 .426-.374l2.435-4.87a2.75 2.75 0 0 1 4.405-.715m3.765 5.886l-4.826-4.825a1.25 1.25 0 0 0-2.002.324l-2.435 4.871a2.25 2.25 0 0 1-1.278 1.12l-3.789 1.31l6.705 6.704l1.308-3.788a2.25 2.25 0 0 1 1.12-1.278l4.872-2.436a1.25 1.25 0 0 0 .325-2.002"/>
                  </svg>
                </button>
                {#if hoveredEmailId === email.id}
                  {#if isInbox}
                    <button class="hover-btn" tabindex="-1" aria-label="Focus" data-tooltip={(email.isFocused ?? true) ? t('messageList.moveToRegular') : t('messageList.moveToPriority')} data-tooltip-position="bottom" onclick={(e) => handleAction(e, () => onToggleFocused?.(email))}>
                      {#if email.isFocused ?? true}
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                          <circle cx="12" cy="12" r="10"/><line x1="8" y1="12" x2="16" y2="12"/>
                        </svg>
                      {:else}
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                          <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="16"/><line x1="8" y1="12" x2="16" y2="12"/>
                        </svg>
                      {/if}
                    </button>
                  {/if}
                  <button class="hover-btn delete-btn" tabindex="-1" aria-label="Delete" data-tooltip={t('common.delete')} data-tooltip-position="bottom" onclick={(e) => handleAction(e, () => onDeleteEmail?.(email))}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="3 6 5 6 21 6" /><path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6" /><path d="M10 11v6" /><path d="M14 11v6" /><path d="M9 6V4a1 1 0 011-1h4a1 1 0 011 1v2" />
                    </svg>
                  </button>
                {/if}
              </div>
          </div>
          <div class="email-subject-row">
            <span class="email-subject">{email.subject || t('messageList.noSubject')}</span>
            <span class="email-date">{formatDate(email.date)}</span>
            <svg class="indicator-icon" style="visibility: {email.isReplied? 'visible' : 'hidden'}" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="9 17 4 12 9 7" />
              <path d="M20 18v-2a4 4 0 00-4-4H4" />
            </svg>
          </div>
          <div class="email-preview-row">
            <div class="email-preview">{email.preview}</div>
            <svg class="indicator-icon" style="visibility: {email.hasAttachment? 'visible' : 'hidden'}" width="14" height="14" viewBox="0 0 16 16">
              <path fill="currentColor" d="M2.283 7.975a.5.5 0 0 0 .854.354l4.595-4.597a2.5 2.5 0 1 1 3.536 3.536l-5.303 5.303a1 1 0 0 1-1.414-1.414l5.303-5.303a.5.5 0 0 0-.708-.708L3.843 10.45a2 2 0 1 0 2.828 2.828l5.303-5.303a3.5 3.5 0 1 0-4.95-4.95L2.43 7.621a.5.5 0 0 0-.146.354"/>
            </svg>
          </div>
        </div>
      </div>
    {/each}

    {#if visibleEmails.length === 0}
      <div class="empty-state">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--text-tertiary)" stroke-width="1" stroke-linecap="round" stroke-linejoin="round">
          <rect x="2" y="4" width="20" height="16" rx="2" />
          <path d="M2 7l10 6 10-6" />
        </svg>
        <p>{filterSort === 'starred' ? t('messageList.noStarred') : filterSort === 'unread' ? t('messageList.noUnread') : t('messageList.noMessages')}</p>
      </div>
    {/if}
  </div>
</section>

<style>
  .message-list {
    width: 30%;
    height: 100%;
    min-width: 350px;
    max-width: 550px;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border-light);
    border-radius: 4px;
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    overflow: hidden;
  }

  /* ── Tab Bar ── */
  .tab-bar {
    display: flex;
    align-items: center;
    padding: 0 12px;
    margin-top: 0;
    border-bottom: 1px solid var(--border-light);
    gap: 0;
  }

  .tab {
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    position: relative;
    margin-bottom: 6px;
    transition: color 0.12s;
    outline: none;
  }

  .tab:hover {
    color: var(--text-primary);
  }

  .tab.active {
    color: var(--text-primary);
    font-weight: 600;
  }

  .tab.active::after {
    content: '';
    position: absolute;
    bottom: -1px;
    left: 20px;
    right: 20px;
    height: 2px;
    background: var(--accent-active);
  }

  .tab-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--text-on-accent);
    font-size: 10px;
    font-weight: 700;
    margin-left: -14px;
    opacity: 0.6;
  }

  .tab-badge.active {
    opacity: 1;
  }

  .folder-title {
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .tab-spacer {
    flex: 1;
  }

  /* ── Filter ── */
  .filter-wrapper {
    position: relative;
  }

  .filter-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 4px;
    color: var(--text-secondary);
    transition: background 0.1s, color 0.1s;
  }

  .filter-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .filter-btn.filter-active {
    color: var(--accent);
  }

  .filter-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    box-shadow: var(--shadow-md, 0 8px 32px rgba(0, 0, 0, 0.18));
    z-index: 100;
    min-width: 160px;
  }

  .filter-option {
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

  .filter-option:hover {
    background: var(--bg-hover);
  }

  .filter-option.active {
    color: var(--accent);
    font-weight: 600;
  }

  /* ── Email Items ── */
  .email-items {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }

  .email-item {
    display: flex;
    align-items: flex-start;
    gap: 0;
    width: 100%;
    padding: 5px 5px 5px 15px;
    text-align: left;
    border-radius: 0;
    transition: background 0.1s ease;
    position: relative;
    border-left: 4px solid transparent;
    border-bottom: 1px solid var(--border-light);
    min-height: 72px;
    cursor: pointer;
    user-select: none;
    /* Native off-screen rendering skip — keeps thousands-of-item lists snappy */
    content-visibility: auto;
    contain-intrinsic-size: auto 72px;
  }

  .email-item:focus {
    outline: none;
  }

  .email-item:hover {
    background: var(--bg-hover);
    border-left-color: var(--border-hover);
  }

  .email-item.selected {
    background: var(--bg-selected);
  }

  .email-item.selected:hover {
    border-left-color: var(--accent);
  }

  .email-item.selected:hover .sender-name {
    color: var(--text-secondary);
  }

  .email-items.active .email-item.selected,
  .email-items.active .email-item.selected:hover {
    border-left-color: var(--accent-active);
  }

  .email-item.unread:not(:hover) {
    border-left-color: var(--accent);
  }

  /* ── Avatar / Checkbox ── */
  .avatar-wrapper {
    position: relative;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    margin-right: 10px;
    margin-top: 2px;
    cursor: pointer;
    outline: none;
  }

  .avatar-face {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: white;
    overflow: hidden;
  }

  .avatar-wrapper:hover .avatar-face,
  .avatar-wrapper:has(input:checked) .avatar-face {
    visibility: hidden;
  }

  .avatar-check {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 0.1s;
    border-radius: 50%;
  }

  /* Shared outline-only checkbox style */
  .avatar-check input[type="checkbox"],
  .avatar-checkbox input[type="checkbox"],
  .select-all-check input[type="checkbox"] {
    -webkit-appearance: none;
    appearance: none;
    width: 16px;
    height: 16px;
    border: 1.5px solid var(--text-tertiary);
    border-radius: 3px;
    background: transparent;
    cursor: pointer;
    position: relative;
    transition: border-color 0.1s;
    flex-shrink: 0;
  }

  .avatar-check input[type="checkbox"]:hover,
  .avatar-checkbox input[type="checkbox"]:hover,
  .select-all-check input[type="checkbox"]:hover {
    border-color: var(--text-secondary);
  }

  .avatar-check input[type="checkbox"]:checked,
  .avatar-checkbox input[type="checkbox"]:checked,
  .select-all-check input[type="checkbox"]:checked {
    border-color: var(--text-secondary);
  }

  .avatar-check input[type="checkbox"]:checked::after,
  .avatar-checkbox input[type="checkbox"]:checked::after,
  .select-all-check input[type="checkbox"]:checked::after {
    content: '';
    position: absolute;
    left: 4px;
    top: 1px;
    width: 5px;
    height: 9px;
    border: solid var(--text-secondary);
    border-width: 0 2px 2px 0;
    transform: rotate(45deg);
  }

  .avatar-wrapper:hover .avatar-check,
  .avatar-wrapper .avatar-check:has(input:checked) {
    opacity: 1;
  }

  .avatar-checkbox {
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    margin-right: 10px;
    margin-top: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }

  .avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
  }

  /* ── Select All (header) ── */
  .select-all-check {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .check-count {
    white-space: nowrap;
  }

  /* ── Content ── */
  .email-content {
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .email-top-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 1px;
  }

  .sender-name {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .unread .sender-name {
    font-weight: 600;
  }

  .unread .email-subject, 
  .unread .email-date {
    color: var(--accent-active);
    font-weight: 600;
  }

  .draft-sender {
    display: flex;
    align-items: baseline;
    gap: 4px;
    min-width: 0;
  }

  .draft-tag {
    color: #c50f1f;
    font-weight: 600;
    font-size: 12px;
    flex-shrink: 0;
  }

  :global([data-theme="dark"]) .draft-tag {
    color: #f87171;
  }

  /* ── Hover Actions ── */
  .hover-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }

  .hover-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    color: var(--text-tertiary);
    opacity: 0;
    pointer-events: none;
    transition: background 0.1s, color 0.1s, opacity 0.1s;
  }

  .hover-btn.active {
    opacity: 1;
    pointer-events: auto;
    color: var(--accent-active);
  }

  .email-item:hover .hover-btn {
    opacity: 1;
    pointer-events: auto;
  }

  .hover-btn:hover {
    color: var(--text-primary);
  }

  .hover-btn.delete-btn:hover {
    color: #c50f1f;
  }

  :global([data-theme="dark"]) .hover-btn.delete-btn:hover {
    color: #f87171;
  }

  /* ── Subject Row (subject + date) ── */
  .email-subject-row, .email-preview-row {
    display: flex;
    align-items:center;
    justify-items: stretch;
    gap: 8px;
    margin-bottom: 1px;
  }

  .email-subject {
    font-size: 12.5px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .unread .email-subject {
    font-weight: 600;
  }

  .email-date {
    font-size: 11px;
    color: var(--text-tertiary);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .email-preview {
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  /* ── Indicators ── */

  .indicator-icon {
    color: var(--text-tertiary);
  }

  /* ── Empty State ── */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 24px;
    color: var(--text-tertiary);
    font-size: 13px;
  }
</style>
