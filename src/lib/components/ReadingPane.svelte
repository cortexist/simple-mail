<script lang="ts">
  import type { Email } from '$lib/types';
  import { formatFullDate } from '$lib/utils';
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
    showAllHeaders?: boolean;
    multiSelectCount?: number;
    /** Fired when a label chip is clicked. Parent uses this to route to a
     *  label:<name> search query. */
    onLabelClick?: (label: string) => void;
  }

  let { email, loadingBody = false, darkMode = false, isJunk = false, focused = false, onFocus, onOpenAttachment, onSaveAttachment, showAllHeaders = $bindable(false), multiSelectCount = 0, onLabelClick }: Props = $props();

  // ── Attachment context menu ──
  let attachMenu: { x: number; y: number; index: number; filename: string } | null = $state(null);

  function openAttachMenu(e: MouseEvent, index: number, filename: string) {
    e.preventDefault();
    attachMenu = { x: e.clientX, y: e.clientY, index, filename };
  }

  function closeAttachMenu() { attachMenu = null; }

  function doOpenAttach() {
    if (attachMenu) onOpenAttachment?.(attachMenu.index);
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

  /** Build a self-contained HTML document for the iframe srcdoc */
  /** Check if the email HTML already supports dark mode natively */
  function hasDarkModeSupport(html: string): boolean {
    // color-scheme meta tag or CSS property containing "dark"
    if (/color-scheme[^"]*dark/i.test(html)) return true;
    // prefers-color-scheme media query
    if (/prefers-color-scheme\s*:\s*dark/i.test(html)) return true;
    return false;
  }

  /** Strip outer HTML/HEAD/BODY wrapper from an email so we can re-wrap it
   *  inside our own controlled document.  Preserves any style blocks from
   *  the original head section. */
  // Build regexes via constructor to avoid literal closing tags that confuse the Svelte parser
  const CL = '<' + '/';
  const RE_HEAD = new RegExp('<head[^>]*>([\\s\\S]*?)' + CL + 'head>', 'i');
  const RE_STYLE_G = new RegExp('<style[\\s\\S]*?' + CL + 'style>', 'gi');
  const RE_BODY = new RegExp('<body[^>]*>([\\s\\S]*?)' + CL + 'body>', 'i');
  const RE_HTML_TAGS = new RegExp(CL + '?html[^>]*>', 'gi');
  const RE_HEAD_BLOCK = new RegExp('<head[^>]*>[\\s\\S]*?' + CL + 'head>', 'i');

  function unwrapHtmlDocument(html: string): string {
    let s = html.trim();
    // Remove doctype
    s = s.replace(/^<!doctype[^>]*>/i, '').trim();
    // Extract <head> content (styles we want to keep)
    const headMatch = s.match(RE_HEAD);
    const headStyles = headMatch
      ? (headMatch[1].match(RE_STYLE_G) || []).join('\n')
      : '';
    // Extract <body> content
    const bodyMatch = s.match(RE_BODY);
    const inner = bodyMatch ? bodyMatch[1] : s.replace(RE_HTML_TAGS, '').replace(RE_HEAD_BLOCK, '');
    return headStyles ? headStyles + '\n' + inner : inner;
  }

  function bodySrcdoc(body: string): string {
    const nativeDark = hasDarkModeSupport(body);
    const content = /<!doctype|<html[\s>]/i.test(body) ? unwrapHtmlDocument(body) : body;
    const darkCss = darkMode && !nativeDark ? `
      html{filter:invert(0.9) hue-rotate(170deg);}
      img,video,picture,svg,canvas,[style*="background-image"]{filter:invert(0.9) hue-rotate(170deg);}
    ` : '';
    // Block all external resources for junk/spam emails
    const cspMeta = isJunk
      ? `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;">`
      : '';
    const css = 'html,body{margin:0;padding:0;height:auto!important;overflow:visible!important;min-height:0!important;}' +
      'body{font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;font-size:14px;line-height:1.65;color:#242424;overflow-wrap:break-word;}' +
      'img{max-width:100%;height:auto;}a{color:#0078d4;}' + darkCss;
    return '<!DOCTYPE html><html><head><meta charset="utf-8">' + cspMeta +
      '<' + 'style>' + css + '<' + '/style><' + '/head>' +
      '<body>' + content + '<' + '/body><' + '/html>';
  }

  /** Extract just the body content for the print-only div.
   *  Strips style blocks so they don't leak into the main document. */
  function printBody(body: string): string {
    let content = /<!doctype|<html[\s>]/i.test(body) ? unwrapHtmlDocument(body) : body;
    return content.replace(RE_STYLE_G, '');
  }

  /** Resize the iframe to match its content.  Only grows (never shrinks)
   *  after the initial sizing to avoid scroll-jump while reading. */
  let iframeInitialized = false;

  function resizeIframe(iframe: HTMLIFrameElement) {
    try {
      const doc = iframe.contentDocument;
      if (!doc?.body) return;

      let sentinel = doc.getElementById('__sentinel');
      if (!sentinel) {
        sentinel = doc.createElement('div');
        sentinel.id = '__sentinel';
        sentinel.style.cssText = 'clear:both;height:1px;';
        doc.body.appendChild(sentinel);
      }

      const h = sentinel.offsetTop + sentinel.offsetHeight;
      const cur = parseInt(iframe.style.height) || 0;

      // First sizing: allow shrink. After that only grow.
      if (!iframeInitialized) {
        if (h > 0) { iframe.style.height = h + 'px'; iframeInitialized = true; }
      } else if (h > cur) {
        iframe.style.height = h + 'px';
      }
    } catch { /* cross-origin – ignore */ }
  }

  /** Write HTML into the iframe via document.write() instead of srcdoc.
   *  This forces synchronous parsing and avoids Chromium srcdoc rendering
   *  bugs where content isn't fully painted on first load. */
  function writeIframeContent(iframe: HTMLIFrameElement, html: string) {
    try {
      const doc = iframe.contentDocument;
      if (!doc) return;
      doc.open();
      doc.write(html);
      doc.close();

      // Resize after layout
      requestAnimationFrame(() => {
        resizeIframe(iframe);
        requestAnimationFrame(() => resizeIframe(iframe));
      });

      // ResizeObserver for body dimension changes
      if (doc.body) {
        const ro = new ResizeObserver(() => resizeIframe(iframe));
        ro.observe(doc.body);

        // Watch all images
        doc.querySelectorAll('img').forEach((img) => {
          if (!img.complete) {
            img.addEventListener('load', () => resizeIframe(iframe), { once: true });
            img.addEventListener('error', () => resizeIframe(iframe), { once: true });
          }
        });

        // MutationObserver for late DOM changes
        const mo = new MutationObserver(() => resizeIframe(iframe));
        mo.observe(doc.body, { childList: true, subtree: true, attributes: true });

        // Polling fallback for the first 5s
        let polls = 0;
        const poll = setInterval(() => {
          resizeIframe(iframe);
          if (++polls >= 10) clearInterval(poll);
        }, 500);

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

  // Write content into iframe whenever email body or iframe element changes
  $effect(() => {
    if (iframeEl && email?.body) {
      iframeInitialized = false;
      hoveredLink = null;
      writeIframeContent(iframeEl, bodySrcdoc(email.body));
    }
  });

  // Reset headers view when email changes
</script>

<section class="reading-pane">
  <div class="reading-pane-indicator"></div>
  {#if email}
    <!-- Email Content -->
    <div class="email-content" bind:this={contentEl} class:pane-focused={focused}>
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
              <div class="auth-warning" title={email.authResults}>
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
                onclick={() => onOpenAttachment?.(attachment.index)}
                oncontextmenu={(e) => openAttachMenu(e, attachment.index, attachment.filename)}
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

        {#if attachMenu}
          <div class="attach-context-menu" style="left: {attachMenu.x}px; top: {attachMenu.y}px;">
            <button class="attach-menu-item" onclick={doOpenAttach}>{t('readingPane.attachOpen')}</button>
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
      <div class="email-body">
        {#if loadingBody && !email.body}
          <div class="body-loading">
            <svg class="spinner" width="20" height="20" viewBox="0 0 20 20">
              <circle cx="10" cy="10" r="8" fill="none" stroke="var(--border)" stroke-width="2" />
              <circle cx="10" cy="10" r="8" fill="none" stroke="var(--accent)" stroke-width="2" stroke-dasharray="28 22" stroke-linecap="round" />
            </svg>
            <span>{t('readingPane.loadingMessage')}</span>
          </div>
        {:else}
          <iframe
            bind:this={iframeEl}
            sandbox="allow-same-origin"
            scrolling="no"
            title={t('readingPane.emailBody')}
            class="body-iframe"
          ></iframe>
          <!-- Print-only: render body directly in DOM so it paginates correctly
               (iframes can't break across pages). Hidden on screen, visible in print. -->
          <div class="print-body">{@html printBody(email.body)}</div>
        {/if}
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
  .email-content {
    flex: 1;
    overflow-y: auto;
    outline: none;
  }

  /* ── Header ── */
  .email-header {
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
    border-radius: 10px;
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
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
    padding: 4px;
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

  .attach-menu-item:hover {
    background: var(--bg-hover);
  }

  /* ── Divider ── */
  .divider {
    border: none;
    border-top: 1px solid var(--border-light);
    margin: 0 32px 12px 32px;
  }

  /* ── Body ── */
  .email-body {
    font-size: 14px;
    line-height: 1.65;
    color: var(--text-primary);
    flex: 1;
    padding: 0 24px;
    min-height: 0;
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
