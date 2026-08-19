<script lang="ts">
  import type { Email } from '$lib/types';
  import { formatFullDate } from '$lib/utils';
  import { shouldForceSaveAs } from '$lib/utils/attachments';
  import { ensureReadableTextColors } from '$lib/readableColors';
  import { buildMailDocument, isHtmlDocument, stripStyleBlocks, unwrapHtmlDocument } from '$lib/emailBody';
  import { beginRenderTrace, installRenderTraceConsole, DISABLED_TRACE, type RenderTrace } from '$lib/renderTrace';
  import { open } from '@tauri-apps/plugin-shell';
  import { locale, t } from '$lib/i18n/index.svelte';

  interface Props {
    email: Email | null;
    loadingBody?: boolean;
    darkMode?: boolean;
    isJunk?: boolean;
    focused?: boolean;
    onFocus?: () => void;
    onOpenAttachment?: (index: number) => void;
    onSaveAttachment?: (index: number, filename: string) => void;
    /** Open a draft's attachment — a local file recorded in its draftData,
     * not a synced MIME part, so it takes a different backend path. */
    onOpenDraftAttachment?: (index: number) => void;
    showAllHeaders?: boolean;
    multiSelectCount?: number;
    /** Fired when a label chip is clicked. Parent uses this to route to a
     *  label:<name> search query. */
    onLabelClick?: (label: string) => void;
  }

  let { email, loadingBody = false, darkMode = false, isJunk = false, focused = false, onFocus, onOpenAttachment, onSaveAttachment, onOpenDraftAttachment, showAllHeaders = $bindable(false), multiSelectCount = 0, onLabelClick }: Props = $props();

  // ── Attachment context menu ──
  let attachMenu: { x: number; y: number; index: number; filename: string; unsafe: boolean } | null = $state(null);

  function openAttachMenu(e: MouseEvent, index: number, filename: string, mimeType: string) {
    e.preventDefault();
    attachMenu = { x: e.clientX, y: e.clientY, index, filename, unsafe: shouldForceSaveAs(filename, mimeType) };
  }

  function closeAttachMenu() { attachMenu = null; }

  function activateAttachment(index: number, filename: string, mimeType: string) {
    if (shouldForceSaveAs(filename, mimeType)) {
      onSaveAttachment?.(index, filename);
    } else {
      onOpenAttachment?.(index);
    }
  }

  function doOpenAttach() {
    if (attachMenu && !attachMenu.unsafe) onOpenAttachment?.(attachMenu.index);
    closeAttachMenu();
  }

  function doSaveAttach() {
    if (attachMenu) onSaveAttachment?.(attachMenu.index, attachMenu.filename);
    closeAttachMenu();
  }

  $effect(() => {
    if (!attachMenu) return;
    const onClick = () => closeAttachMenu();
    const onEsc = (ev: KeyboardEvent) => { if (ev.key === 'Escape') closeAttachMenu(); };
    window.addEventListener('click', onClick);
    window.addEventListener('keydown', onEsc);
    return () => {
      window.removeEventListener('click', onClick);
      window.removeEventListener('keydown', onEsc);
    };
  });

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  let iframeEl: HTMLIFrameElement | undefined = $state();
  let contentEl: HTMLDivElement | undefined = $state();

  function formatAuthBadges(authResults: string): string {
    return authResults.split(/\s+/).map((part) => {
      const [proto, result] = part.split('=');
      if (!proto || !result) return part;
      const cls = result === 'pass' ? 'auth-pass' : result === 'fail' || result === 'softfail' ? 'auth-fail' : 'auth-neutral';
      return `<span class="auth-badge ${cls}">${proto.toUpperCase()}=${result}</span>`;
    }).join(' ');
  }

  export function focus() {
    contentEl?.focus();
  }

  export function scroll(delta: number) {
    if (contentEl) contentEl.scrollTop += delta;
  }

  export function toggleHeaders() {
    showAllHeaders = !showAllHeaders;
  }
  let hoveredLink: string | null = $state(null);

  /** Build the self-contained document written into the iframe. The assembly
   *  lives in $lib/emailBody, where it stays pure and measurable. */
  function bodySrcdoc(body: string): string {
    return buildMailDocument(body, { darkMode, isJunk, ensureReadableTextColors });
  }

  /** Extract just the body content for the print-only div.
   *  Strips style blocks so they don't leak into the main document. */
  function printBody(body: string): string {
    const content = isHtmlDocument(body) ? unwrapHtmlDocument(body) : body;
    return stripStyleBlocks(content);
  }

  /** Resize the iframe to match its content.  Only grows (never shrinks)
   *  after the initial sizing to avoid scroll-jump while reading.
   *
   *  Measures the document's own geometry rather than an injected sentinel
   *  element, for two reasons learned the hard way:
   *
   *  1. `doc.close()` can return while the parser is still streaming the
   *     document (a render-blocking `@import` does this), so an element
   *     appended "at the end of body" freezes mid-document and everything
   *     the parser adds after it goes unmeasured — the mail shows only its
   *     top part, forever.
   *  2. The email's own CSS applies to anything we inject: a stock Gmail
   *     dark-mode hack (`img+div{display:none}`) hid the sentinel outright.
   *
   *  Trusting document geometry instead requires our `height:auto` reset to
   *  win the cascade against email boilerplate like `body{height:100%
   *  !important}` — bodySrcdoc guarantees that by repeating the geometry
   *  rules after the content, winning equal-specificity ties on source
   *  order. */
  let iframeInitialized = false;
  /** Timing for the document currently being rendered. No-op unless tracing is
   *  switched on — see $lib/renderTrace. */
  let renderTrace: RenderTrace = DISABLED_TRACE;

  /** Returns the height it measured, or null if the document wasn't ready. */
  function resizeIframe(iframe: HTMLIFrameElement): number | null {
    try {
      const doc = iframe.contentDocument;
      if (!doc?.body) return null;
      renderTrace.measure();

      // Fixed-width mail wider than the pane would be clipped on the right
      // (scrolling="no"). Engage squeeze mode (see geometryCss) to relax the
      // fixed widths. Engage-only: removing it when the overflow disappears
      // would re-create the overflow and oscillate; a fresh document (next
      // message, or re-selecting this one) starts un-squeezed anyway.
      const de = doc.documentElement;
      if (de.scrollWidth > de.clientWidth + 1 && !de.classList.contains('__mailSqueeze')) {
        de.classList.add('__mailSqueeze');
      }

      const h = Math.ceil(Math.max(
        doc.documentElement.getBoundingClientRect().height,
        doc.body.scrollHeight,
      ));
      const cur = parseInt(iframe.style.height) || 0;

      // First sizing: allow shrink. After that only grow.
      // `> 1`, not `> 0`: a render-blocked document (stylesheet import still
      // loading) reports ~1px — initializing on that locks the pane collapsed.
      if (!iframeInitialized) {
        if (h > 1) {
          iframe.style.height = h + 'px';
          iframeInitialized = true;
          renderTrace.mark('firstSize');
          renderTrace.resized(h);
        }
      } else if (h > cur) {
        iframe.style.height = h + 'px';
        renderTrace.resized(h);
      }
      return h;
    } catch { /* cross-origin – ignore */ }
    return null;
  }

  /** Coalesce every resize trigger onto one measurement per animation frame.
   *
   *  A mail with 79 images fires ~90 triggers while it loads — one per image,
   *  plus the ResizeObserver, the MutationObserver and the poll. Called
   *  directly, each one reads layout (`scrollWidth`, `getBoundingClientRect`,
   *  `scrollHeight`) and then writes `iframe.style.height`, so every trigger
   *  forces a synchronous layout and dirties the one after it.
   *
   *  Batching collapses that to one measurement per frame. Dark theme benefits
   *  a little more — `darkCss` filters the iframe root, so each repaint also
   *  re-runs the invert (measured at +1.2ms per resize on a 79-image digest in
   *  Chromium) — but the win is mostly the layout thrash, which both themes pay
   *  equally. This is not a fix for the dark-theme render delay; that cause is
   *  still unidentified, and the filter is too cheap to account for it. */
  function makeResizeScheduler(iframe: HTMLIFrameElement) {
    let frame = 0;
    // Retry budget for the initial sizing. A render-blocked document measures
    // ~1px and can't initialize (see resizeIframe), so keep asking until it
    // resolves — bounded, since a genuinely empty mail never exceeds 1px.
    let initialRetries = 60;
    const run = () => {
      frame = 0;
      resizeIframe(iframe);
      if (!iframeInitialized && initialRetries-- > 0) schedule();
    };
    const schedule = () => { if (!frame) frame = requestAnimationFrame(run); };
    const cancel = () => { if (frame) cancelAnimationFrame(frame); frame = 0; };
    return { schedule, cancel };
  }

  /** Drops the observers, timers and pending frame belonging to the document
   *  currently in the iframe. Every message switch replaces that document, and
   *  without this the previous message's poll and observers keep firing against
   *  the new one. */
  let teardownIframe: (() => void) | null = null;

  /** Write HTML into the iframe via document.write() instead of srcdoc.
   *  This forces synchronous parsing and avoids Chromium srcdoc rendering
   *  bugs where content isn't fully painted on first load. */
  function writeIframeContent(iframe: HTMLIFrameElement, html: string) {
    try {
      const doc = iframe.contentDocument;
      if (!doc) return;
      teardownIframe?.();
      // `filtered` is read off the generated document rather than the theme:
      // mail that styles itself for dark backgrounds renders unfiltered in both.
      // Held locally as well as in `renderTrace`: the closures below must
      // publish *this* render's timings even if a newer message has already
      // replaced the current trace by the time they run.
      const trace = beginRenderTrace({
        darkMode,
        filtered: html.includes('filter:invert'),
        docBytes: html.length,
        images: (html.match(/<img/gi) || []).length,
      });
      renderTrace = trace;
      doc.open();
      doc.write(html);
      doc.close();
      trace.mark('write');

      const { schedule, cancel } = makeResizeScheduler(iframe);
      // Measure after layout. Every other trigger below funnels into the same
      // scheduler, so they collapse into one measurement per frame.
      schedule();

      // ResizeObserver for body dimension changes
      if (doc.body) {
        const ro = new ResizeObserver(schedule);
        ro.observe(doc.body);

        // Parsing + subresources done. When the parser finishes async (see
        // resizeIframe's doc comment) this is the moment the content exists —
        // and the img listeners below saw none of it, since the imgs weren't
        // in the DOM yet when we queried.
        doc.defaultView?.addEventListener('load', () => {
          trace.mark('load');
          schedule();
        }, { once: true });

        // Watch all images (only those the parser has emitted so far — the
        // load event above covers the rest)
        doc.querySelectorAll('img').forEach((img) => {
          if (!img.complete) {
            img.addEventListener('load', schedule, { once: true });
            img.addEventListener('error', schedule, { once: true });
          }
        });

        // MutationObserver for late DOM changes
        const mo = new MutationObserver(schedule);
        mo.observe(doc.body, { childList: true, subtree: true, attributes: true });

        // Polling fallback for the first 5s, for changes none of the observers
        // above report. Gives up early once the document has finished loading
        // and stopped growing — the common case, where the remaining ticks
        // would each re-measure (and in dark theme re-filter) for nothing.
        let polls = 0;
        let stable = 0;
        let lastH = -1;
        const poll = setInterval(() => {
          const h = resizeIframe(iframe);
          stable = h !== null && h === lastH ? stable + 1 : 0;
          lastH = h ?? -1;
          if (++polls >= 10 || (stable >= 2 && doc.readyState === 'complete')) {
            clearInterval(poll);
            // Height has stopped moving: the render is as done as it gets, so
            // this is the point where the trace is worth reading.
            trace.finish();
          }
        }, 500);

        teardownIframe = () => {
          cancel();
          ro.disconnect();
          mo.disconnect();
          clearInterval(poll);
          trace.finish();   // no-op if the poll already published it
          teardownIframe = null;
        };

        // Intercept link clicks → open in system browser
        doc.addEventListener('click', (evt) => {
          const anchor = (evt.target as Element)?.closest?.('a');
          if (anchor) {
            evt.preventDefault();
            const href = anchor.getAttribute('href');
            if (href && (href.startsWith('http://') || href.startsWith('https://') || href.startsWith('mailto:'))) {
              open(href);
            }
          }
        });

        // Forward keyboard events from iframe to parent so global handler can process them
        doc.addEventListener('keydown', (evt) => {
          // Block Tab to prevent focus escaping to titlebar
          if (evt.key === 'Tab') { evt.preventDefault(); return; }
          const forwarded = new KeyboardEvent('keydown', {
            key: evt.key, code: evt.code,
            ctrlKey: evt.ctrlKey, shiftKey: evt.shiftKey,
            altKey: evt.altKey, metaKey: evt.metaKey,
            bubbles: true, cancelable: true,
          });
          const cancelled = !document.dispatchEvent(forwarded);
          if (cancelled) evt.preventDefault();
        });

        // Notify parent when iframe content gets focus
        doc.addEventListener('focus', () => onFocus?.(), true);

        // Show link URL on hover for security
        doc.addEventListener('mouseover', (evt) => {
          const anchor = (evt.target as Element)?.closest?.('a');
          if (anchor) {
            const href = anchor.getAttribute('href');
            if (href) hoveredLink = href;
          }
        });
        doc.addEventListener('mouseout', (evt) => {
          const anchor = (evt.target as Element)?.closest?.('a');
          if (anchor) hoveredLink = null;
        });
      }
    } catch { /* ignore */ }
  }

  // Expose __mailRenderTrace so render timing can be switched on from the
  // devtools console in a running build. Registration only; tracing stays off
  // until enabled (and the setting persists across restarts).
  installRenderTraceConsole();

  // Write content into iframe whenever email body or iframe element changes
  $effect(() => {
    if (iframeEl && email?.body) {
      iframeInitialized = false;
      hoveredLink = null;
      writeIframeContent(iframeEl, bodySrcdoc(email.body));
    }
    return () => teardownIframe?.();
  });

  // Reset headers view when email changes
