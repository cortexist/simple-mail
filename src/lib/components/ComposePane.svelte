<script lang="ts">
  import type { Email, ComposeMode, Contact, ComposeDraft, RecipientSuggestion, ContactListMember } from '$lib/types';
  import { formatFullDate, isLikelyEmail } from '$lib/utils';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import { getContactLists } from '$lib/data/dataService';
  import { untrack } from 'svelte';
  import { t } from '$lib/i18n/index.svelte';

  interface Attachment {
    name: string;
    path: string;
    size: number;
  }

  interface Props {
    mode: ComposeMode;
    replyTo?: Email | null;
    draft?: ComposeDraft | null;
    accountId?: string;
    signature?: string;
    onDraftChange?: (draft: ComposeDraft) => void;
    onSend: (draft: { to: string; cc: string; bcc: string; subject: string; body: string; attachments: Attachment[] }) => void;
    onDiscard: () => void;
    onSaveDraft: () => void;
  }

  let { mode, replyTo = null, draft = null, accountId = '', signature = '', onDraftChange, onSend, onDiscard, onSaveDraft }: Props = $props();

  // Pre-fill fields based on compose mode
  let toField = $state('');
  let ccField = $state('');
  let subjectField = $state('');
  let bodyHtml = $state('');
  let showCc = $state(false);
  let showBcc = $state(false);
  let bccField = $state('');
  let editorEl: HTMLDivElement | undefined = $state();
  let showLinkDialog = $state(false);
  let linkUrl = $state('');
  let linkInputEl: HTMLInputElement | undefined = $state();
  let attachments = $state<Attachment[]>([]);
  let showEmojiPicker = $state(false);
  let savedSelection: Range | null = null;
  let emojiBtnEl: HTMLButtonElement | undefined = $state();
  let emojiPickerStyle = $state('');
  let composePaneEl: HTMLElement | undefined = $state();
  let toInputEl: HTMLInputElement | undefined = $state();
  let ccInputEl: HTMLInputElement | undefined = $state();
  let bccInputEl: HTMLInputElement | undefined = $state();
  let subjectInputEl: HTMLInputElement | undefined = $state();
  let sendBtnEl: HTMLButtonElement | undefined = $state();
  let discardBtnEl: HTMLButtonElement | undefined = $state();
  let showDiscardConfirm = $state(false);
  let discardCancelBtnEl: HTMLButtonElement | undefined = $state();
  let discardConfirmBtnEl: HTMLButtonElement | undefined = $state();

  // Track whether content has been modified from initial state
  let initialSnapshot = '';
  let isDirty = $derived.by(() => {
    const current = `${toField}|${ccField}|${bccField}|${subjectField}|${bodyHtml}|${attachments.length}`;
    return current !== initialSnapshot;
  });


  const EMOJI_GROUPS = [
    { key: 'compose.emojiSmileys', emojis: ['😊','😂','🤣','😍','😘','🥰','😎','🤔','😅','😢','😭','🥺','😤','🤗','🤩','😴','🙄','😏','😬','🫡'] },
    { key: 'compose.emojiGestures', emojis: ['👍','👎','👋','🤝','🙏','👏','💪','✌️','🤞','👀','🫶','🤙','👊','✋','🖐️','🤘'] },
    { key: 'compose.emojiHearts', emojis: ['❤️','🧡','💛','💚','💙','💜','🖤','🤍','💔','❣️','💕','💖','💗','💝','💘','💞'] },
    { key: 'compose.emojiObjects', emojis: ['📎','📌','📁','📧','📅','🔗','💡','🔔','⭐','🎉','🎁','🏆','🔥','✅','❌','⚡','💬','📝','📊','🗂️'] },
    { key: 'compose.emojiArrows', emojis: ['➡️','⬅️','⬆️','⬇️','↩️','↪️','🔄','🔀','▶️','⏸️','⏹️','⏩','⏪','🔁'] },
  ];

  // ── Recipient chip + autocomplete state ──
  interface RecipientChip {
    name: string;
    email: string;
    display: string; // "Name <email>" or just "email"
  }

  let toChips = $state<RecipientChip[]>([]);
  let ccChips = $state<RecipientChip[]>([]);
  let bccChips = $state<RecipientChip[]>([]);
  let toInput = $state('');
  let ccInput = $state('');
  let bccInput = $state('');
  let suggestions = $state<RecipientSuggestion[]>([]);
  let activeField = $state<'to' | 'cc' | 'bcc' | null>(null);
  let highlightedIdx = $state(-1);
  let suggestionsEl: HTMLDivElement | undefined = $state();
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  /** Parse "Name <email>" or bare "email" into a chip */
  function parseRecipient(raw: string): RecipientChip | null {
    const s = raw.trim();
    if (!s) return null;
    const match = s.match(/^(.+?)\s*<([^>]+)>$/);
    if (match) {
      return { name: match[1].trim(), email: match[2].trim(), display: s };
    }
    // Bare email
    if (s.includes('@')) {
      return { name: '', email: s, display: s };
    }
    // Treat as email anyway (user might be typing a local address)
    return { name: '', email: s, display: s };
  }

  /** Parse a semicolon-separated string into chips.
   *  We split on semicolons first, then only split on commas for segments
   *  that don't look like "Name <email>" (to preserve "Last, First <email>"). */
  function parseRecipientString(str: string): RecipientChip[] {
    // First split on semicolons (our canonical separator from chipsToString)
    const semiParts = str.split(';');
    const parts: string[] = [];
    for (const seg of semiParts) {
      // If the segment contains angle brackets, it's a "Name <email>" — keep it whole
      if (seg.includes('<') && seg.includes('>')) {
        parts.push(seg);
      } else {
        // Safe to split on commas for bare emails like "a@b.com, c@d.com"
        parts.push(...seg.split(','));
      }
    }
    return parts.map(parseRecipient).filter((c): c is RecipientChip => c !== null);
  }

  /** Serialize chips back to the semicolon-separated string for draft storage */
  function chipsToString(chips: RecipientChip[]): string {
    return chips.map((c) => c.display).join('; ');
  }

  // Keep toField/ccField/bccField in sync with chips (for draft persistence)
  $effect(() => { toField = chipsToString(toChips); });
  $effect(() => { ccField = chipsToString(ccChips); });
  $effect(() => { bccField = chipsToString(bccChips); });

  function commitInput(field: 'to' | 'cc' | 'bcc') {
    const input = field === 'to' ? toInput : field === 'cc' ? ccInput : bccInput;
    const chip = parseRecipient(input);
    if (chip) {
      if (field === 'to') { toChips = [...toChips, chip]; toInput = ''; }
      else if (field === 'cc') { ccChips = [...ccChips, chip]; ccInput = ''; }
      else { bccChips = [...bccChips, chip]; bccInput = ''; }
    }
    suggestions = [];
    highlightedIdx = -1;
  }

  function removeChip(field: 'to' | 'cc' | 'bcc', idx: number) {
    if (field === 'to') toChips = toChips.filter((_, i) => i !== idx);
    else if (field === 'cc') ccChips = ccChips.filter((_, i) => i !== idx);
    else bccChips = bccChips.filter((_, i) => i !== idx);
  }

  async function selectSuggestion(s: RecipientSuggestion) {
    if (s.source === 'list' && s.listId) {
      // Expand list members into individual chips
      const lists = await getContactLists(accountId);
      const list = lists.find(l => l.id === s.listId);
      if (list) {
        const existingEmails = new Set(
          (activeField === 'to' ? toChips : activeField === 'cc' ? ccChips : bccChips)
            .map(c => c.email.toLowerCase())
        );
        const newChips: RecipientChip[] = list.members
          .filter(m => !existingEmails.has(m.email.toLowerCase()))
          .map(m => ({
            name: m.name,
            email: m.email,
            display: m.name ? `${m.name} <${m.email}>` : m.email,
          }));
        if (activeField === 'to') { toChips = [...toChips, ...newChips]; toInput = ''; }
        else if (activeField === 'cc') { ccChips = [...ccChips, ...newChips]; ccInput = ''; }
        else if (activeField === 'bcc') { bccChips = [...bccChips, ...newChips]; bccInput = ''; }
      }
    } else {
      const chip: RecipientChip = {
        name: s.name,
        email: s.email,
        display: s.name ? `${s.name} <${s.email}>` : s.email,
      };
      if (activeField === 'to') { toChips = [...toChips, chip]; toInput = ''; }
      else if (activeField === 'cc') { ccChips = [...ccChips, chip]; ccInput = ''; }
      else if (activeField === 'bcc') { bccChips = [...bccChips, chip]; bccInput = ''; }
    }
    suggestions = [];
    highlightedIdx = -1;
  }

  async function searchRecipients(query: string) {
    if (query.trim().length < 1) { suggestions = []; return; }
    try {
      suggestions = await invoke<RecipientSuggestion[]>('search_recipients', { query: query.trim(), accountId });
      highlightedIdx = -1;
    } catch { suggestions = []; }
  }

  function handleRecipientInput(field: 'to' | 'cc' | 'bcc', value: string) {
    if (field === 'to') toInput = value;
    else if (field === 'cc') ccInput = value;
    else bccInput = value;
    activeField = field;
    // Debounce search
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => searchRecipients(value), 150);
  }

  function handleRecipientKeydown(field: 'to' | 'cc' | 'bcc', e: KeyboardEvent) {
    const input = field === 'to' ? toInput : field === 'cc' ? ccInput : bccInput;

    if (e.key === 'ArrowDown' && suggestions.length > 0) {
      e.preventDefault();
      highlightedIdx = Math.min(highlightedIdx + 1, suggestions.length - 1);
      return;
    }
    if (e.key === 'ArrowUp' && suggestions.length > 0) {
      e.preventDefault();
      highlightedIdx = Math.max(highlightedIdx - 1, 0);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (highlightedIdx >= 0 && highlightedIdx < suggestions.length) {
        selectSuggestion(suggestions[highlightedIdx]);
      } else {
        commitInput(field);
      }
      return;
    }
    if (e.key === ';' || e.key === ',') {
      e.preventDefault();
      commitInput(field);
      return;
    }
    if (e.key === 'Tab' && input.trim()) {
      e.preventDefault();
      if (highlightedIdx >= 0 && highlightedIdx < suggestions.length) {
        selectSuggestion(suggestions[highlightedIdx]);
      } else {
        commitInput(field);
      }
      return;
    }
    if (e.key === 'Backspace' && !input) {
      // Remove last chip
      const chips = field === 'to' ? toChips : field === 'cc' ? ccChips : bccChips;
      if (chips.length > 0) removeChip(field, chips.length - 1);
      return;
    }
    if (e.key === 'Escape') {
      suggestions = [];
      highlightedIdx = -1;
    }
  }

  function handleRecipientBlur(field: 'to' | 'cc' | 'bcc') {
    // Delay to allow click on suggestion to fire first
    setTimeout(() => {
      const input = field === 'to' ? toInput : field === 'cc' ? ccInput : bccInput;
      if (input.trim()) commitInput(field);
      if (activeField === field) { suggestions = []; activeField = null; }
    }, 200);
  }

  // Initialize fields: restore from saved draft, or build from mode + replyTo
  let initialized = false;
  $effect(() => {
    if (initialized) return;
    initialized = true;

    if (draft) {
      // Restore from saved draft
      toChips = parseRecipientString(draft.to);
      ccChips = parseRecipientString(draft.cc);
      bccChips = parseRecipientString(draft.bcc);
      subjectField = draft.subject;
      bodyHtml = draft.body;
      showCc = draft.showCc;
      showBcc = draft.showBcc;
      attachments = [...draft.attachments];
    } else {
      // Build fresh from compose mode
      attachments = [];
      const sig = signatureBlock();
      if (mode === 'new') {
        toChips = [];
        ccChips = [];
        subjectField = '';
        bodyHtml = `<br>${sig}`;
      } else if (mode === 'reply' && replyTo) {
        toChips = [{ name: replyTo.from.name, email: replyTo.from.email, display: `${replyTo.from.name} <${replyTo.from.email}>` }];
        ccChips = [];
        subjectField = `Re: ${replyTo.subject.replace(/^(Re|Fwd): /i, '')}`;
        bodyHtml = `<br>${sig}${buildQuotedBody(replyTo)}`;
      } else if (mode === 'replyAll' && replyTo) {
        toChips = [{ name: replyTo.from.name, email: replyTo.from.email, display: `${replyTo.from.name} <${replyTo.from.email}>` }];
        ccChips = replyTo.to
          .filter((c) => c.email !== 'you@company.com')
          .map((c) => ({ name: c.name, email: c.email, display: `${c.name} <${c.email}>` }));
        if (ccChips.length > 0) showCc = true;
        subjectField = `Re: ${replyTo.subject.replace(/^(Re|Fwd): /i, '')}`;
        bodyHtml = `<br>${sig}${buildQuotedBody(replyTo)}`;
      } else if (mode === 'forward' && replyTo) {
        toChips = [];
        ccChips = [];
        subjectField = `Fwd: ${replyTo.subject.replace(/^(Re|Fwd): /i, '')}`;
        bodyHtml = `<br>${sig}${buildForwardBody(replyTo)}`;
      }
    }
  });

  // Capture initial snapshot for dirty detection (runs after init effect populates derived fields)
  let snapshotCaptured = false;
  $effect(() => {
    // Access reactive fields to track them — we want this to run after init
    const snap = `${toField}|${ccField}|${bccField}|${subjectField}|${bodyHtml}|${attachments.length}`;
    if (initialized && !snapshotCaptured) {
      snapshotCaptured = true;
      initialSnapshot = snap;
    }
  });

  // Sync draft state up to parent whenever fields change
  $effect(() => {
    // Access all reactive fields to track them
    const d: ComposeDraft = {
      to: toField,
      cc: ccField,
      bcc: bccField,
      subject: subjectField,
      body: bodyHtml,
      showCc,
      showBcc,
      attachments: [...attachments],
    };
    // untrack prevents parent reactive reads (emails, activeDraftId) from
    // becoming dependencies of this effect, which would cause an infinite loop
    untrack(() => onDraftChange?.(d));
  });

  // Sync HTML into contenteditable when bodyHtml is set programmatically
  $effect(() => {
    if (editorEl && bodyHtml !== undefined) {
      // Only set innerHTML when it differs (avoid cursor jumps during typing)
      if (editorEl.innerHTML !== bodyHtml) {
        editorEl.innerHTML = bodyHtml;
      }
    }
  });

  // Continuously track the editor selection so toolbar actions always have a valid range
  $effect(() => {
    function onSelectionChange() {
      const sel = window.getSelection();
      if (sel && sel.rangeCount > 0 && editorEl?.contains(sel.anchorNode)) {
        savedSelection = sel.getRangeAt(0).cloneRange();
      }
    }
    document.addEventListener('selectionchange', onSelectionChange);
    return () => document.removeEventListener('selectionchange', onSelectionChange);
  });

  /** Strip style/link/script elements from email HTML to prevent style leaking and 404s. */
  function sanitizeQuotedHtml(html: string): string {
    let result = html;
    // Strip paired tags: style and script (open to close)
    for (const tag of ['style', 'script']) {
      const open = '<' + tag;
      const close = '</' + tag;
      let lower = result.toLowerCase();
      let start = lower.indexOf(open);
      while (start !== -1) {
        const end = lower.indexOf(close, start);
        if (end === -1) break;
        const tagEnd = lower.indexOf('>', end);
        if (tagEnd === -1) break;
        result = result.slice(0, start) + result.slice(tagEnd + 1);
        lower = result.toLowerCase();
        start = lower.indexOf(open);
      }
    }
    // Strip self-closing / void tags: <link ...>
    const linkOpen = '<' + 'link';
    let lower = result.toLowerCase();
    let pos = lower.indexOf(linkOpen);
    while (pos !== -1) {
      const tagEnd = lower.indexOf('>', pos);
      if (tagEnd === -1) break;
      result = result.slice(0, pos) + result.slice(tagEnd + 1);
      lower = result.toLowerCase();
      pos = lower.indexOf(linkOpen);
    }
    return result;
  }

  function signatureBlock(): string {
    if (!signature) return '';
    return `<div class="signature-block" style="margin-top:16px; color:#666;">${signature}</div>`;
  }

  function buildQuotedBody(email: Email): string {
    const header = `<b>From:</b> ${email.from.name} &lt;${email.from.email}&gt;<br>` +
      `<b>Sent:</b> ${formatFullDate(email.date)}<br>` +
      `<b>To:</b> ${email.to.map((t) => t.name).join(', ')}<br>` +
      `<b>Subject:</b> ${email.subject}`;
    return `<br><br><div class="quote-block" style="border-left:3px solid var(--accent, #0078d4); padding-left:16px; margin-top:4px; color:#666;">` +
      `<div style="font-size:12px; margin-bottom:8px;">${header}</div>` +
      `<div>${sanitizeQuotedHtml(email.body)}</div></div>`;
  }

  function buildForwardBody(email: Email): string {
    const header = `<b>From:</b> ${email.from.name} &lt;${email.from.email}&gt;<br>` +
      `<b>Date:</b> ${formatFullDate(email.date)}<br>` +
      `<b>Subject:</b> ${email.subject}<br>` +
      `<b>To:</b> ${email.to.map((t) => t.name).join(', ')}`;
    return `<br><br><div class="quote-block" style="border-left:3px solid var(--accent, #0078d4); padding-left:16px; margin-top:4px; color:#666;">` +
      `<div style="font-size:12px; margin-bottom:8px;">---------- Forwarded message ----------<br>${header}</div>` +
      `<div>${sanitizeQuotedHtml(email.body)}</div></div>`;
  }

  function handleEditorInput() {
    if (editorEl) {
      bodyHtml = editorEl.innerHTML;
    }
  }

  function saveSelection() {
    const sel = window.getSelection();
    if (sel && sel.rangeCount > 0 && editorEl?.contains(sel.anchorNode)) {
      savedSelection = sel.getRangeAt(0).cloneRange();
    }
  }

  function restoreSelection() {
    if (savedSelection) {
      const sel = window.getSelection();
      if (sel) {
        sel.removeAllRanges();
        sel.addRange(savedSelection);
      }
    }
  }

  function execFormat(command: string, value?: string) {
    // Capture before focus() triggers selectionchange which overwrites savedSelection
    const rangeToRestore = savedSelection?.cloneRange() ?? null;
    editorEl?.focus();
    if (rangeToRestore) {
      const sel = window.getSelection();
      if (sel) {
        sel.removeAllRanges();
        sel.addRange(rangeToRestore);
      }
    }
    document.execCommand(command, false, value);
    handleEditorInput();
  }

  /** Keep focus in editor when clicking toolbar buttons */
  function handleToolbarMousedown(e: MouseEvent) {
    // Prevent toolbar button clicks from stealing focus from contenteditable
    e.preventDefault();
  }

  function openLinkDialog() {
    saveSelection();
    linkUrl = '';
    showLinkDialog = true;
    requestAnimationFrame(() => linkInputEl?.focus());
  }

  function insertLink() {
    if (linkUrl.trim()) {
      showLinkDialog = false;
      execFormat('createLink', linkUrl.trim());
    }
  }

  function cancelLinkDialog() {
    showLinkDialog = false;
    editorEl?.focus();
  }

  let sendError = $state('');
  function handleSend() {
    const allChips = [...toChips, ...ccChips, ...bccChips];
    if (allChips.some(c => !isLikelyEmail(c.email))) {
      sendError = t('compose.invalidRecipients');
      return;
    }
    sendError = '';
    onSend({ to: toField, cc: ccField, bcc: bccField, subject: subjectField, body: bodyHtml, attachments });
  }

  async function openFilePicker() {
    try {
      const selected = await open({
        multiple: true,
        title: t('compose.attachFiles'),
      });
      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        for (const filePath of paths) {
          if (!filePath) continue;
          const name = filePath.split(/[\\/]/).pop() ?? filePath;
          // Avoid duplicates by path
          if (!attachments.some((a) => a.path === filePath)) {
            attachments.push({ name, path: filePath, size: 0 });
          }
        }
      }
    } catch (e) {
      console.error('File picker error:', e);
    }
  }

  function removeAttachment(path: string) {
    attachments = attachments.filter((a) => a.path !== path);
  }

  function toggleEmojiPicker() {
    if (!showEmojiPicker) {
      saveSelection();
      // Position picker near the emoji button
      if (emojiBtnEl) {
        const rect = emojiBtnEl.getBoundingClientRect();
        emojiPickerStyle = `position:fixed; top:${rect.bottom + 4}px; left:${Math.max(8, rect.right - 320)}px;`;
      }
    }
    showEmojiPicker = !showEmojiPicker;
  }

  function insertEmoji(emoji: string) {
    showEmojiPicker = false;
    editorEl?.focus();
    restoreSelection();
    document.execCommand('insertText', false, emoji);
    handleEditorInput();
  }

  function closeEmojiPicker() {
    showEmojiPicker = false;
  }

  /** Expose focus method so parent can direct focus into the compose pane */
  export function focus() {
    editorEl?.focus();
  }

  function getModeLabel(): string {
    switch (mode) {
      case 'new': return t('compose.newMessage');
      case 'reply': return t('compose.reply');
      case 'replyAll': return t('compose.replyAll');
      case 'forward': return t('compose.forward');
    }
  }

  // Auto-focus when the compose pane mounts: body for reply/forward (To is pre-filled), To for new
  $effect(() => {
    if (mode === 'new' || mode === 'forward') {
      if (toInputEl) requestAnimationFrame(() => toInputEl?.focus());
    } else {
      if (editorEl) requestAnimationFrame(() => editorEl?.focus());
    }
  });

  /** Get ordered list of tabbable elements within the compose pane */
  function getTabbableElements(): HTMLElement[] {
    const els: HTMLElement[] = [];
    if (toInputEl) els.push(toInputEl);
    if (showCc && ccInputEl) els.push(ccInputEl);
    if (showBcc && bccInputEl) els.push(bccInputEl);
    if (subjectInputEl) els.push(subjectInputEl);
    if (editorEl) els.push(editorEl);
    if (sendBtnEl) els.push(sendBtnEl);
    if (discardBtnEl) els.push(discardBtnEl);
    return els;
  }

  function requestDiscard() {
    if (isDirty) {
      showDiscardConfirm = true;
      requestAnimationFrame(() => discardCancelBtnEl?.focus());
    } else {
      onDiscard();
    }
  }

  function confirmDiscard() {
    showDiscardConfirm = false;
    onDiscard();
  }

  function cancelDiscard() {
    editorEl?.focus();
    showDiscardConfirm = false;
  }

  function handleComposeKeydown(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;

    // ── Ctrl+Shift+S: Send ──
    if (mod && e.shiftKey && e.key === 'S') {
      e.preventDefault();
      handleSend();
      return;
    }

    // ── Ctrl+S: Save draft and close ──
    if (mod && e.key === 's') {
      e.preventDefault();
      onSaveDraft();
      return;
    }

    // ── Escape: Discard (with confirmation if dirty) ──
    if (e.key === 'Escape') {
      e.preventDefault();
      requestDiscard();
      return;
    }

    // ── Alt+C: toggle Cc, Alt+B: toggle Bcc ──
    if (e.altKey && !mod && e.key === 'c') {
      e.preventDefault();
      if (showCc && (ccChips.length > 0 || ccInput.trim())) return; // don't hide if has values
      showCc = !showCc;
      if (showCc) requestAnimationFrame(() => ccInputEl?.focus());
      return;
    }
    if (e.altKey && !mod && e.key === 'b') {
      e.preventDefault();
      if (showBcc && (bccChips.length > 0 || bccInput.trim())) return; // don't hide if has values
      showBcc = !showBcc;
      if (showBcc) requestAnimationFrame(() => bccInputEl?.focus());
      return;
    }

    // ── Tab trap ──
    if (e.key !== 'Tab') return;

    // If a child handler already handled Tab (e.g. committing a recipient chip), don't also move focus
    if (e.defaultPrevented) return;

    e.preventDefault();
    const els = getTabbableElements();
    if (els.length === 0) return;

    const active = document.activeElement as HTMLElement;
    let idx = els.indexOf(active);

    if (e.shiftKey) {
      idx = idx <= 0 ? els.length - 1 : idx - 1;
    } else {
      idx = idx >= els.length - 1 ? 0 : idx + 1;
    }

    els[idx].focus();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="compose-pane" aria-label={getModeLabel()} bind:this={composePaneEl} onkeydown={handleComposeKeydown}>
  <!-- Compose Header -->
  <div class="compose-header">
    <h2 class="compose-title">{getModeLabel()}</h2>
    <div class="compose-header-actions">
      <button class="compose-header-btn" onclick={onSaveDraft} data-tooltip={t('compose.saveDraftShortcut')} data-tooltip-position="bottom" aria-label={t('compose.saveDraft')}>
        <svg width="18" height="18" viewBox="0 0 16 16">
          <path fill="currentColor" d="M4 3a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1V9.5A1.5 1.5 0 0 1 5.5 8h5A1.5 1.5 0 0 1 12 9.5V13a1 1 0 0 0 1-1V5.621a1 1 0 0 0-.293-.707l-1.621-1.621A1 1 0 0 0 10.379 3H10v1.5A1.5 1.5 0 0 1 8.5 6h-2A1.5 1.5 0 0 1 5 4.5V3zm2 0v1.5a.5.5 0 0 0 .5.5h2a.5.5 0 0 0 .5-.5V3zm5 10V9.5a.5.5 0 0 0-.5-.5h-5a.5.5 0 0 0-.5.5V13zM2 4a2 2 0 0 1 2-2h6.379a2 2 0 0 1 1.414.586l1.621 1.621A2 2 0 0 1 14 5.621V12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z"/>
        </svg>
      </button>
      <button class="compose-header-btn" onclick={requestDiscard} data-tooltip={t('compose.discardShortcut')} data-tooltip-position="bottom" aria-label={t('compose.discard')}>
        <svg width="18" height="18" viewBox="0 0 24 24">
          <path fill="currentColor" d="M10 5h4a2 2 0 1 0-4 0M8.5 5a3.5 3.5 0 1 1 7 0h5.75a.75.75 0 0 1 0 1.5h-1.32l-1.17 12.111A3.75 3.75 0 0 1 15.026 22H8.974a3.75 3.75 0 0 1-3.733-3.389L4.07 6.5H2.75a.75.75 0 0 1 0-1.5zm2 4.75a.75.75 0 0 0-1.5 0v7.5a.75.75 0 0 0 1.5 0zM14.25 9a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-1.5 0v-7.5a.75.75 0 0 1 .75-.75m-7.516 9.467a2.25 2.25 0 0 0 2.24 2.033h6.052a2.25 2.25 0 0 0 2.24-2.033L18.424 6.5H5.576z"/>
        </svg>
      </button>
    </div>
  </div>

  <!-- Address Fields -->
  <div class="compose-fields">
    <div class="field-row">
      <label class="field-label" for="compose-to">{t('compose.to')}</label>
      <div class="chip-field">
        {#each toChips as chip, i}
          <span class="recipient-chip" class:invalid={!isLikelyEmail(chip.email)} data-tooltip={isLikelyEmail(chip.email) ? chip.display : `${chip.display} — ${t('compose.invalidEmail')}`}>
            <span class="chip-text">{chip.name || chip.email}</span>
            <button class="chip-remove" onclick={() => removeChip('to', i)} aria-label={t('common.remove')}>
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </span>
        {/each}
        <input
          id="compose-to"
          class="chip-input"
          type="text"
          placeholder={toChips.length === 0 ? t('compose.recipients') : ''}
          value={toInput}
          oninput={(e) => handleRecipientInput('to', (e.target as HTMLInputElement).value)}
          onkeydown={(e) => handleRecipientKeydown('to', e)}
          onfocus={() => { activeField = 'to'; if (toInput.trim()) searchRecipients(toInput); }}
          onblur={() => handleRecipientBlur('to')}
          bind:this={toInputEl}
        />
      </div>
      <div class="field-extras">
        {#if !showCc}
          <button class="field-toggle" onclick={() => (showCc = true)}>{t('compose.cc')}</button>
        {/if}
        {#if !showBcc}
          <button class="field-toggle" onclick={() => (showBcc = true)}>{t('compose.bcc')}</button>
        {/if}
      </div>
    </div>

    {#if showCc}
      <div class="field-row">
        <label class="field-label" for="compose-cc">{t('compose.cc')}</label>
        <div class="chip-field">
          {#each ccChips as chip, i}
            <span class="recipient-chip" class:invalid={!isLikelyEmail(chip.email)} data-tooltip={isLikelyEmail(chip.email) ? chip.display : `${chip.display} — ${t('compose.invalidEmail')}`}>
              <span class="chip-text">{chip.name || chip.email}</span>
              <button class="chip-remove" onclick={() => removeChip('cc', i)} aria-label={t('common.remove')}>
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                </svg>
              </button>
            </span>
          {/each}
          <input
            id="compose-cc"
            class="chip-input"
            type="text"
            placeholder={ccChips.length === 0 ? t('compose.ccRecipients') : ''}
            value={ccInput}
            oninput={(e) => handleRecipientInput('cc', (e.target as HTMLInputElement).value)}
            onkeydown={(e) => handleRecipientKeydown('cc', e)}
            onfocus={() => { activeField = 'cc'; if (ccInput.trim()) searchRecipients(ccInput); }}
            onblur={() => handleRecipientBlur('cc')}
            bind:this={ccInputEl}
          />
        </div>
      </div>
    {/if}

    {#if showBcc}
      <div class="field-row">
        <label class="field-label" for="compose-bcc">{t('compose.bcc')}</label>
        <div class="chip-field">
          {#each bccChips as chip, i}
            <span class="recipient-chip" class:invalid={!isLikelyEmail(chip.email)} data-tooltip={isLikelyEmail(chip.email) ? chip.display : `${chip.display} — ${t('compose.invalidEmail')}`}>
              <span class="chip-text">{chip.name || chip.email}</span>
              <button class="chip-remove" onclick={() => removeChip('bcc', i)} aria-label={t('common.remove')}>
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                </svg>
              </button>
            </span>
          {/each}
          <input
            id="compose-bcc"
            class="chip-input"
            type="text"
            placeholder={bccChips.length === 0 ? t('compose.bccRecipients') : ''}
            value={bccInput}
            oninput={(e) => handleRecipientInput('bcc', (e.target as HTMLInputElement).value)}
            onkeydown={(e) => handleRecipientKeydown('bcc', e)}
            onfocus={() => { activeField = 'bcc'; if (bccInput.trim()) searchRecipients(bccInput); }}
            onblur={() => handleRecipientBlur('bcc')}
            bind:this={bccInputEl}
          />
        </div>
      </div>
    {/if}

    <!-- Autocomplete dropdown -->
    {#if suggestions.length > 0 && activeField}
      <div class="suggestions-dropdown" bind:this={suggestionsEl}>
        {#each suggestions as s, i}
          <button
            class="suggestion-item"
            class:highlighted={i === highlightedIdx}
            onmousedown={(e) => { e.preventDefault(); selectSuggestion(s); }}
            onmouseenter={() => (highlightedIdx = i)}
          >
            <span class="suggestion-avatar" style="background-color: {s.color}">
              {#if s.source === 'list'}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>
                </svg>
              {:else if s.photoUrl}
                <img class="suggestion-avatar-img" src={s.photoUrl} alt="" />
              {:else}
                {s.initials}
              {/if}
            </span>
            <div class="suggestion-info">
              <span class="suggestion-name">
                {s.name || s.email}
                {#if s.isFavorite}
                  <svg class="suggestion-star" width="12" height="12" viewBox="0 0 24 24" fill="var(--warning, #e8a517)" stroke="none"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01z"/></svg>
                {/if}
              </span>
              {#if s.source === 'list'}
                <span class="suggestion-email">{s.email}</span>
              {:else if s.name}
                <span class="suggestion-email">{s.email}</span>
              {/if}
            </div>
            <span class="suggestion-source">{s.source === 'list' ? t('compose.sourceList') : s.source === 'contact' ? t('compose.sourceContact') : t('compose.sourceSender')}</span>
          </button>
        {/each}
      </div>
    {/if}

    <div class="field-row">
      <label class="field-label" for="compose-subject">{t('compose.subject')}</label>
      <input
        id="compose-subject"
        class="field-input"
        type="text"
        placeholder={t('compose.addSubject')}
        bind:value={subjectField}
        bind:this={subjectInputEl}
      />
    </div>
  </div>

  <!-- Formatting Toolbar -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="compose-toolbar" onmousedown={handleToolbarMousedown} role="toolbar">
    <button class="toolbar-btn" data-tooltip={t('compose.bold')} data-tooltip-position="bottom" aria-label={t('compose.bold')} onclick={() => execFormat('bold')}>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <path fill="currentColor" d="M4 3.3C4 2.6 4.6 2 5.3 2h3.3c2 0 3.5 1.7 3.5 3.5c0 .7-.2 1.4-.6 1.9c.7.6 1.2 1.6 1.2 2.8c0 2.4-2 3.7-3.7 3.7H5.3c-.7.1-1.3-.5-1.3-1.2zm2.6 1.3v1.9h2c.5 0 1-.4 1-1c0-.5-.4-1-1-1h-2zm0 4.5v2.4H9c.6 0 1.2-.5 1.2-1.2S9.6 9.1 9 9.1z"/>
      </svg>
    </button>
    <button class="toolbar-btn" data-tooltip={t('compose.italic')} data-tooltip-position="bottom" aria-label={t('compose.italic')} onclick={() => execFormat('italic')}>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <path fill="currentColor" d="M12.8 2H7a.75.75 0 0 0 0 1.5h2.01l-3.428 9H3.2a.75.75 0 0 0 0 1.5H9a.75.75 0 0 0 0-1.5H7.188l3.428-9H12.8a.75.75 0 0 0 0-1.5"/>
      </svg>
    </button>
    <button class="toolbar-btn" data-tooltip={t('compose.underline')} data-tooltip-position="bottom" aria-label={t('compose.underline')} onclick={() => execFormat('underline')}>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <path fill="currentColor" d="M4.5 2a.5.5 0 0 1 .5.5V8c0 1.624 1.376 3 3 3s3-1.376 3-3V2.5a.5.5 0 0 1 1 0V8c0 2.176-1.824 4-4 4s-4-1.824-4-4V2.5a.5.5 0 0 1 .5-.5M4 13.5a.5.5 0 0 1 .5-.5h7a.5.5 0 0 1 0 1h-7a.5.5 0 0 1-.5-.5"/>
      </svg>
    </button>
    <div class="toolbar-separator"></div>
    <button class="toolbar-btn" data-tooltip={t('compose.bulletList')} data-tooltip-position="bottom" aria-label={t('compose.bulletList')} onclick={() => execFormat('insertUnorderedList')}>
      <svg width="16" height="16" viewBox="0 0 16 16"><path fill="currentColor" d="M2 4.5a1 1 0 1 0 0-2a1 1 0 0 0 0 2M2 9a1 1 0 1 0 0-2a1 1 0 0 0 0 2m1 3.5a1 1 0 1 1-2 0a1 1 0 0 1 2 0M5.5 3a.5.5 0 0 0 0 1h9a.5.5 0 0 0 0-1zM5 8a.5.5 0 0 1 .5-.5h9a.5.5 0 0 1 0 1h-9A.5.5 0 0 1 5 8m.5 4a.5.5 0 0 0 0 1h9a.5.5 0 0 0 0-1z"/>
      </svg>
    </button>
    <button class="toolbar-btn" data-tooltip={t('compose.numberList')} data-tooltip-position="bottom" aria-label={t('compose.numberList')} onclick={() => execFormat('insertOrderedList')}>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <path fill="currentColor" d="M3.684 1.01c.193.045.33.21.33.402v3.294a.42.42 0 0 1-.428.412a.42.42 0 0 1-.428-.412V2.58a3 3 0 0 1-.664.435a.436.436 0 0 1-.574-.184a.405.405 0 0 1 .192-.552c.353-.17.629-.432.82-.661a3 3 0 0 0 .27-.388a.44.44 0 0 1 .482-.22m-1.53 6.046a.4.4 0 0 1 0-.582l.002-.001V6.47l.004-.002l.008-.008a1 1 0 0 1 .103-.084a2.2 2.2 0 0 1 1.313-.435h.007c.32.004.668.084.947.283c.295.21.485.536.485.951c0 .452-.207.767-.488.992c-.214.173-.49.303-.714.409q-.054.024-.103.049c-.267.128-.468.24-.61.39a.8.8 0 0 0-.147.22h1.635a.42.42 0 0 1 .427.411a.42.42 0 0 1-.428.412H2.457a.42.42 0 0 1-.428-.412c0-.51.17-.893.446-1.184c.259-.275.592-.445.86-.574q.065-.03.124-.06c.231-.11.4-.19.529-.293c.12-.097.18-.193.18-.36c0-.148-.057-.23-.14-.289a.8.8 0 0 0-.448-.122a1.32 1.32 0 0 0-.818.289l-.005.005a.44.44 0 0 1-.602-.003m.94 5.885a.42.42 0 0 1 .427-.412c.294 0 .456-.08.537-.15a.3.3 0 0 0 .11-.246c-.006-.16-.158-.427-.647-.427c-.352 0-.535.084-.618.137a.4.4 0 0 0-.076.062l-.003.004l.01-.018v.001l-.002.002l-.002.004l-.003.006l-.005.008l.002-.003a.436.436 0 0 1-.563.165a.405.405 0 0 1-.191-.552v-.002l.002-.003l.003-.006l.008-.013a.7.7 0 0 1 .087-.12c.058-.067.142-.146.259-.22c.238-.153.59-.276 1.092-.276c.88 0 1.477.556 1.502 1.22c.012.303-.1.606-.339.84c.238.232.351.535.34.838c-.026.664-.622 1.22-1.503 1.22c-.502 0-.854-.122-1.092-.275a1.2 1.2 0 0 1-.326-.308l-.02-.033l-.008-.013l-.003-.005l-.001-.003v-.001l-.001-.001a.405.405 0 0 1 .19-.553a.436.436 0 0 1 .564.165l.003.004c.01.01.033.035.076.063c.083.053.266.137.618.137c.489 0 .641-.268.648-.428a.3.3 0 0 0-.11-.245c-.082-.072-.244-.151-.538-.151a.42.42 0 0 1-.427-.412M7.5 3a.5.5 0 0 0 0 1h6a.5.5 0 0 0 0-1zm0 4a.5.5 0 0 0 0 1h6a.5.5 0 0 0 0-1zm0 4a.5.5 0 0 0 0 1h6a.5.5 0 0 0 0-1z"/>
      </svg>
    </button>
    <div class="toolbar-separator"></div>
    <button class="toolbar-btn" data-tooltip={t('compose.insertLink')} data-tooltip-position="bottom" aria-label={t('compose.insertLink')} onclick={openLinkDialog}>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <path fill="currentColor" d="M4 2a2 2 0 0 0-2 2v4a2 2 0 0 0 2 2h1V9H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1v1a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2zm7 4v1h1a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1V6a2 2 0 0 0-2 2v4a2 2 0 0 0 2 2h4a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2z"/>
      </svg>
    </button>
    <button class="toolbar-btn" data-tooltip={t('compose.attachFile')} data-tooltip-position="bottom" aria-label={t('compose.attachFile')} onclick={openFilePicker}>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <path fill="currentColor" d="M2.283 7.975a.5.5 0 0 0 .854.354l4.595-4.597a2.5 2.5 0 1 1 3.536 3.536l-5.303 5.303a1 1 0 0 1-1.414-1.414l5.303-5.303a.5.5 0 0 0-.708-.708L3.843 10.45a2 2 0 1 0 2.828 2.828l5.303-5.303a3.5 3.5 0 1 0-4.95-4.95L2.43 7.621a.5.5 0 0 0-.146.354"/>
      </svg>
    </button>
    <button class="toolbar-btn" data-tooltip={t('compose.insertEmoji')} data-tooltip-position="bottom" aria-label={t('compose.insertEmoji')} onclick={toggleEmojiPicker} bind:this={emojiBtnEl}>
      <svg width="16" height="16" viewBox="0 0 16 16">
        <path fill="currentColor" d="M6.25 7.75a.75.75 0 1 0 0-1.5a.75.75 0 0 0 0 1.5m-.114 1.917a.5.5 0 1 0-.745.667A3.5 3.5 0 0 0 8 11.5a3.5 3.5 0 0 0 2.609-1.166a.5.5 0 0 0-.745-.667A2.5 2.5 0 0 1 8 10.5c-.74 0-1.405-.321-1.864-.833M10.5 7A.75.75 0 1 1 9 7a.75.75 0 0 1 1.5 0M14 8A6 6 0 1 0 2 8a6 6 0 0 0 12 0M3 8a5 5 0 1 1 10 0A5 5 0 0 1 3 8"/>
      </svg>
    </button>
  </div>

  <!-- Emoji Picker -->
  {#if showEmojiPicker}
    <div class="emoji-overlay" onclick={closeEmojiPicker} role="presentation"></div>
    <div class="emoji-picker" style={emojiPickerStyle}>
      <div class="emoji-picker-header">
        <span class="emoji-picker-title">{t('compose.emoji')}</span>
        <button class="emoji-picker-close" onclick={closeEmojiPicker} aria-label={t('compose.closeEmojiPicker')}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </div>
      <div class="emoji-picker-body">
        {#each EMOJI_GROUPS as group}
          <div class="emoji-group">
            <div class="emoji-group-label">{t(group.key)}</div>
            <div class="emoji-grid">
              {#each group.emojis as emoji}
                <button class="emoji-btn" onclick={() => insertEmoji(emoji)} aria-label={emoji}>{emoji}</button>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Attachments -->
  {#if attachments.length > 0}
    <div class="attachments-bar">
      {#each attachments as att (att.path)}
        <div class="attachment-chip">
          <svg class="attachment-chip-icon" width="14" height="14" viewBox="0 0 16 16">
            <path fill="currentColor" d="M2.283 7.975a.5.5 0 0 0 .854.354l4.595-4.597a2.5 2.5 0 1 1 3.536 3.536l-5.303 5.303a1 1 0 0 1-1.414-1.414l5.303-5.303a.5.5 0 0 0-.708-.708L3.843 10.45a2 2 0 1 0 2.828 2.828l5.303-5.303a3.5 3.5 0 1 0-4.95-4.95L2.43 7.621a.5.5 0 0 0-.146.354"/>
          </svg>
          <span class="attachment-chip-name" title={att.path}>{att.name}</span>
          <button class="attachment-chip-remove" onclick={() => removeAttachment(att.path)} aria-label={t('compose.removeAttachment', { name: att.name })}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Body Editor -->
  <div class="compose-body-wrap">
    <div
      class="compose-body"
      contenteditable="true"
      role="textbox"
      aria-multiline="true"
      aria-label={t('compose.messageBody')}
      bind:this={editorEl}
      oninput={handleEditorInput}
      data-placeholder={t('compose.messagePlaceholder')}
    ></div>
  </div>

  <!-- Footer Actions -->
  <div class="compose-footer">
    {#if sendError}
      <span class="send-error" role="alert">{sendError}</span>
    {/if}
    <button class="send-btn" data-tooltip={t('compose.sendShortcut')} data-tooltip-position="top" onclick={handleSend} bind:this={sendBtnEl}>
      <svg width="14" height="14" viewBox="0 0 24 24">
        <path fill="currentColor" d="M5.694 12L2.299 3.272a.75.75 0 0 1 .942-.982l.093.039l18 9a.75.75 0 0 1 .097 1.284l-.097.058l-18 9c-.583.291-1.217-.245-1.065-.848l.03-.095zL2.299 3.272zM4.402 4.54l2.61 6.71h6.627a.75.75 0 0 1 .743.648l.007.102a.75.75 0 0 1-.649.743l-.101.007H7.01l-2.609 6.71L19.322 12z"/>
      </svg>
      {t('compose.send')}
    </button>
  </div>

  <!-- Insert Link Dialog -->
  {#if showLinkDialog}
    <div class="link-dialog-overlay" onclick={cancelLinkDialog} role="presentation"></div>
    <div class="link-dialog">
      <div class="link-dialog-header">
        <span class="link-dialog-title">{t('compose.insertLink')}</span>
        <button class="link-dialog-close" onclick={cancelLinkDialog} aria-label={t('common.cancel')}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </div>
      <input
        class="link-dialog-input"
        type="url"
        placeholder="https://example.com"
        bind:value={linkUrl}
        bind:this={linkInputEl}
        onkeydown={(e) => { if (e.key === 'Enter') insertLink(); if (e.key === 'Escape') cancelLinkDialog(); }}
      />
      <div class="link-dialog-actions">
        <button class="link-dialog-btn primary" onclick={insertLink}>{t('compose.insert')}</button>
        <button class="link-dialog-btn" onclick={cancelLinkDialog}>{t('common.cancel')}</button>
      </div>
    </div>
  {/if}

  <!-- Discard Confirmation Dialog -->
  {#if showDiscardConfirm}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="discard-overlay" onmousedown={cancelDiscard} role="presentation"></div>
    <div class="discard-dialog" role="alertdialog" tabindex="-1" aria-labelledby="discard-dialog-title" onkeydown={(e) => {
      e.stopPropagation();
      if (e.key === 'Tab' || e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault();
        const btns = [discardConfirmBtnEl, discardCancelBtnEl].filter(Boolean) as HTMLButtonElement[];
        const idx = btns.indexOf(document.activeElement as HTMLButtonElement);
        const next = idx === 0 ? 1 : 0;
        btns[next]?.focus();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        cancelDiscard();
      } else if (e.key === 'Enter') {
        e.preventDefault();
        (document.activeElement as HTMLButtonElement)?.click();
      }
    }}>
      <p id="discard-dialog-title" class="discard-dialog-text">{t('compose.discardConfirm')}</p>
      <div class="discard-dialog-actions">
        <button class="discard-dialog-btn danger" onclick={confirmDiscard} bind:this={discardConfirmBtnEl}>{t('compose.discard')}</button>
        <button class="discard-dialog-btn" onclick={cancelDiscard} bind:this={discardCancelBtnEl}>{t('common.cancel')}</button>
      </div>
    </div>
  {/if}
</section>

<style>
  .compose-pane {
    flex: 1;
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 300px;
    position: relative;
  }

  /* ── Header ── */
  .compose-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 24px 8px;
    flex-shrink: 0;
  }

  .compose-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .compose-header-actions {
    display: flex;
    gap: 4px;
  }

  .compose-header-btn {
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
    outline: none;
  }

  .compose-header-btn:hover {
    color: var(--accent-hover);
  }

  /* ── Address Fields ── */
  .compose-fields {
    padding: 0 24px;
    flex-shrink: 0;
    position: relative;
  }

  .field-row {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border-light);
    min-height: 36px;
  }

  .field-label {
    font-size: 13px;
    color: var(--text-secondary);
    width: 56px;
    flex-shrink: 0;
    font-weight: 500;
    align-self: flex-start;
    padding-top: 8px;
  }

  .chip-field {
    flex: 1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    padding: 4px 0;
    min-width: 0;
    min-height: 32px;
  }

  .recipient-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 4px 2px 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-light);
    border-radius: 12px;
    font-size: 12px;
    color: var(--text-primary);
    max-width: 200px;
    white-space: nowrap;
  }

  .chip-text {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .recipient-chip.invalid {
    border-color: #d83b01;
    color: #d83b01;
    background: rgba(216, 59, 1, 0.08);
  }

  .send-error {
    color: #d83b01;
    font-size: 12px;
    margin-right: 8px;
    align-self: center;
  }

  .chip-remove {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    color: var(--text-tertiary);
    cursor: pointer;
    padding: 0;
  }

  .chip-remove:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .chip-input {
    flex: 1;
    min-width: 80px;
    font-size: 13px;
    padding: 4px 0;
    color: var(--text-primary);
    border: none;
    outline: none;
    background: transparent;
  }

  .chip-input::placeholder {
    color: var(--text-tertiary);
  }

  .field-input {
    width: 100%;
  }
  
  .field-extras {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
    align-self: flex-start;
    padding-top: 6px;
  }

  .field-toggle {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 2px 8px;
    border-radius: 3px;
  }

  .field-toggle:hover {
    background: var(--bg-hover);
    color: var(--accent);
  }

  /* ── Autocomplete Suggestions ── */
  .suggestions-dropdown {
    position: absolute;
    left: 24px;
    right: 24px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: var(--shadow-lg);
    z-index: 100;
    max-height: 240px;
    overflow-y: auto;
    padding: 4px 0;
  }

  .suggestion-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 12px;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
  }

  .suggestion-item:hover,
  .suggestion-item.highlighted {
    background: var(--bg-hover);
  }

  .suggestion-avatar {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
    overflow: hidden;
  }

  .suggestion-avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .suggestion-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .suggestion-name {
    font-weight: 500;
    display: flex;
    align-items: center;
    gap: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .suggestion-star {
    flex-shrink: 0;
  }

  .suggestion-email {
    font-size: 11px;
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .suggestion-source {
    font-size: 10px;
    color: var(--text-tertiary);
    flex-shrink: 0;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  /* ── Formatting Toolbar ── */
  .compose-toolbar {
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 6px 0;
    margin: 0 24px;
    border-bottom: 1px solid var(--border-light);
    flex-shrink: 0;
  }

  .toolbar-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
    transition: background 0.1s;
  }

  .toolbar-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .toolbar-separator {
    width: 1px;
    height: 18px;
    background: var(--border-light);
    margin: 0 4px;
    flex-shrink: 0;
  }

  /* ── Attachments ── */
  .attachments-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 24px;
    border-bottom: 1px solid var(--border-light);
    flex-shrink: 0;
  }

  .attachment-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px 4px 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-light);
    border-radius: 4px;
    font-size: 12px;
    color: var(--text-primary);
    max-width: 220px;
  }

  .attachment-chip-icon {
    flex-shrink: 0;
    color: var(--text-secondary);
  }

  .attachment-chip-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attachment-chip-remove {
    flex-shrink: 0;
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: 3px;
    cursor: pointer;
    color: var(--text-secondary);
    padding: 0;
  }

  .attachment-chip-remove:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* ── Body ── */
  .compose-body-wrap {
    flex: 1;
    overflow: hidden;
    padding: 0 24px;
  }

  .compose-body {
    width: 100%;
    height: 100%;
    border: none;
    outline: none;
    font-family: inherit;
    font-size: 14px;
    line-height: 1.6;
    color: var(--text-primary);
    padding: 12px 0;
    background: transparent;
    overflow-y: auto;
    word-wrap: break-word;
  }

  .compose-body:empty::before {
    content: attr(data-placeholder);
    color: var(--text-tertiary);
    pointer-events: none;
  }

  .compose-body :global(.quote-block) {
    border-left: 3px solid var(--accent, #0078d4);
    padding-left: 16px;
    margin-top: 4px;
    color: #666;
  }

  .compose-body :global(.quote-block ul),
  .compose-body :global(.quote-block ol) {
    padding-left: 20px;
    margin: 4px 0;
  }

  .compose-body :global(.quote-block li) {
    margin: 0;
    padding: 0;
  }

  /* Restore list styles inside editor (global reset strips them) */
  .compose-body :global(ul),
  .compose-body :global(ol) {
    padding-left: 24px;
    margin: 4px 0;
  }

  .compose-body :global(ul) {
    list-style-type: disc;
  }

  .compose-body :global(ol) {
    list-style-type: decimal;
  }

  .compose-body :global(li) {
    margin: 2px 0;
  }

  .compose-body :global(hr) {
    margin-block-start: 0.5em;
    margin-block-end: 0.5em;
  }
  /* ── Footer ── */
  .compose-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    border-top: 1px solid var(--border-light);
    flex-shrink: 0;
  }

  .send-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 20px;
    background: var(--accent);
    color: var(--text-on-accent);
    font-size: 13px;
    font-weight: 600;
    border-radius: 5px;
    transition: background 0.1s;
    outline: none;
  }

  .send-btn:hover {
    background: var(--accent-hover);
  }

  .send-btn:active {
    background: var(--accent-active);
  }

  .send-btn:focus {
    background: var(--accent-active);
  }

  /* ── Insert Link Dialog ── */
  .link-dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.25);
    z-index: 9999;
  }

  .link-dialog {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 10000;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow-lg);
    padding: 16px 20px;
    width: 360px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .link-dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .link-dialog-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .link-dialog-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    color: var(--text-tertiary);
    transition: background 0.1s, color 0.1s;
  }

  .link-dialog-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .link-dialog-input {
    width: 100%;
    padding: 6px 10px;
    font-size: 13px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-primary);
    color: var(--text-primary);
    outline: none;
    transition: border-color 0.15s, box-shadow 0.15s;
  }

  .link-dialog-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .link-dialog-input::placeholder {
    color: var(--text-tertiary);
  }

  .link-dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .link-dialog-btn {
    padding: 5px 14px;
    font-size: 13px;
    border-radius: 4px;
    color: var(--text-secondary);
    border: 1px solid var(--border);
    transition: background 0.1s;
  }

  .link-dialog-btn:hover {
    background: var(--bg-hover);
  }

  .link-dialog-btn.primary {
    background: var(--accent);
    color: var(--text-on-accent);
    border-color: var(--accent);
    font-weight: 600;
  }

  .link-dialog-btn.primary:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  /* ── Emoji Picker ── */
  .emoji-overlay {
    position: fixed;
    inset: 0;
    z-index: 9999;
  }

  .emoji-picker {
    position: fixed;
    width: 320px;
    max-height: 340px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0,0,0,0.14);
    z-index: 10000;
    display: flex;
    flex-direction: column;
  }

  .emoji-picker-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 6px;
    flex-shrink: 0;
  }

  .emoji-picker-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .emoji-picker-close {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
    background: none;
    border: none;
    cursor: pointer;
  }

  .emoji-picker-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .emoji-picker-body {
    overflow-y: auto;
    padding: 0 12px 10px;
    flex: 1;
  }

  .emoji-group {
    margin-bottom: 8px;
  }

  .emoji-group-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 6px 0 4px;
    position: sticky;
    top: 0;
    background: var(--bg-primary);
  }

  .emoji-grid {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 2px;
  }

  .emoji-btn {
    width: 34px;
    height: 34px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 18px;
    border: none;
    background: none;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.1s;
  }

  .emoji-btn:hover {
    background: var(--bg-hover);
  }

  /* ── Discard Confirmation Dialog ── */
  .discard-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    z-index: 10000;
  }

  .discard-dialog {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 24px;
    z-index: 10001;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
    min-width: 320px;
  }

  .discard-dialog-text {
    margin: 0 0 20px;
    font-size: 14px;
    color: var(--text-primary);
    line-height: 1.5;
  }

  .discard-dialog-actions {
    display: flex;
    justify-content: center;
    gap: 8px;
  }

  .discard-dialog-btn {
    padding: 6px 16px;
    border-radius: 4px;
    font-size: 13px;
    cursor: pointer;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    outline: none;
  }

  .discard-dialog-btn:hover {
    color: white;
    background: var(--bg-hover);
  }

  .discard-dialog-btn:focus {
    color: white;
    background: #777;
  }

  .discard-dialog-btn.danger {
    background-color: #4d0303;
    border-color: #4d0303;
  }

  .discard-dialog-btn.danger:hover {
    color: white;
    background: #b52a2d;
  }

  .discard-dialog-btn.danger:focus {
    color: white;
    background: #d13438;
  }
</style>
