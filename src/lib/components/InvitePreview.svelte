<script lang="ts">
  /**
   * InvitePreview.svelte — read a meeting invitation before importing it.
   *
   * An .ics attachment is useless on a desktop with no calendar handler
   * registered: "Open" hands it to nothing. This shows what the invitation
   * actually says and leaves the decision to the user — import it into a
   * calendar, or just save the file.
   */
  import type { CalendarCategory, CalendarInvite } from '$lib/types';
  import Select from './Select.svelte';
  import { formatTimeRange } from '$lib/utils';
  import { locale, t } from '$lib/i18n/index.svelte';

  interface Props {
    invite: CalendarInvite;
    /** Calendars the event can be imported into; the first is preselected. */
    categories: CalendarCategory[];
    onImport: (calendarId: string) => void;
    /** RSVP to the organiser. Accepting and tentatively accepting also file
     *  the event in `calendarId`; declining does not. */
    onRespond: (response: 'accepted' | 'declined' | 'tentative', calendarId: string) => void;
    onSaveAs: () => void;
    onClose: () => void;
    /** A reply is in flight — the buttons stay disabled until it lands. */
    sending?: boolean;
  }

  let { invite, categories, onImport, onRespond, onSaveAs, onClose, sending = false }: Props = $props();

  // An RSVP needs a meeting request and someone to send the answer to.
  let canRespond = $derived(invite.method === 'REQUEST' && !!invite.organizer);

  let overlayEl: HTMLDivElement | undefined = $state();
  let calendarId = $state('');

  // Preselect the first writable calendar. `untrack` isn't needed: the list
  // is static for the life of the dialog.
  $effect(() => {
    if (!calendarId) calendarId = categories[0]?.id ?? '';
  });

  $effect(() => { overlayEl?.focus(); });

  let calendarOptions = $derived(categories.map((c) => ({ value: c.id, label: c.name })));

  let start = $derived(new Date(invite.start));
  let end = $derived(new Date(invite.end));
  let datesValid = $derived(!isNaN(start.getTime()) && !isNaN(end.getTime()));

  /** "Tuesday, 16 September 2026, 2:00 PM – 3:00 PM" — or just the day for all-day events. */
  let whenLabel = $derived.by(() => {
    if (!datesValid) return invite.start;
    const loc = locale();
    const day = start.toLocaleDateString(loc, {
      weekday: 'long', year: 'numeric', month: 'long', day: 'numeric',
    });
    if (invite.isAllDay) return `${day} · ${t('invite.allDay')}`;
    // A meeting that runs past midnight needs the end date spelled out too.
    if (start.toDateString() !== end.toDateString()) {
      const endDay = end.toLocaleDateString(loc, { month: 'long', day: 'numeric' });
      return `${day}, ${start.toLocaleTimeString(loc, { hour: 'numeric', minute: '2-digit' })} – ${endDay}, ${end.toLocaleTimeString(loc, { hour: 'numeric', minute: '2-digit' })}`;
    }
    return `${day}, ${formatTimeRange(start, end, loc)}`;
  });

  let repeatLabel = $derived.by(() => {
    const r = invite.recurrence;
    if (!r) return '';
    const every = r.interval > 1 ? t('invite.repeatsEvery', { interval: r.interval, freq: t(`invite.freq.${r.freq}`) })
                                 : t(`invite.repeats.${r.freq}`);
    return r.endDate ? `${every} · ${t('invite.until', { date: r.endDate })}` : every;
  });

  /** The METHOD tells the user whether this adds, changes, or cancels. */
  let methodLabel = $derived.by(() => {
    switch (invite.method) {
      case 'REQUEST': return t('invite.methodRequest');
      case 'CANCEL': return t('invite.methodCancel');
      case 'REPLY': return t('invite.methodReply');
      default: return t('invite.methodEvent');
    }
  });

  function handleKeydown(ev: KeyboardEvent) {
    if (ev.key === 'Escape') onClose();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="modal-overlay"
  role="dialog"
  aria-modal="true"
  aria-label={invite.title || methodLabel}
  tabindex="-1"
  bind:this={overlayEl}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal-backdrop" onclick={onClose} onkeydown={handleKeydown}></div>
  <div class="modal-card">
    <div class="invite-head">
      <span class="invite-method" class:cancel={invite.method === 'CANCEL'}>{methodLabel}</span>
      <span class="invite-file">{invite.filename}</span>
    </div>

    <h3 class="invite-title">{invite.title || t('invite.untitled')}</h3>

    <dl class="invite-facts">
      <dt>{t('invite.when')}</dt>
      <dd>
        {whenLabel}
        {#if repeatLabel}<span class="invite-repeat">{repeatLabel}</span>{/if}
        <!-- The organiser's times were converted to this machine's zone.
             Naming their zone makes that visible, so a reader who compares
             the invitation with the email text isn't left guessing. -->
        {#if invite.sourceTzid && !invite.isAllDay}
          <span class="invite-repeat">{t('invite.zoneNote', { zone: invite.sourceTzid })}</span>
        {/if}
      </dd>

      {#if invite.location}
        <dt>{t('invite.where')}</dt>
        <dd>{invite.location}</dd>
      {/if}

      {#if invite.organizer}
        <dt>{t('invite.organizer')}</dt>
        <dd>
          {invite.organizer.name}
          {#if invite.organizer.name !== invite.organizer.email}
            <span class="invite-email">&lt;{invite.organizer.email}&gt;</span>
          {/if}
        </dd>
      {/if}

      {#if invite.attendees.length > 0}
        <dt>{t('invite.attendees')}</dt>
        <dd>
          <div class="invite-attendees">
            {#each invite.attendees as a (a.email)}
              <span class="invite-attendee" title={a.email}>
                <span class="invite-avatar" style="background:{a.color}">{a.initials}</span>
                {a.name}{#if a.role === 'optional'}<span class="invite-optional">{t('invite.optional')}</span>{/if}
              </span>
            {/each}
          </div>
        </dd>
      {/if}
    </dl>

    {#if invite.description}
      <div class="invite-description">{invite.description}</div>
    {/if}

    <p class="invite-note">
      {#if canRespond}
        {t('invite.rsvpNote', { organizer: invite.organizer?.email ?? '' })}
      {:else}
        {t('invite.importNote')}
      {/if}
    </p>

    <div class="modal-actions">
      <label class="invite-calendar">
        <span>{t('invite.addTo')}</span>
        <Select
          bind:value={calendarId}
          options={calendarOptions}
          disabled={calendarOptions.length === 0}
          placeholder={t('invite.noCalendars')}
          ariaLabel={t('invite.addTo')}
        />
      </label>
      <div class="modal-buttons">
        <button class="modal-btn modal-btn-cancel" onclick={onSaveAs}>{t('readingPane.attachSaveAs')}</button>
        <button class="modal-btn modal-btn-cancel" onclick={onClose}>{t('invite.close')}</button>
        {#if canRespond}
          <button
            class="modal-btn modal-btn-cancel"
            onclick={() => onRespond('declined', calendarId)}
            disabled={sending}
          >
            {t('invite.decline')}
          </button>
          <button
            class="modal-btn modal-btn-cancel"
            onclick={() => onRespond('tentative', calendarId)}
            disabled={sending || !datesValid}
          >
            {t('invite.tentative')}
          </button>
          <button
            class="modal-btn modal-btn-confirm"
            onclick={() => onRespond('accepted', calendarId)}
            disabled={sending || !datesValid}
          >
            {sending ? t('invite.sending') : t('invite.accept')}
          </button>
        {:else}
          <button
            class="modal-btn modal-btn-confirm"
            onclick={() => onImport(calendarId)}
            disabled={!calendarId || !datesValid}
          >
            {t('invite.import')}
          </button>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2000;
  }

  .modal-backdrop {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
  }

  .modal-card {
    position: relative;
    background: var(--bg-primary);
    border: 1px solid var(--border-light);
    border-radius: 8px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
    padding: 18px 22px 16px;
    min-width: 400px;
    max-width: 560px;
    width: 90%;
    max-height: 80vh;
    overflow-y: auto;
  }

  .invite-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .invite-method {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    padding: 2px 7px;
    border-radius: 3px;
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .invite-method.cancel {
    background: var(--danger);
  }

  .invite-file {
    font-size: 11px;
    color: var(--text-tertiary, var(--text-secondary));
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .invite-title {
    margin: 0 0 14px 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.3;
  }

  .invite-facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 14px;
    margin: 0 0 14px 0;
    font-size: 13px;
  }

  .invite-facts dt {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .invite-facts dd {
    margin: 0;
    color: var(--text-primary);
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .invite-repeat {
    display: block;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .invite-email {
    color: var(--text-secondary);
  }

  .invite-attendees {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .invite-attendee {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px 2px 2px;
    border-radius: 11px;
    background: var(--bg-secondary);
    font-size: 12px;
  }

  .invite-avatar {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    color: #fff;
    font-size: 9px;
    font-weight: 600;
  }

  .invite-optional {
    margin-left: 4px;
    color: var(--text-secondary);
    font-size: 11px;
  }

  .invite-description {
    max-height: 180px;
    overflow-y: auto;
    padding: 10px 12px;
    margin-bottom: 16px;
    border: 1px solid var(--border-light);
    border-radius: 4px;
    background: var(--bg-secondary);
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-primary);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .invite-note {
    margin: 0 0 12px 0;
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.5;
  }

  .modal-actions {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .invite-calendar {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-secondary);
    min-width: 180px;
  }

  .modal-buttons {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }

  .modal-btn {
    padding: 6px 16px;
    font-size: 13px;
    font-weight: 500;
    border-radius: 4px;
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

  .modal-btn-cancel {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--border);
  }

  .modal-btn-cancel:hover {
    background: var(--bg-hover);
  }

  .modal-btn-confirm {
    background: var(--accent);
    color: var(--text-on-accent);
    border: 1px solid transparent;
  }

  .modal-btn-confirm:hover {
    background: var(--accent-hover);
  }

  .modal-btn-confirm:active {
    background: var(--accent-active);
  }

  .modal-btn-confirm:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