</script>

<section class="reading-pane">
  <div class="reading-pane-indicator"></div>
  {#if email}
    <!-- Email Content -->
    <div class="email-content" class:pane-focused={focused}>
      <!-- Header -->
      <div class="email-header">
        <div class="email-subject-row">
          <h1 class="email-subject">{email.subject}</h1>
          {#if email.labels && email.labels.length > 0}
            <div class="email-labels" aria-label="Labels">
              {#each email.labels as label (label)}
                <button class="label-chip" type="button" onclick={() => onLabelClick?.(label)}>{label}</button>
              {/each}
            </div>
          {/if}
        </div>

        {#if showAllHeaders}
          <table class="all-headers">
            <tbody>
              <tr><td class="hdr-label">{t('readingPane.from')}</td><td class="hdr-value"><strong>{email.from.name}</strong> &lt;{email.from.email}&gt;</td></tr>
              <tr><td class="hdr-label">{t('readingPane.to')}</td><td class="hdr-value">{email.to.map((r) => r.name ? `${r.name} <${r.email}>` : r.email).join(', ')}</td></tr>
              {#if email.cc && email.cc.length > 0}
                <tr><td class="hdr-label">{t('readingPane.cc')}</td><td class="hdr-value">{email.cc.map((r) => r.name ? `${r.name} <${r.email}>` : r.email).join(', ')}</td></tr>
              {/if}
              {#if email.replyTo && email.replyTo !== email.from.email}
                <tr><td class="hdr-label">{t('readingPane.replyTo')}</td><td class="hdr-value">{email.replyTo}</td></tr>
              {/if}
              <tr><td class="hdr-label">{t('readingPane.date')}</td><td class="hdr-value">{formatFullDate(email.date)}</td></tr>
              <tr><td class="hdr-label">{t('readingPane.subject')}</td><td class="hdr-value">{email.subject}</td></tr>
              <tr><td class="hdr-label">{t('readingPane.folder')}</td><td class="hdr-value">{email.folder}</td></tr>
              {#if email.messageId}
                <tr><td class="hdr-label">{t('readingPane.messageId')}</td><td class="hdr-value hdr-mono">{email.messageId}</td></tr>
              {/if}
              {#if email.authResults}
                <tr><td class="hdr-label">{t('readingPane.auth')}</td><td class="hdr-value">{@html formatAuthBadges(email.authResults)}</td></tr>
              {/if}
            </tbody>
          </table>
        {:else}
          <div class="email-meta">
            <span class="meta-avatar" style="background-color: {email.from.color}">
              {#if email.from.photoUrl}
                <img class="meta-avatar-img" src={email.from.photoUrl} alt={email.from.name} />
              {:else}
                {email.from.initials}
              {/if}
            </span>
            <div class="meta-details">
              <div class="meta-from">
                <strong>{email.from.name}</strong>
                <span class="meta-email">&lt;{email.from.email}&gt;</span>
              </div>
              <div class="meta-to">
                {t('readingPane.to')}: {email.to.map((r) => r.name || r.email).join(', ')}
              </div>
            </div>
            <div class="meta-date">
              {formatFullDate(email.date)}
            </div>
          </div>
          {#if email.authResults}
            {@const hasFailure = email.authResults.includes('=fail') || email.authResults.includes('=softfail') || email.authResults.includes('=none')}
            {#if hasFailure}
              <div class="auth-warning" data-tooltip={email.authResults}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z" />
                  <line x1="12" y1="9" x2="12" y2="13" />
                  <line x1="12" y1="17" x2="12.01" y2="17" />
                </svg>
                <span>{t('readingPane.authWarning', { results: email.authResults })}</span>
              </div>
            {/if}
          {/if}
        {/if}

        {#if email.attachments && email.attachments.length > 0}
          <div class="attachments">
            {#each email.attachments as attachment (attachment.index)}
              <button
                class="attachment-chip"
                onclick={() => activateAttachment(attachment.index, attachment.filename, attachment.mimeType)}
                oncontextmenu={(e) => openAttachMenu(e, attachment.index, attachment.filename, attachment.mimeType)}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48" />
                </svg>
                <span class="attachment-name">{attachment.filename}</span>
                {#if attachment.size > 0}
                  <span class="attachment-size">{formatBytes(attachment.size)}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}

        <!-- Draft attachments live in draftData as local file paths (nothing
             is uploaded until send), so they render from there rather than
             from the synced-attachment list, and open via the local path. -->
        {#if email.folder === 'drafts' && email.draftData && email.draftData.attachments.length > 0}
          <div class="attachments">
            {#each email.draftData.attachments as att, i (att.path)}
              <button
                class="attachment-chip"
                onclick={() => onOpenDraftAttachment?.(i)}
                data-tooltip={att.path}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48" />
                </svg>
                <span class="attachment-name">{att.name}</span>
                {#if att.size > 0}
                  <span class="attachment-size">{formatBytes(att.size)}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}

        {#if attachMenu}
          <div class="attach-context-menu" style="left: {attachMenu.x}px; top: {attachMenu.y}px;">
            <button class="attach-menu-item" onclick={doOpenAttach} disabled={attachMenu.unsafe}>{t('readingPane.attachOpen')}</button>
            <button class="attach-menu-item" onclick={doSaveAttach}>{t('readingPane.attachSaveAs')}</button>
          </div>
        {/if}
        {#if email.isReplied}
        {@const repliedDate = email.repliedAt ? new Date(email.repliedAt) : null}
        <div class="replied-banner">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="9 17 4 12 9 7" />
            <path d="M20 18v-2a4 4 0 0 0-4-4H4" />
          </svg>
          {#if repliedDate}
            {t('readingPane.repliedAt', {
              time: repliedDate.toLocaleTimeString(locale(), { hour: 'numeric', minute: '2-digit' }),
              date: repliedDate.toLocaleDateString(locale(), { month: 'long', day: 'numeric', year: 'numeric' })
            })}
          {:else}
            {t('readingPane.replied')}
          {/if}
        </div>
      {/if}
      </div>
      
      <!-- Divider -->
      <hr class="divider" />

      <!-- Body -->
      <!-- The scroller, and the element the dark-theme invert hangs on. Its box is
           viewport-sized, which is the whole point: a filter over the full mail was
           ~9400px of surface on a 2x display, which WebKitGTK either failed to paint
           or re-rasterized on every unrelated repaint. See .email-scroll.inverted. -->
      <div class="email-scroll" bind:this={contentEl} class:inverted={darkMode}>
        <div class="email-body">
          {#if loadingBody && !email.body}
            <div class="body-loading">
              <svg class="spinner" width="20" height="20" viewBox="0 0 20 20">
                <circle cx="10" cy="10" r="8" fill="none" stroke="var(--border)" stroke-width="2" />
                <circle cx="10" cy="10" r="8" fill="none" stroke="var(--accent-text)" stroke-width="2" stroke-dasharray="28 22" stroke-linecap="round" />
              </svg>
              <span>{t('readingPane.loadingMessage')}</span>
            </div>
          {:else}
            <!-- allow-scripts is required for the app's own listeners (link
                 interception, key forwarding, hover preview) to run at all:
                 WebKitGTK blocks every listener on a script-less sandboxed
                 document, parent-registered ones included. Mail's own scripts are
                 blocked by the CSP that buildMailDocument emits instead — see the
                 note there. Deliberately absent: allow-top-navigation,
                 allow-popups, allow-forms, allow-modals. -->
            <iframe
              bind:this={iframeEl}
              sandbox="allow-same-origin allow-scripts"
              scrolling="no"
              title={t('readingPane.emailBody')}
              class="body-iframe"
              class:inverted={darkMode}
            ></iframe>
            <!-- Print-only: render body directly in DOM so it paginates correctly
                 (iframes can't break across pages). Hidden on screen, visible in print. -->
            <div class="print-body">{@html printBody(email.body)}</div>
          {/if}
        </div>
      </div>
    </div>
    {#if hoveredLink}
      <div class="link-tooltip">{hoveredLink}</div>
    {/if}
  {:else if multiSelectCount > 1}
    <!-- Multi-select: blank pane -->
    <div class="no-selection"></div>
  {:else}
    <!-- No Email Selected -->
    <div class="no-selection">
      <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="var(--text-tertiary)" stroke-width="0.75" stroke-linecap="round" stroke-linejoin="round">
        <rect x="2" y="4" width="20" height="16" rx="2" />
        <path d="M2 7l10 6 10-6" />
      </svg>
      <p class="no-selection-title">{t('readingPane.selectMessage')}</p>
      <p class="no-selection-subtitle">{t('readingPane.selectMessageHint')}</p>
    </div>
  {/if}
</section>

<style>
  .reading-pane {
    flex: 1;
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 300px;
    position: relative;
  }

  .reading-pane-indicator {
    height: 4px;
    width: 80px;
    padding: 0;
  }

  .reading-pane:has(.pane-focused) .reading-pane-indicator {
    background: var(--accent-active);
    z-index: 1;
  }

  /* ── Email Content ── */
  /* Pins the header: this column no longer scrolls, .email-scroll inside it does. */
  .email-content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    outline: none;
  }

  .email-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    outline: none;
  }

  /* Dark theme's document-wide invert.
     It hangs here, on the scroller, and that is load-bearing. A filter applies
     to the element's painted box, and this box is clipped to the viewport — so
     however tall the mail is, WebKit only ever filters a screenful. Hung on the
     mail instead it covers the whole message: a Medium digest is ~4700px, or
     ~9400px of surface on a 2x display, and past some size WebKitGTK stops
     painting most of it and never invalidates it — the mail goes blank until
     something forces a real repaint. Hung on the iframe element from the parent
     document it paints correctly but is re-rasterized by every unrelated repaint
     in the app, so hovering the message list visibly blinks the mail. Confirmed
     in that order on WebKitGTK 2.52; buildMailDocument's darkCss has the detail.
     The threshold tracks the pane's area, so it appears only when the window is
     large — alone on a sway workspace, not tiled.
     Must stay the inverse of the per-image counter-filter darkCss emits inside
     the document; the pair is what leaves pictures as authored.
     The backdrop is not affected by a filter, so the pane's own background still
     shows through unfiltered around the mail. */
  .email-scroll.inverted {
    filter: invert(0.9) hue-rotate(170deg);
  }

  /* The scrollbar is painted as part of the filtered element, so it has to be
     authored pre-inverted to come out looking like the app's. */
  .email-scroll.inverted::-webkit-scrollbar-thumb {
    background: rgba(0, 0, 0, 0.15);
  }

  .email-scroll.inverted::-webkit-scrollbar-thumb:hover {
    background: rgba(0, 0, 0, 0.25);
  }

  /* ── Header ── */
  .email-header {
    flex-shrink: 0;
    max-width: 1000px;
    display: flex;
    flex-direction: column;
    gap: 15px;
    padding: 16px 32px;
  }

  .email-subject-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .email-subject {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 20px;
    font-weight: 600;
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-primary);
  }

  .email-labels {
    flex: 0 1 auto;
    display: flex;
    flex-wrap: nowrap;
    gap: 4px;
    overflow: hidden;
  }
  .label-chip {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    border-radius: 8px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-hover);
    border: 1px solid var(--border-subtle);
    cursor: pointer;
    line-height: 1;
    white-space: nowrap;
  }
  .label-chip:hover {
    background: var(--bg-hover-strong, var(--bg-hover));
    color: var(--text-primary);
  }

  .email-meta {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .meta-avatar {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
    overflow: hidden;
  }

  .meta-avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
  }

  .meta-details {
    flex: 1;
    min-width: 0;
  }

  .meta-from {
    font-size: 14px;
    line-height: 1.4;
  }

  .meta-from strong {
    font-weight: 600;
    color: var(--text-primary);
  }

  .meta-email {
    color: var(--text-tertiary);
    font-size: 12px;
    margin-left: 4px;
  }

  .meta-to {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 2px;
  }

  .meta-date {
    font-size: 12px;
    color: var(--text-tertiary);
    white-space: nowrap;
    flex-shrink: 0;
    margin-top: 2px;
  }

  /* ── All Headers ── */
  .all-headers {
    border-collapse: collapse;
    font-size: 13px;
    background: var(--bg-secondary);
    border-radius: 6px;
    overflow: hidden;
  }

  .all-headers tr + tr {
    border-top: 1px solid var(--border-light);
  }

  .hdr-label {
    padding: 5px 10px;
    font-weight: 600;
    color: var(--text-secondary);
    white-space: nowrap;
    width: 1%;
    vertical-align: top;
  }

  .hdr-value {
    padding: 5px 10px 5px 0;
    color: var(--text-primary);
    word-break: break-word;
  }
  
  .hdr-mono {
    font-family: monospace;
    font-size: 11px;
    color: var(--text-secondary);
  }

  /* ── Auth badges ── */
  :global(.auth-badge) {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 11px;
    font-weight: 600;
    font-family: monospace;
    margin-right: 4px;
  }
  :global(.auth-pass) {
    background: rgba(34, 197, 94, 0.15);
    color: #16a34a;
  }
  :global(.auth-fail) {
    background: rgba(239, 68, 68, 0.15);
    color: #dc2626;
  }
  :global(.auth-neutral) {
    background: rgba(234, 179, 8, 0.15);
    color: #ca8a04;
  }

  .auth-warning {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
    background: #e0bc1b;
    color: #222;
  }

  .auth-warning svg {
    flex-shrink: 0;
  }

  /* ── Replied banner ── */
  .replied-banner {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
    background: color-mix(in srgb, #e0bc1b 15%, transparent);
    color: var(--text-secondary);
  }


  /* ── Attachments ── */
  .attachments {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .attachment-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-light);
    border-radius: 4px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
    transition: background 0.1s;
    font-family: inherit;
  }

  .attachment-name {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attachment-size {
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .attachment-chip:hover {
    background: var(--bg-hover);
  }

  .attach-context-menu {
    position: fixed;
    min-width: 160px;
    background: var(--bg-primary);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    box-shadow: var(--shadow-md, 0 8px 32px rgba(0, 0, 0, 0.18));
    z-index: 1000;
  }

  .attach-menu-item {
    width: 100%;
    text-align: left;
    border-radius: 4px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--text-primary);
    background: none;
    border: none;
    cursor: pointer;
    font-family: inherit;
  }

  .attach-menu-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .attach-menu-item:disabled {
    color: color-mix(in srgb, var(--text-primary) 40%, transparent);
    cursor: default;
  }

  /* ── Divider ── */
  .divider {
    flex-shrink: 0;
    border: none;
    border-top: 1px solid var(--border-light);
    margin-left: 32px;
    margin-right: 32px;
    margin-bottom: 12px;
    max-width: 940px;
  }

  /* ── Body ── */
  .email-body {
    font-size: 14px;
    line-height: 1.65;
    color: var(--text-primary);
    flex: 1;
    padding: 0 24px;
    min-height: 0;
    max-width: 1080px;
  }

  .body-iframe {
    width: 100%;
    border: none;
    display: block;
  }

  /* Print-only body: rendered directly in the DOM so content paginates.
     Hidden on screen; the iframe is hidden in print (see app.css). */
  .print-body {
    display: none;
  }

  @media print {
    .body-iframe {
      display: none !important;
    }
    .print-body {
      display: block !important;
      font-family: -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif;
      font-size: 14px;
      line-height: 1.65;
      color: #242424;
      overflow-wrap: break-word;
    }
    .print-body :global(img) {
      max-width: 100%;
      height: auto;
    }
  }

  .body-loading {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);
    font-size: 13px;
    padding: 12px 0;
  }

  .spinner {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* ── Link Tooltip ── */
  .link-tooltip {
    position: absolute;
    bottom: 0;
    left: 0;
    max-width: 80%;
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border-top: 1px solid var(--border-light);
    border-right: 1px solid var(--border-light);
    border-top-right-radius: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
    z-index: 10;
  }

  /* ── No Selection ── */
  .no-selection {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-tertiary);
    user-select: none;
  }

  .no-selection-title {
    font-size: 16px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .no-selection-subtitle {
    font-size: 13px;
    color: var(--text-tertiary);
  }
</style>
