<script lang="ts">
  /**
   * Select.svelte — dropdown drawn by the webview instead of the platform.
   *
   * A native `<select>` delegates its popup to the OS widget toolkit, and on
   * Linux/WebKitGTK that popup is a GTK menu themed from the *desktop*
   * settings, not from ours. Under the app's dark theme it lands white-on-white
   * and is unreadable. Windows and macOS happen to look fine, but this replaces
   * the control everywhere: one dropdown that reads from the same CSS custom
   * properties as the rest of the app can't drift per platform, which is the
   * failure mode a Linux-only fix would leave in place.
   *
   * Values are passed through untouched, so numeric options stay numbers —
   * a `<select>` would have stringified them.
   */
  import { onMount } from 'svelte';

  export interface SelectOption {
    value: unknown;
    label: string;
    disabled?: boolean;
  }

  interface Props {
    value: unknown;              // bindable
    options: SelectOption[];
    id?: string;
    disabled?: boolean;
    /** Shown when `value` matches no option. */
    placeholder?: string;
    ariaLabel?: string;
    /** Mirrors the native `onchange`: fires only on an actual change. */
    onchange?: (value: unknown) => void;
  }

  let {
    value = $bindable(),
    options,
    id,
    disabled = false,
    placeholder,
    ariaLabel,
    onchange,
  }: Props = $props();

  let open        = $state(false);
  let activeIndex = $state(-1);
  let popupStyle  = $state('');
  let triggerEl   = $state<HTMLButtonElement | null>(null);
  let listEl      = $state<HTMLDivElement | null>(null);

  // Unique enough to tie `aria-activedescendant` to the rendered option ids.
  const uid = `sel-${Math.random().toString(36).slice(2, 9)}`;

  let selectedIndex = $derived(options.findIndex((o) => o.value === value));
  let selectedLabel = $derived(selectedIndex >= 0 ? options[selectedIndex].label : '');

  const MAX_POPUP_HEIGHT = 260;

  function position() {
    if (!triggerEl) return;
    const r = triggerEl.getBoundingClientRect();
    // Rough height guess is enough to choose a side; the popup caps itself.
    const wanted = Math.min(MAX_POPUP_HEIGHT, options.length * 30 + 8);
    const below = window.innerHeight - r.bottom - 8;
    const above = r.top - 8;
    const flip = below < wanted && above > below;
    const maxH = Math.max(96, Math.min(MAX_POPUP_HEIGHT, flip ? above : below));

    popupStyle = flip
      ? `left:${r.left}px;width:${r.width}px;bottom:${window.innerHeight - r.top + 4}px;max-height:${maxH}px`
      : `left:${r.left}px;width:${r.width}px;top:${r.bottom + 4}px;max-height:${maxH}px`;
  }

  function openList() {
    if (disabled) return;
    activeIndex = selectedIndex >= 0 ? selectedIndex : firstEnabled();
    position();
    open = true;
  }

  function closeList(refocus = true) {
    open = false;
    if (refocus) triggerEl?.focus();
  }

  function toggle() {
    if (open) closeList();
    else openList();
  }

  function firstEnabled() {
    return options.findIndex((o) => !o.disabled);
  }

  function commit(index: number) {
    const opt = options[index];
    if (!opt || opt.disabled) return;
    const changed = opt.value !== value;
    value = opt.value;
    closeList();
    if (changed) onchange?.(opt.value);
  }

  /** Next selectable index in `dir`, skipping disabled entries. */
  function step(from: number, dir: 1 | -1) {
    let i = from;
    for (let n = 0; n < options.length; n++) {
      i += dir;
      if (i < 0) i = options.length - 1;
      if (i >= options.length) i = 0;
      if (!options[i].disabled) return i;
    }
    return from;
  }

  // ── Typeahead: jump to the next label starting with what was typed ──
  let typed = '';
  let typedAt = 0;

  function typeahead(ch: string) {
    const now = Date.now();
    typed = now - typedAt > 600 ? ch : typed + ch;
    typedAt = now;

    const from = activeIndex >= 0 ? activeIndex : 0;
    for (let n = 1; n <= options.length; n++) {
      const i = (from + n) % options.length;
      const o = options[i];
      if (!o.disabled && o.label.toLowerCase().startsWith(typed)) {
        activeIndex = i;
        if (!open) commit(i);
        return;
      }
    }
  }

  function onTriggerKey(e: KeyboardEvent) {
    if (disabled) return;
    switch (e.key) {
      case 'Enter':
      case ' ':
      case 'ArrowDown':
      case 'ArrowUp':
        e.preventDefault();
        openList();
        break;
      case 'Home':
      case 'End':
        e.preventDefault();
        commit(e.key === 'Home' ? firstEnabled() : step(0, -1));
        break;
      default:
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          typeahead(e.key.toLowerCase());
        }
    }
  }

  function onListKey(e: KeyboardEvent) {
    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        e.stopPropagation();      // don't also close a surrounding modal
        closeList();
        break;
      case 'ArrowDown':
        e.preventDefault();
        activeIndex = step(activeIndex, 1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        activeIndex = step(activeIndex, -1);
        break;
      case 'Home':
        e.preventDefault();
        activeIndex = firstEnabled();
        break;
      case 'End':
        e.preventDefault();
        activeIndex = step(0, -1);
        break;
      case 'Enter':
      case ' ':
        e.preventDefault();
        commit(activeIndex);
        break;
      case 'Tab':
        closeList(false);
        break;
      default:
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          typeahead(e.key.toLowerCase());
        }
    }
  }

  // Keep the highlighted row visible while arrowing through a long list.
  $effect(() => {
    if (!open || activeIndex < 0 || !listEl) return;
    listEl.querySelector<HTMLElement>(`#${uid}-opt-${activeIndex}`)
      ?.scrollIntoView({ block: 'nearest' });
  });

  // Focus the list on open so keys land here rather than on the trigger.
  $effect(() => {
    if (open) listEl?.focus();
  });

  /**
   * Move the popup layer to `<body>`.
   *
   * Not cosmetic. Several call sites wrap the control in a `<label>`, and a
   * label with no `for` forwards clicks from anywhere inside it to the first
   * labelable descendant — here, the trigger. Rendered in place, the popup and
   * its backdrop are *inside* that label, so choosing an option closed the list
   * and the forwarded click immediately reopened it: the value changed but the
   * dropdown never went away. Out at the body it is nobody's descendant, which
   * also puts it beyond any ancestor `overflow` or `transform` that would clip
   * or re-anchor a fixed element.
   */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  // A scrolling settings pane would otherwise leave the popup behind, since it
  // is positioned against the viewport rather than the trigger's container.
  onMount(() => {
    const reflow = () => { if (open) position(); };
    window.addEventListener('scroll', reflow, true);
    window.addEventListener('resize', reflow);
    return () => {
      window.removeEventListener('scroll', reflow, true);
      window.removeEventListener('resize', reflow);
    };
  });
