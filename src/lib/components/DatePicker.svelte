<script lang="ts">
  import { getMonthGrid, isSameDay } from '$lib/utils';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    value: string;      // YYYY-MM-DD, bindable
    id?: string;
    placeholder?: string;
  }

  let { value = $bindable(''), id, placeholder }: Props = $props();

  const today = new Date();

  let open      = $state(false);
  let viewMonth = $state(today.getMonth());
  let viewYear  = $state(today.getFullYear());
  let popupStyle = $state('');
  let triggerEl  = $state<HTMLButtonElement | null>(null);

  // Parse value to a local-midnight Date so there are no UTC-offset surprises
  let selected = $derived(value ? new Date(value + 'T12:00:00') : null);
  let grid     = $derived(getMonthGrid(viewYear, viewMonth));

  function displayValue(d: Date | null): string {
    if (!d) return '';
    const month = t(`datePicker.month${d.getMonth()}`);
    return `${month} ${d.getDate()}, ${d.getFullYear()}`;
  }

  function toggle() {
    if (open) { open = false; return; }
    if (selected) {
      viewMonth = selected.getMonth();
      viewYear  = selected.getFullYear();
    }
    if (triggerEl) {
      const r = triggerEl.getBoundingClientRect();
      popupStyle = `top:${r.bottom + 4}px;left:${r.left}px;width:228px`;
    }
    open = true;
  }

  function pick(day: Date) {
    const y = day.getFullYear();
    const m = String(day.getMonth() + 1).padStart(2, '0');
    const d = String(day.getDate()).padStart(2, '0');
    value = `${y}-${m}-${d}`;
    open = false;
  }

  function prevMonth() {
    if (viewMonth === 0) { viewMonth = 11; viewYear--; } else viewMonth--;
  }
  function nextMonth() {
    if (viewMonth === 11) { viewMonth = 0; viewYear++; } else viewMonth++;
  }
</script>

<div class="dp">
  <button
    bind:this={triggerEl}
    type="button"
    {id}
    class="dp-trigger"
    onclick={toggle}
    aria-haspopup="true"
    aria-expanded={open}
  >
    <span class:dp-placeholder={!selected}>{selected ? displayValue(selected) : (placeholder ?? t('datePicker.selectDate'))}</span>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <rect x="3" y="4" width="18" height="18" rx="2"/><line x1="16" y1="2" x2="16" y2="6"/>
      <line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/>
    </svg>
  </button>

  {#if open}
    <div class="dp-backdrop" onclick={() => (open = false)} role="presentation"></div>
    <div class="dp-popup" style={popupStyle} role="dialog" aria-label={t('datePicker.datePicker')}>
      <div class="dp-header">
        <button type="button" class="dp-nav" onclick={prevMonth} aria-label={t('datePicker.previousMonth')}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
        </button>
        <span class="dp-month">{t(`datePicker.month${viewMonth}`)} {viewYear}</span>
        <button type="button" class="dp-nav" onclick={nextMonth} aria-label={t('datePicker.nextMonth')}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 6 15 12 9 18"/></svg>
        </button>
      </div>
      <div class="dp-weekdays">
        {#each [0,1,2,3,4,5,6] as d}<span>{t(`datePicker.weekday${d}`)}</span>{/each}
      </div>
      <div class="dp-grid">
        {#each grid as day}
          <button
            type="button"
            class="dp-cell"
            class:other={day.getMonth() !== viewMonth}
            class:today={isSameDay(day, today)}
            class:sel={!!selected && isSameDay(day, selected)}
            onclick={() => pick(day)}
          >{day.getDate()}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .dp { position: relative; width: 100%; }

  .dp-trigger {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
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
  .dp-trigger:hover  { border-color: var(--accent); }
  .dp-trigger:focus  { outline: none; border-color: var(--accent-active); }
  .dp-trigger svg    { color: var(--text-secondary); flex-shrink: 0; }
  .dp-placeholder    { color: var(--text-tertiary); }

  .dp-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1999;
  }

  .dp-popup {
    position: fixed;
    z-index: 2000;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow-lg);
    padding: 12px;
    min-width: 228px;
  }

  .dp-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .dp-month { font-size: 13px; font-weight: 600; color: var(--text-primary); }

  .dp-nav {
    width: 24px; height: 24px;
    display: flex; align-items: center; justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .dp-nav:hover { background: var(--bg-hover); }

  .dp-weekdays {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    text-align: center;
    margin-bottom: 2px;
  }
  .dp-weekdays span {
    font-size: 10px;
    color: var(--text-secondary);
    padding: 2px 0;
  }

  .dp-grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
  }

  .dp-cell {
    height: 28px;
    display: flex; align-items: center; justify-content: center;
    font-size: 11px;
    color: var(--text-primary);
    cursor: pointer;
    border-radius: 50%;
    position: relative;
    isolation: isolate;
    font-family: inherit;
  }
  .dp-cell:hover   { background: var(--bg-hover); }
  .dp-cell.other   { color: var(--text-tertiary); }

  .dp-cell.today {
    color: var(--text-on-accent);
    font-weight: 600;
  }
  .dp-cell.today::before {
    content: '';
    position: absolute;
    inset: 2px;
    border-radius: 50%;
    background: var(--accent);
    z-index: -1;
  }
  .dp-cell.sel:not(.today) {
    background: var(--bg-selected);
    font-weight: 600;
  }
</style>
