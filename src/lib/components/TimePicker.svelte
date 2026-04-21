<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    value: string;   // HH:MM (24-hour), bindable
    id?: string;
  }

  let { value = $bindable('09:00'), id }: Props = $props();

  // 30-minute slots across 24 hours
  const TIMES: string[] = [];
  for (let h = 0; h < 24; h++) {
    for (const m of [0, 30]) {
      TIMES.push(`${String(h).padStart(2,'0')}:${String(m).padStart(2,'0')}`);
    }
  }

  let open      = $state(false);
  let popupStyle = $state('');
  let triggerEl  = $state<HTMLButtonElement | null>(null);
  let listEl     = $state<HTMLDivElement | null>(null);

  function fmt(v: string): string {
    if (!v) return '';
    const [h, m] = v.split(':').map(Number);
    const ampm = h >= 12 ? t('timePicker.pm') : t('timePicker.am');
    const h12  = h % 12 || 12;
    return `${h12}:${String(m).padStart(2,'0')} ${ampm}`;
  }

  function toggle() {
    if (open) { open = false; return; }
    if (triggerEl) {
      const r = triggerEl.getBoundingClientRect();
      const spaceBelow = window.innerHeight - r.bottom;
      if (spaceBelow >= 212 || spaceBelow >= r.top) {
        popupStyle = `top:${r.bottom + 4}px;left:${r.left}px;width:${r.width}px`;
      } else {
        // Not enough room below — open upward
        popupStyle = `bottom:${window.innerHeight - r.top + 4}px;left:${r.left}px;width:${r.width}px`;
      }
    }
    open = true;
    requestAnimationFrame(() => {
      if (!listEl || !value) return;
      const idx = TIMES.findIndex((tm) => tm === value);
      if (idx >= 0) (listEl.children[idx] as HTMLElement)?.scrollIntoView({ block: 'center' });
    });
  }

  function pick(time: string) {
    value = time;
    open = false;
  }
</script>

<div class="tp">
  <button
    bind:this={triggerEl}
    type="button"
    {id}
    class="tp-trigger"
    onclick={toggle}
    aria-haspopup="listbox"
    aria-expanded={open}
  >
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>
    </svg>
    <span>{fmt(value) || value || '—'}</span>
  </button>

  {#if open}
    <div class="tp-backdrop" onclick={() => (open = false)} role="presentation"></div>
    <div class="tp-popup" style={popupStyle} role="listbox" aria-label={t('timePicker.selectTime')} bind:this={listEl}>
      {#each TIMES as time}
        <button
          type="button"
          class="tp-option"
          class:active={time === value}
          role="option"
          aria-selected={time === value}
          onclick={() => pick(time)}
        >{fmt(time)}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tp { position: relative; width: 100%; }

  .tp-trigger {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 6px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.15s;
  }
  .tp-trigger:hover { border-color: var(--accent); }
  .tp-trigger:focus { outline: none; border-color: var(--accent-active); }
  .tp-trigger svg   { color: var(--text-secondary); flex-shrink: 0; }

  .tp-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1999;
  }

  .tp-popup {
    position: fixed;
    z-index: 2000;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: var(--shadow-lg);
    overflow-y: auto;
    max-height: 208px;
    padding: 4px;
  }

  .tp-option {
    width: 100%;
    display: block;
    padding: 5px 10px;
    font-size: 13px;
    font-family: inherit;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
    border-radius: 4px;
    background: none;
    border: none;
  }
  .tp-option:hover  { background: var(--bg-hover); }
  .tp-option.active { background: var(--bg-selected); color: var(--accent); font-weight: 600; }
</style>