</script>

<button
  bind:this={triggerEl}
  {id}
  type="button"
  class="sel-trigger"
  class:open
  {disabled}
  role="combobox"
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-controls={open ? `${uid}-list` : undefined}
  aria-label={ariaLabel}
  onclick={toggle}
  onkeydown={onTriggerKey}
>
  <span class="sel-value" class:sel-placeholder={selectedIndex < 0}>
    {selectedIndex >= 0 ? selectedLabel : (placeholder ?? '')}
  </span>
  <svg class="sel-caret" width="12" height="12" viewBox="0 0 24 24" fill="none"
       stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
    <polyline points="6 9 12 15 18 9" />
  </svg>
</button>

{#if open}
  <div class="sel-layer" use:portal>
    <div class="sel-backdrop" role="presentation" onclick={() => closeList(false)}></div>
    <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
    <div
      bind:this={listEl}
      id="{uid}-list"
      class="sel-popup"
      style={popupStyle}
      role="listbox"
      tabindex="-1"
      aria-activedescendant={activeIndex >= 0 ? `${uid}-opt-${activeIndex}` : undefined}
      onkeydown={onListKey}
    >
      {#each options as opt, i}
        <!-- Keyboard handling lives on the listbox and drives `aria-activedescendant`,
             which is the ARIA pattern for a single-select listbox: focus stays on the
             container and the options themselves are never tab stops. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          id="{uid}-opt-{i}"
          class="sel-option"
          tabindex="-1"
          class:active={i === activeIndex}
          class:selected={i === selectedIndex}
          class:disabled={opt.disabled}
          role="option"
          aria-selected={i === selectedIndex}
          aria-disabled={opt.disabled}
          onclick={() => commit(i)}
          onmouseenter={() => { if (!opt.disabled) activeIndex = i; }}
        >
          <span class="sel-option-label">{opt.label}</span>
          {#if i === selectedIndex}
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                 stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="20 6 9 17 4 12" />
            </svg>
          {/if}
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  /* Matches the .form-input / .ct-input / .ev-input look the native selects
     sat alongside, so swapping the control changes nothing visually. */
  .sel-trigger {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    font-size: 13px;
    font-family: inherit;
    text-align: left;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    outline: none;
    transition: border-color 0.15s;
  }

  .sel-trigger:hover:not(:disabled) { border-color: var(--accent-hover); }
  .sel-trigger:focus-visible,
  .sel-trigger.open                 { border-color: var(--accent-active); }

  .sel-trigger:disabled {
    cursor: default;
    opacity: 0.6;
  }

  .sel-value {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sel-placeholder { color: var(--text-tertiary); }

  .sel-caret {
    flex: 0 0 auto;
    color: var(--text-secondary);
    transition: transform 0.15s;
  }
  .sel-trigger.open .sel-caret { transform: rotate(180deg); }

  /* Portaled to <body>; a bare wrapper so it establishes no stacking context
     of its own and the children below keep their z-index against the page. */
  .sel-layer { display: contents; }

  /* Fixed, above the backdrop — so a dropdown near the bottom of a scrolling
     settings pane is not clipped by its container. */
  .sel-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1999;
  }

  .sel-popup {
    position: fixed;
    z-index: 2000;
    overflow-y: auto;
    padding: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: var(--shadow-lg, 0 8px 24px rgba(0, 0, 0, 0.18));
    outline: none;
  }

  .sel-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px;
    font-size: 13px;
    color: var(--text-primary);
    border-radius: 4px;
    cursor: pointer;
    user-select: none;
  }

  .sel-option-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sel-option svg      { flex: 0 0 auto; color: var(--accent); }
  .sel-option.active   { background: var(--bg-hover); }
  .sel-option.selected { font-weight: 600; }

  .sel-option.disabled {
    color: var(--text-tertiary);
    cursor: default;
  }
  .sel-option.disabled.active { background: none; }
</style>
