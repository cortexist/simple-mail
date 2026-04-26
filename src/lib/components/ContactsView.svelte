<script lang="ts">
  import { untrack } from 'svelte';
  import type { FullContact, ContactList, ContactListMember } from '$lib/types';
  import { toggleContactFavorite } from '$lib/data/dataService';
  import { t } from '$lib/i18n/index.svelte';
  import { isLikelyEmail } from '$lib/utils';

  interface Props {
    contacts: FullContact[];
    onSaveContact?: (contact: FullContact) => void;
    onDeleteContact?: (contactId: string) => void;
    onEmailContact?: (email: string, name: string) => void;
    requestNewContact?: number;
    requestNewContactList?: number;
    requestEditContact?: number;
    requestDeleteContact?: number;
    onResetContactTriggers?: () => void;
    onSelectedContactChange?: (contact: FullContact | null) => void;
    contactLists?: ContactList[];
    onSaveContactList?: (list: ContactList) => void;
    onDeleteContactList?: (listId: string) => void;
    onListModeChange?: (isListMode: boolean) => void;
    onToggleFavorite?: (contactId: string, isFavorite: boolean) => void;
    onMeetContact?: (contact: FullContact) => void;
    onCallContact?: (phone: string) => void;
    onFocusPaneChange?: (pane: 'nav' | 'list' | 'detail' | null) => void;
    requestFocusPane?: 'nav' | 'list' | 'detail' | 'none' | null;
    searchQuery?: string;
    selectedContactId?: string;
    onSelectedContactIdChange?: (id: string) => void;
    selectedListId?: string;
    onSelectedListIdChange?: (id: string) => void;
    filteredContactsList?: FullContact[];
    /** Global set of muted (ignored) email addresses, lowercased. Owned by the parent so all views stay in sync. */
    mutedAddresses: Set<string>;
    /** Toggle an address's muted state. Parent persists and updates the set. */
    onToggleMute: (email: string) => void | Promise<void>;
  }

  let { contacts, onSaveContact, onDeleteContact, onEmailContact, requestNewContact, requestNewContactList, requestEditContact, requestDeleteContact, onResetContactTriggers, onSelectedContactChange, contactLists = [], onSaveContactList, onDeleteContactList, onListModeChange, onToggleFavorite, onMeetContact, onCallContact, onFocusPaneChange, requestFocusPane = null, searchQuery = '', selectedContactId: selectedContactIdProp = '', onSelectedContactIdChange, selectedListId: selectedListIdProp = '', onSelectedListIdChange, filteredContactsList = $bindable([]), mutedAddresses, onToggleMute }: Props = $props();

  let selectedCategory = $state<'all' | 'lists'>('all');
  let selectedContactId = $state<string>(untrack(() => selectedContactIdProp));
  let activePane = $state<'nav' | 'list' | 'detail' | null>(null);
  let detailPaneEl = $state<HTMLDivElement | undefined>();
  const navCategories: Array<'all' | 'lists'> = ['all', 'lists'];
  let favoritesExpanded = $state(true);

  // Muted contacts — state lives in the parent and is passed in; these locals
  // just adapt the shared set to per-contact checks and the toggle button.
  let showMuted = $state(false);

  function isMuted(contact: FullContact): boolean {
    return mutedAddresses.has(contact.email.toLowerCase());
  }

  async function toggleFavorite(contact: FullContact) {
    const newVal = !contact.isFavorite;
    await toggleContactFavorite(contact.id, newVal);
    onToggleFavorite?.(contact.id, newVal);
  }

  function toggleMute(contact: FullContact) {
    onToggleMute(contact.email.toLowerCase());
  }

  $effect(() => { onFocusPaneChange?.(activePane); });
  $effect(() => { onSelectedContactIdChange?.(selectedContactId); });
  $effect(() => { selectedContactId = selectedContactIdProp; });
  $effect(() => { filteredContactsList = filteredContacts; });
  $effect(() => { onSelectedListIdChange?.(selectedListId); });

  $effect(() => {
    if (requestFocusPane === 'none') activePane = null;
    else if (requestFocusPane) activePane = requestFocusPane;
  });

  let categoryContacts = $derived(contacts);

  let visibleContacts = $derived(
    showMuted ? categoryContacts : categoryContacts.filter((c) => !isMuted(c))
  );

  let filteredContacts = $derived(
    searchQuery
      ? visibleContacts.filter(
          (c) =>
            c.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
            c.email.toLowerCase().includes(searchQuery.toLowerCase()) ||
            (c.organization ?? '').toLowerCase().includes(searchQuery.toLowerCase())
        )
      : visibleContacts
  );

  // Counts for nav categories
  let allContactsCount = $derived(showMuted ? contacts.length : contacts.filter((c) => !isMuted(c)).length);

  let selectedContact = $derived(
    filteredContacts.find((c) => c.id === selectedContactId) ?? null
  );

  // Group contacts by first letter
  let groupedContacts = $derived(() => {
    const groups: Record<string, FullContact[]> = {};
    for (const c of filteredContacts) {
      const letter = c.name.charAt(0).toUpperCase();
      if (!groups[letter]) groups[letter] = [];
      groups[letter].push(c);
    }
    return Object.entries(groups).sort(([a], [b]) => a.localeCompare(b));
  });

  function selectContact(contact: FullContact) {
    selectedContactId = contact.id;
    activePane = 'list';
  }

  function switchCategory(cat: 'all' | 'lists') {
    selectedCategory = cat;
    onListModeChange?.(cat === 'lists');
  }

  let favoriteContacts = $derived(filteredContacts.filter((c) => c.isFavorite));

  // Auto-select first contact if none selected
  $effect(() => {
    if (!selectedContactId && contacts.length > 0) {
      selectedContactId = contacts[0].id;
    }
  });

  // Notify parent when selected contact changes (lists are never read-only, but disable when none selected)
  $effect(() => {
    if (selectedCategory === 'lists') {
      onSelectedContactChange?.(selectedList ? { isReadOnly: false } as any : { isReadOnly: true } as any);
    } else {
      onSelectedContactChange?.(selectedContact);
    }
  });

  function normalizePhone(value: string): string {
    const trimmed = value.trim();
    if (!trimmed) return trimmed;

    // International: starts with '+'
    if (trimmed.startsWith('+')) {
      const digits = trimmed.replace(/\D/g, '');
      // +1 US/Canada
      if (digits.startsWith('1') && digits.length === 11) {
        return `+1 (${digits.slice(1, 4)}) ${digits.slice(4, 7)}-${digits.slice(7)}`;
      }
      // Generic international: keep as-is but normalize spacing
      return trimmed.replace(/[\s\-().]+/g, ' ').trim();
    }

    const digits = trimmed.replace(/\D/g, '');

    // US 10-digit
    if (digits.length === 10) {
      return `(${digits.slice(0, 3)}) ${digits.slice(3, 6)}-${digits.slice(6)}`;
    }

    // US 11-digit with leading 1
    if (digits.length === 11 && digits.startsWith('1')) {
      return `+1 (${digits.slice(1, 4)}) ${digits.slice(4, 7)}-${digits.slice(7)}`;
    }

    // Fallback: return as-is
    return trimmed;
  }

  // ── Contact Edit/Create Modal ──
  let showContactModal = $state(false);
  let editingContactId = $state<string | null>(null);
  let ctModalEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (!showContactModal) return;
    const timer = setTimeout(() => {
      (document.getElementById('ct-first') as HTMLInputElement | null)?.focus();
    }, 0);
    function trapTab(e: KeyboardEvent) {
      if (e.key !== 'Tab' || !ctModalEl) return;
      const focusable = Array.from(ctModalEl.querySelectorAll<HTMLElement>(
        'button:not([disabled]):not([tabindex="-1"]), input:not([disabled]):not([tabindex="-1"]):not([type="file"]), select:not([disabled]), textarea:not([disabled])'
      ));
      if (!focusable.length) { e.preventDefault(); return; }
      const first = focusable[0], last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (e.shiftKey) {
        if (!ctModalEl.contains(active) || active === first) { e.preventDefault(); last.focus(); }
      } else {
        if (!ctModalEl.contains(active) || active === last) { e.preventDefault(); first.focus(); }
      }
    }
    document.addEventListener('keydown', trapTab, true);
    return () => { clearTimeout(timer); document.removeEventListener('keydown', trapTab, true); };
  });

  const AVATAR_COLORS = ['#0078d4', '#498205', '#8764b8', '#ca5010', '#c50f1f', '#038387', '#6b69d6', '#bf0077'];

  let ctFirstName = $state('');
  let ctLastName = $state('');
  let ctMiddleName = $state('');
  let ctPrefix = $state('');
  let ctSuffix = $state('');
  let ctOrganization = $state('');
  let ctJobTitle = $state('');
  let ctDepartment = $state('');
  let ctBirthday = $state('');
  let ctNotes = $state('');
  let ctIsFavorite = $state(false);
  let ctPhotoUrl = $state<string | undefined>(undefined);

  // Multi-value lists — UI is bound to these arrays with + buttons to append rows
  type EmailRow = { email: string; label: string; isDefault: boolean };
  type PhoneRow = { number: string; label: string; subtypes: string[]; isDefault: boolean };
  type AddressRow = { street: string; city: string; region: string; postalCode: string; country: string; label: string; isDefault: boolean };

  let ctEmails = $state<EmailRow[]>([]);
  let ctPhones = $state<PhoneRow[]>([]);
  let ctAddresses = $state<AddressRow[]>([]);

  const EMAIL_LABELS = ['home', 'work', 'other'] as const;
  const PHONE_LABELS = ['home', 'work', 'cell', 'other'] as const;
  const ADDRESS_LABELS = ['home', 'work', 'other'] as const;
  const PHONE_SUBTYPES = ['voice', 'text', 'fax', 'video', 'pager'] as const;

  function moveRow<T>(items: T[], i: number, dir: -1 | 1): T[] {
    const j = i + dir;
    if (j < 0 || j >= items.length) return items;
    const next = items.slice();
    [next[i], next[j]] = [next[j], next[i]];
    return next;
  }

  function addEmailRow() {
    ctEmails = [...ctEmails, { email: '', label: 'work', isDefault: false }];
  }
  function removeEmailRow(i: number) {
    ctEmails = ctEmails.filter((_, idx) => idx !== i);
  }
  function moveEmailRow(i: number, dir: -1 | 1) { ctEmails = moveRow(ctEmails, i, dir); }

  function addPhoneRow() {
    ctPhones = [...ctPhones, { number: '', label: 'work', subtypes: ['voice'], isDefault: false }];
  }
  function removePhoneRow(i: number) {
    ctPhones = ctPhones.filter((_, idx) => idx !== i);
  }
  function movePhoneRow(i: number, dir: -1 | 1) { ctPhones = moveRow(ctPhones, i, dir); }
  function togglePhoneSubtype(i: number, subtype: string) {
    const p = ctPhones[i];
    if (!p) return;
    const has = p.subtypes.includes(subtype);
    p.subtypes = has ? p.subtypes.filter(s => s !== subtype) : [...p.subtypes, subtype];
  }

  function addAddressRow() {
    ctAddresses = [...ctAddresses, { street: '', city: '', region: '', postalCode: '', country: '', label: 'home', isDefault: false }];
  }
  function removeAddressRow(i: number) {
    ctAddresses = ctAddresses.filter((_, idx) => idx !== i);
  }
  function moveAddressRow(i: number, dir: -1 | 1) { ctAddresses = moveRow(ctAddresses, i, dir); }

  function addressIsEmpty(a: AddressRow): boolean {
    return !a.street.trim() && !a.city.trim() && !a.region.trim() && !a.postalCode.trim() && !a.country.trim();
  }

  function formatAddress(a: { street?: string; city?: string; region?: string; postalCode?: string; country?: string }): string {
    return [a.street, a.city, a.region, a.postalCode, a.country].filter(s => s && s.trim()).join(', ');
  }

  /** Translate a label key, falling back to capitalised label if the key is missing. */
  function labelText(key: string): string {
    const full = t(`contacts.label.${key}`);
    if (!full || full === `contacts.label.${key}`) {
      return key ? key[0].toUpperCase() + key.slice(1) : '';
    }
    return full;
  }

  /** Build FN from name parts; fall back to organization */
  function buildFullName(): string {
    const parts = [ctPrefix, ctFirstName, ctMiddleName, ctLastName, ctSuffix]
      .map(s => s.trim()).filter(Boolean);
    return parts.join(' ') || ctOrganization.trim();
  }

  let ctCanSave = $derived(
    !!buildFullName() &&
    ctEmails.some(e => e.email.trim()) &&
    ctEmails.every(e => !e.email.trim() || isLikelyEmail(e.email))
  );
  let photoFileInput = $state<HTMLInputElement>(undefined!);

  function openNewContact() {
    editingContactId = null;
    ctFirstName = ''; ctLastName = ''; ctMiddleName = ''; ctPrefix = ''; ctSuffix = '';
    ctOrganization = '';
    ctJobTitle = '';
    ctDepartment = '';
    ctBirthday = '';
    ctNotes = '';
    ctIsFavorite = false;
    ctPhotoUrl = undefined;
    ctEmails = [{ email: '', label: 'work', isDefault: true }];
    ctPhones = [];
    ctAddresses = [];
    showContactModal = true;
  }

  function openEditContact(contact: FullContact) {
    editingContactId = contact.id;
    ctFirstName = contact.firstName ?? '';
    ctLastName = contact.lastName ?? '';
    ctMiddleName = contact.middleName ?? '';
    ctPrefix = contact.prefix ?? '';
    ctSuffix = contact.suffix ?? '';
    ctOrganization = contact.organization ?? '';
    ctJobTitle = contact.jobTitle ?? '';
    ctDepartment = contact.department ?? '';
    ctBirthday = contact.birthday ?? '';
    ctNotes = contact.notes ?? '';
    ctIsFavorite = contact.isFavorite;
    ctPhotoUrl = contact.photoUrl;
    const sortDefaultFirst = <T extends { isDefault?: boolean }>(arr: T[]): T[] => {
      const def = arr.findIndex(x => x.isDefault);
      if (def <= 0) return arr;
      return [arr[def], ...arr.filter((_, i) => i !== def)];
    };
    ctEmails = sortDefaultFirst((contact.emails ?? []).map(e => ({ email: e.email, label: e.label || 'work', isDefault: !!e.isDefault })));
    if (ctEmails.length === 0) ctEmails = [{ email: '', label: 'work', isDefault: true }];
    ctPhones = sortDefaultFirst((contact.phones ?? []).map(p => ({
      number: p.number,
      label: p.label || 'work',
      subtypes: p.subtypes ?? [],
      isDefault: !!p.isDefault,
    })));
    ctAddresses = sortDefaultFirst((contact.addresses ?? []).map(a => ({
      street: a.street ?? '',
      city: a.city ?? '',
      region: a.region ?? '',
      postalCode: a.postalCode ?? '',
      country: a.country ?? '',
      label: a.label || 'home',
      isDefault: !!a.isDefault,
    })));
    showContactModal = true;
  }

  function closeContactModal() {
    showContactModal = false;
  }

  function triggerPhotoPicker() {
    photoFileInput?.click();
  }

  function handlePhotoFileChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file || !file.type.startsWith('image/')) return;
    const reader = new FileReader();
    reader.onload = () => {
      ctPhotoUrl = reader.result as string;
    };
    reader.readAsDataURL(file);
    input.value = '';
  }

  function removePhoto() {
    ctPhotoUrl = undefined;
  }

  function contactByEmail(email: string) {
    const key = email.toLowerCase();
    return contacts.find(c =>
      c.email.toLowerCase() === key ||
      (c.emails ?? []).some(e => e.email.toLowerCase() === key)
    );
  }

  function getInitials(name: string): string {
    return name.split(' ').map((w) => w[0]).join('').toUpperCase().slice(0, 2);
  }

  function saveContact() {
    const fullName = buildFullName();
    const cleanedEmails = ctEmails
      .map(e => ({ email: e.email.trim(), label: e.label || 'work', isDefault: false }))
      .filter(e => e.email);
    if (!fullName || cleanedEmails.length === 0) return;
    cleanedEmails.forEach((e, i) => { e.isDefault = i === 0; });

    const cleanedPhones = ctPhones
      .map(p => ({
        number: p.number.trim(),
        label: p.label || 'work',
        subtypes: p.subtypes.filter(s => PHONE_SUBTYPES.includes(s as typeof PHONE_SUBTYPES[number])),
        isDefault: false,
      }))
      .filter(p => p.number);
    cleanedPhones.forEach((p, i) => { p.isDefault = i === 0; });

    const cleanedAddresses = ctAddresses
      .map(a => ({
        street: a.street.trim(),
        city: a.city.trim(),
        region: a.region.trim(),
        postalCode: a.postalCode.trim(),
        country: a.country.trim(),
        label: a.label || 'home',
        isDefault: false,
      }))
      .filter(a => a.street || a.city || a.region || a.postalCode || a.country);
    cleanedAddresses.forEach((a, i) => { a.isDefault = i === 0; });

    const primaryEmail = cleanedEmails[0].email;
    const existing = editingContactId ? contacts.find((c) => c.id === editingContactId) : null;
    const ct: FullContact = {
      id: editingContactId ?? crypto.randomUUID(),
      name: fullName,
      firstName: ctFirstName.trim() || undefined,
      lastName: ctLastName.trim() || undefined,
      middleName: ctMiddleName.trim() || undefined,
      prefix: ctPrefix.trim() || undefined,
      suffix: ctSuffix.trim() || undefined,
      email: primaryEmail,
      emails: cleanedEmails,
      phones: cleanedPhones,
      addresses: cleanedAddresses,
      initials: existing?.initials ?? getInitials(fullName),
      color: existing?.color ?? AVATAR_COLORS[Math.floor(Math.random() * AVATAR_COLORS.length)],
      organization: ctOrganization.trim() || undefined,
      jobTitle: ctJobTitle.trim() || undefined,
      department: ctDepartment.trim() || undefined,
      birthday: ctBirthday.trim() || undefined,
      notes: ctNotes.trim() || undefined,
      isFavorite: ctIsFavorite,
      photoUrl: ctPhotoUrl || undefined,
    };
    onSaveContact?.(ct);
    selectedContactId = ct.id;
    showContactModal = false;
  }

  function deleteContact() {
    if (editingContactId) {
      onDeleteContact?.(editingContactId);
      selectedContactId = '';
      showContactModal = false;
    }
  }

  // ── Contact List Edit/Create ──
  let selectedListId = $state<string>(untrack(() => selectedListIdProp));
  let selectedList = $derived(contactLists.find(l => l.id === selectedListId) ?? null);

  let filteredLists = $derived(
    searchQuery
      ? contactLists.filter(l => l.name.toLowerCase().includes(searchQuery.toLowerCase()))
      : contactLists
  );

  let showListModal = $state(false);
  let editingListId = $state<string | null>(null);
  let clName = $state('');
  let clMembers = $state<ContactListMember[]>([]);
  let clNewName = $state('');
  let clNewEmail = $state('');
  let showContactPicker = $state(false);

  function openNewList() {
    editingListId = null;
    clName = '';
    clMembers = [];
    clNewName = '';
    clNewEmail = '';
    showListModal = true;
  }

  function openEditList(list: ContactList) {
    editingListId = list.id;
    clName = list.name;
    clMembers = [...list.members];
    clNewName = '';
    clNewEmail = '';
    showListModal = true;
  }

  function closeListModal() {
    showListModal = false;
    showContactPicker = false;
  }

  function addListMember() {
    if (!clNewEmail.trim()) return;
    const email = clNewEmail.trim();
    if (!isLikelyEmail(email)) return;
    if (clMembers.some(m => m.email.toLowerCase() === email.toLowerCase())) return;
    clMembers = [...clMembers, { name: clNewName.trim(), email }];
    clNewName = '';
    clNewEmail = '';
  }

  function addContactAsMember(contact: FullContact) {
    if (clMembers.some(m => m.email.toLowerCase() === contact.email.toLowerCase())) return;
    clMembers = [...clMembers, { name: contact.name, email: contact.email }];
  }

  function removeListMember(idx: number) {
    clMembers = clMembers.filter((_, i) => i !== idx);
  }

  function saveList() {
    if (!clName.trim()) return;
    const list: ContactList = {
      id: editingListId ?? crypto.randomUUID(),
      name: clName.trim(),
      members: clMembers,
    };
    onSaveContactList?.(list);
    selectedListId = list.id;
    showListModal = false;
    showContactPicker = false;
  }

  function deleteList() {
    if (editingListId) {
      onDeleteContactList?.(editingListId);
      selectedListId = '';
      showListModal = false;
      showContactPicker = false;
    }
  }

  // React to external requests
  $effect(() => {
    if (requestNewContact && requestNewContact > 0) {
      openNewContact();
      onResetContactTriggers?.();
    }
  });

  $effect(() => {
    if (requestNewContactList && requestNewContactList > 0) {
      openNewList();
      onResetContactTriggers?.();
    }
  });

  $effect(() => {
    if (requestEditContact && requestEditContact > 0) {
      if (selectedCategory === 'lists' && selectedList) {
        openEditList(selectedList);
      } else if (selectedContact && !selectedContact.isReadOnly) {
        openEditContact(selectedContact);
      }
      onResetContactTriggers?.();
    }
  });

  $effect(() => {
    if (requestDeleteContact && requestDeleteContact > 0) {
      if (selectedCategory === 'lists' && selectedList) {
        onDeleteContactList?.(selectedList.id);
        selectedListId = '';
      } else if (selectedContact && !selectedContact.isReadOnly) {
        onDeleteContact?.(selectedContact.id);
        selectedContactId = '';
      }
      onResetContactTriggers?.();
    }
  });

  export function emailSelected() {
    if (selectedContact) onEmailContact?.(selectedContact.email, selectedContact.name);
  }

  export function meetSelected() {
    if (selectedContact) onMeetContact?.(selectedContact);
  }

  export function callSelected() {
    const primary = primaryPhone(selectedContact);
    if (primary) onCallContact?.(primary);
  }

  function primaryPhone(c: FullContact | null | undefined): string | undefined {
    if (!c) return undefined;
    const p = c.phones?.find(p => p.isDefault) ?? c.phones?.[0];
    return p?.number;
  }

  export function toggleSelectedFavorite() {
    if (selectedContact) toggleFavorite(selectedContact);
  }

  export function toggleShowMuted() {
    showMuted = !showMuted;
  }

  /** Navigate contacts via arrow keys — called from global keydown handler */
  export function navigateArrow(key: string) {
    if (key === 'ArrowRight' && activePane === 'nav') {
      const hasItems = selectedCategory === 'lists' ? filteredLists.length > 0 : filteredContacts.length > 0;
      if (!hasItems) return;
      activePane = 'list';
      return;
    }
    if (key === 'ArrowLeft' && activePane === 'list') {
      activePane = 'nav';
      return;
    }

    const down = key === 'ArrowDown';
    if (key !== 'ArrowUp' && key !== 'ArrowDown') return;

    if (activePane === 'nav') {
      const curIdx = navCategories.indexOf(selectedCategory);
      const nextIdx = down ? Math.min(curIdx + 1, navCategories.length - 1) : Math.max(curIdx - 1, 0);
      if (curIdx !== nextIdx) switchCategory(navCategories[nextIdx]);
    } else {
      // Navigate items in the list pane
      if (selectedCategory === 'lists') {
        const lists = filteredLists;
        if (lists.length === 0) return;
        const curIdx = lists.findIndex(l => l.id === selectedListId);
        const nextIdx = down
          ? Math.min((curIdx === -1 ? -1 : curIdx) + 1, lists.length - 1)
          : Math.max((curIdx === -1 ? 0 : curIdx) - 1, 0);
        selectedListId = lists[nextIdx].id;
      } else {
        const list = filteredContacts;
        if (list.length === 0) return;
        const curIdx = list.findIndex(c => c.id === selectedContactId);
        const nextIdx = down
          ? Math.min((curIdx === -1 ? -1 : curIdx) + 1, list.length - 1)
          : Math.max((curIdx === -1 ? 0 : curIdx) - 1, 0);
        selectedContactId = list[nextIdx].id;
      }
    }
  }

</script>

{#snippet contactRow(contact: FullContact)}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="contact-item"
    class:selected={contact.id === selectedContactId}
    class:muted={isMuted(contact)}
    onclick={() => selectContact(contact)}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectContact(contact); } }}
    tabindex="-1"
    role="option"
    aria-selected={contact.id === selectedContactId}
  >
    {#if contact.photoUrl}
      <img class="contact-avatar-sm contact-avatar-img" src={contact.photoUrl} alt={contact.name} />
    {:else}
      <span class="contact-avatar-sm" style="background: {contact.color}">{contact.initials}</span>
    {/if}
    <div class="contact-item-info">
      <span class="contact-item-name">{contact.name}</span>
      <span class="contact-item-detail">{contact.jobTitle ?? contact.email}</span>
    </div>
    <div class="contact-hover-actions">
      <button class="contact-hover-btn" class:selected={isMuted(contact)} tabindex="-1" aria-label="Mute" data-tooltip={isMuted(contact) ? t('contacts.unmute') : t('contacts.mute')} data-tooltip-position="bottom-end" onclick={(e) => { e.stopPropagation(); toggleMute(contact); }}>
        {#if isMuted(contact)}
          <svg width="16" height="16" viewBox="0 0 16 16">
            <path fill="currentColor" d="M8 2.75v10.5a.751.751 0 0 1-1.238.57L3.473 11H1.75A1.75 1.75 0 0 1 0 9.25v-2.5C0 5.784.784 5 1.75 5h1.722l3.29-2.82A.75.75 0 0 1 8 2.75m3.28 2.47L13 6.94l1.72-1.72a.75.75 0 0 1 1.042.018a.75.75 0 0 1 .018 1.042L14.06 8l1.72 1.72a.749.749 0 0 1-.326 1.275a.75.75 0 0 1-.734-.215L13 9.06l-1.72 1.72a.749.749 0 0 1-1.275-.326a.75.75 0 0 1 .215-.734L11.94 8l-1.72-1.72a.749.749 0 0 1 .326-1.275a.75.75 0 0 1 .734.215m-7.042 1.1a.75.75 0 0 1-.488.18h-2a.25.25 0 0 0-.25.25v2.5c0 .138.112.25.25.25h2c.179 0 .352.064.488.18L6.5 11.62V4.38Z"/>
          </svg>
        {:else}
          <svg width="16" height="16" viewBox="0 0 16 16">
            <path fill="currentColor" d="M7.563 2.069A.75.75 0 0 1 8 2.75v10.5a.751.751 0 0 1-1.238.57L3.472 11H1.75A1.75 1.75 0 0 1 0 9.25v-2.5C0 5.784.784 5 1.75 5h1.723l3.289-2.82a.75.75 0 0 1 .801-.111M6.5 4.38L4.238 6.319a.75.75 0 0 1-.488.181h-2a.25.25 0 0 0-.25.25v2.5c0 .138.112.25.25.25h2c.179 0 .352.064.488.18L6.5 11.62Zm6.096-2.038a.75.75 0 0 1 1.06 0a8 8 0 0 1 0 11.314a.75.75 0 0 1-1.042-.018a.75.75 0 0 1-.018-1.042a6.5 6.5 0 0 0 0-9.193a.75.75 0 0 1 0-1.06Zm-1.06 2.121l-.001.001a5 5 0 0 1 0 7.07a.749.749 0 0 1-1.275-.326a.75.75 0 0 1 .215-.734a3.5 3.5 0 0 0 0-4.95a.75.75 0 1 1 1.061-1.061"/>
          </svg>
        {/if}
      </button>
      <button class="contact-hover-btn" class:selected={contact.isFavorite} tabindex="-1" aria-label="Favorite" data-tooltip={contact.isFavorite ? t('contacts.unfavorite') : t('contacts.favorite')} data-tooltip-position="bottom-end" onclick={(e) => { e.stopPropagation(); toggleFavorite(contact); }}>
        {#if contact.isFavorite}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
        {/if}
      </button>      
    </div>
  </div>
{/snippet}

<div class="contacts-view">
  <!-- 1st Level Nav -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <nav class="contacts-nav" class:active={activePane === 'nav'} onmousedown={() => (activePane = 'nav')}>
    <h2 class="contacts-nav-title">{t('contacts.contacts')}</h2>
    <button
      class="contacts-nav-item"
      class:selected={selectedCategory === 'all'}
      tabindex="-1"
      onclick={() => { switchCategory('all'); activePane = 'nav'; }}
    >
      <svg width="16" height="16" viewBox="0 0 24 24">
        <path fill="currentColor" d="M7.48 10.385c1.136 0 2.068.475 2.853.983c.387.25.774.534 1.12.777c.359.252.695.474 1.031.65c.166.083.363.23.556.22c.048-.002.242-.026.569-.42l.94-1.302c1.126-1.237 3.204-1.218 4.295.105l3.419 4.186c.271.463.3 1.02.097 1.507l-.12.237c-.44.718-1.132 1.867-2.045 2.83c-.906.957-2.139 1.845-3.674 1.846c-1.137 0-2.07-.475-2.854-.983c-.387-.25-.775-.535-1.121-.778a12 12 0 0 0-.777-.51l-.254-.141c-.322-.169-.43-.226-.555-.22c-.05.004-.248.03-.58.433l-.717 1.024v.002c-.99 1.405-3.062 1.555-4.275.405l-3.494-4.208a1.69 1.69 0 0 1-.133-1.968l.376-.609c.422-.67.982-1.498 1.667-2.22c.906-.957 2.14-1.846 3.676-1.846m0 1.485c-.935 0-1.801.544-2.596 1.383c-.734.774-1.299 1.68-1.857 2.583a.21.21 0 0 0 .012.247l3.264 3.96l.108.118c.574.54 1.58.459 2.035-.186l3.924-5.594a4 4 0 0 1-.575-.27c-.425-.223-.825-.49-1.194-.75c-.382-.268-.72-.516-1.076-.747c-.701-.454-1.338-.744-2.045-.744m10.216.474c-.546-.663-1.66-.62-2.144.067l.002.001l-.56.797l.009.006l-3.364 4.796c.264.093.466.213.566.265l.315.176c.308.181.602.381.879.575c.381.268.719.516 1.075.746c.702.455 1.34.744 2.047.744c.934 0 1.8-.543 2.595-1.381c.789-.832 1.404-1.846 1.856-2.584c.04-.093.04-.159-.012-.247zM7 3a3 3 0 1 1 0 6a3 3 0 0 1 0-6m10 0a3 3 0 1 1 0 6a3 3 0 0 1 0-6M7 4.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 0 0 0-3m10 0a1.5 1.5 0 1 0 0 3a1.5 1.5 0 0 0 0-3"/>
      </svg>
      {t('contacts.yourContacts')}
      {#if allContactsCount > 0}<span class="nav-count">{allContactsCount}</span>{/if}
    </button>
    <button
      class="contacts-nav-item"
      class:selected={selectedCategory === 'lists'}
      tabindex="-1"
      onclick={() => { switchCategory('lists'); activePane = 'nav'; }}
    >
    <svg height="16" width="16" viewBox="0 0 24 24">
      <g fill="currentColor">
        <path d="M14.4,13.2c-1,1.5-3.6,5.2-4.1,6l-0.7,1v0c-1,1.4-3.1,1.6-4.3,0.4l-3.5-4.2c-0.5-0.6-0.5-1.4-0.1-2l0.4-0.6
        c0.4-0.7,1-1.5,1.7-2.2c0.9-1,2.2-1.8,3.7-1.8c0.8,0,1.3-0.1,4.2,1.8C12.6,12.2,14.4,13.2,14.4,13.2z M7.5,9.7c1.1,0,2,0.5,2.7,1
        M7.4,11.2c-0.9,0-1.8,0.5-2.6,1.4c-0.7,0.8-1.3,1.7-1.9,2.6c-0.1,0.1,0,0.2,0,0.2l3.3,4l0.1,0.1c0.6,0.5,1.6,0.5,2-0.2l3.9-5.6
        c-0.2-0.1-0.4-0.2-0.6-0.3c-0.4-0.2-0.8-0.5-1.2-0.8c-0.4-0.3-0.7-0.5-1.1-0.7C8.8,11.5,8.1,11.2,7.4,11.2 M6.9,2.4c1.7,0,3,1.3,3,3
        s-1.3,3-3,3s-3-1.3-3-3S5.3,2.4,6.9,2.4 M16.9,2.4c1.7,0,3,1.3,3,3s-1.3,3-3,3s-3-1.3-3-3S15.3,2.4,16.9,2.4 M6.9,3.9
        c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5s1.5-0.7,1.5-1.5S7.8,3.9,6.9,3.9 M16.9,3.9c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5
        s1.5-0.7,1.5-1.5S17.8,3.9,16.9,3.9"/>
        <rect x="16.8" y="11.2" width="5.3" height="1.5"/>
        <rect x="14.9" y="15.5" width="7.2" height="1.5"/>
        <rect x="13.4" y="19.7" width="8.7" height="1.5"/>
      </g>
    </svg>
      {t('contacts.yourContactLists')}
      {#if contactLists.length > 0}<span class="nav-count">{contactLists.length}</span>{/if}
    </button>
  </nav>

  <!-- 2nd Level Nav — Contact/List List -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="contacts-list-pane" onmousedown={() => (activePane = 'list')}>
    {#if selectedCategory === 'lists'}
      <!-- Contact Lists -->
      <div class="contacts-scroll" class:active={activePane === 'list'}>
        {#each filteredLists as list}
          <button
            class="contact-item"
            class:selected={list.id === selectedListId}
            tabindex="-1"
            onclick={() => { selectedListId = list.id; activePane = 'list'; }}
          >
            <span class="contact-avatar-sm" style="background: var(--accent, #0078d4)">{getInitials(list.name)}</span>
            <div class="contact-item-info">
              <span class="contact-item-name">{list.name}</span>
              <span class="contact-item-detail">{list.members.length === 1 ? t('contacts.memberCount', { count: 1 }) : t('contacts.membersCount', { count: list.members.length })}</span>
            </div>
          </button>
        {/each}
        {#if filteredLists.length === 0}
          <div class="contacts-empty">
            <svg height="32" width="32" viewBox="0 0 24 24">
            <g fill="currentColor">
                <path d="M14.4,13.2c-1,1.5-3.6,5.2-4.1,6l-0.7,1v0c-1,1.4-3.1,1.6-4.3,0.4l-3.5-4.2c-0.5-0.6-0.5-1.4-0.1-2l0.4-0.6
                c0.4-0.7,1-1.5,1.7-2.2c0.9-1,2.2-1.8,3.7-1.8c0.8,0,1.3-0.1,4.2,1.8C12.6,12.2,14.4,13.2,14.4,13.2z M7.5,9.7c1.1,0,2,0.5,2.7,1
                M7.4,11.2c-0.9,0-1.8,0.5-2.6,1.4c-0.7,0.8-1.3,1.7-1.9,2.6c-0.1,0.1,0,0.2,0,0.2l3.3,4l0.1,0.1c0.6,0.5,1.6,0.5,2-0.2l3.9-5.6
                c-0.2-0.1-0.4-0.2-0.6-0.3c-0.4-0.2-0.8-0.5-1.2-0.8c-0.4-0.3-0.7-0.5-1.1-0.7C8.8,11.5,8.1,11.2,7.4,11.2 M6.9,2.4c1.7,0,3,1.3,3,3
                s-1.3,3-3,3s-3-1.3-3-3S5.3,2.4,6.9,2.4 M16.9,2.4c1.7,0,3,1.3,3,3s-1.3,3-3,3s-3-1.3-3-3S15.3,2.4,16.9,2.4 M6.9,3.9
                c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5s1.5-0.7,1.5-1.5S7.8,3.9,6.9,3.9 M16.9,3.9c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5
                s1.5-0.7,1.5-1.5S17.8,3.9,16.9,3.9"/>
                <rect x="16.8" y="11.2" width="5.3" height="1.5"/>
                <rect x="14.9" y="15.5" width="7.2" height="1.5"/>
                <rect x="13.4" y="19.7" width="8.7" height="1.5"/>
            </g>
            </svg>
            <p>{t('contacts.noContactLists')}</p>
          </div>
        {/if}
      </div>
    {:else}
      <!-- Contact List -->
      <div class="contacts-muted-toggle">
        <label class="muted-toggle-label">
          <button type="button" tabindex="-1" class="toggle-switch" aria-label={t('contacts.toggleMuted')} class:on={showMuted} onclick={() => showMuted = !showMuted} role="switch" aria-checked={showMuted}>
            <span class="toggle-knob"></span>
          </button>
          <span class="muted-toggle-text">{t('contacts.showMuted')}</span>
        </label>
      </div>
      <div class="contacts-scroll" class:active={activePane === 'list'}>
        {#if favoriteContacts.length > 0}
          <div class="contact-letter-group">
            <button type="button" class="contact-letter contact-section-toggle" onclick={() => favoritesExpanded = !favoritesExpanded}>
              <svg class="contact-section-chevron" class:expanded={favoritesExpanded} width="10" height="10" viewBox="0 0 12 12" fill="currentColor">
                <path d="M4.5 2l4 4-4 4" />
              </svg>
              <span>{t('contacts.favorites')}</span>
            </button>
            {#if favoritesExpanded}
              {#each favoriteContacts as contact (contact.id)}
                {@render contactRow(contact)}
              {/each}
            {/if}
          </div>
        {/if}
        {#each groupedContacts() as [letter, contactGroup]}
          <div class="contact-letter-group">
            <div class="contact-letter">{letter}</div>
            {#each contactGroup as contact (contact.id)}
              {@render contactRow(contact)}
            {/each}
          </div>
        {/each}
        {#if filteredContacts.length === 0}
          <div class="contacts-empty">
            <svg width="32" height="32" viewBox="0 0 32 32">
              <path fill="var(--text-tertiary)" d="M23 9A7 7 0 1 1 9 9a7 7 0 0 1 14 0m-2 0a5 5 0 1 0-10 0a5 5 0 0 0 10 0M7.5 18A3.5 3.5 0 0 0 4 21.5v.5c0 2.393 1.523 4.417 3.685 5.793C9.859 29.177 12.802 30 16 30s6.14-.823 8.315-2.207C26.477 26.417 28 24.393 28 22v-.5a3.5 3.5 0 0 0-3.5-3.5zM6 21.5A1.5 1.5 0 0 1 7.5 20h17a1.5 1.5 0 0 1 1.5 1.5v.5c0 1.473-.94 2.949-2.759 4.106C21.434 27.256 18.877 28 16 28s-5.434-.744-7.241-1.894C6.939 24.95 6 23.472 6 22z"/>
            </svg>
            <p>{t('contacts.noContacts')}</p>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Detail Pane -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="contact-detail-pane" bind:this={detailPaneEl} onmousedown={() => { if (activePane !== 'nav') activePane = 'list'; }}>
    {#if selectedCategory === 'lists' && selectedList}
      <div class="contact-detail-card">
        <div class="contact-hero">
          <div class="contact-hero-avatar" style="background: var(--accent, #0078d4)">
            {getInitials(selectedList.name)}
          </div>
          <div class="contact-hero-text">
            <h2 class="contact-hero-name">{selectedList.name}</h2>
            <p class="contact-hero-title">{selectedList.members.length} member{selectedList.members.length === 1 ? '' : 's'}</p>
          </div>
          <div class="contact-hero-actions">
            <button class="hero-action-btn" tabindex="-1" aria-label={t('contacts.editList')} data-tooltip={t('contacts.editList')} onclick={() => openEditList(selectedList!)}>
              <svg width="18" height="18" viewBox="0 0 24 24">
                <path fill="currentColor" d="M13.25 4a.75.75 0 0 1 0 1.5h-7A1.75 1.75 0 0 0 4.5 7.25v10.5c0 .966.784 1.75 1.75 1.75h10.5a1.75 1.75 0 0 0 1.75-1.75v-7a.75.75 0 0 1 1.5 0v7A3.25 3.25 0 0 1 16.75 21H6.25A3.25 3.25 0 0 1 3 17.75V7.25A3.25 3.25 0 0 1 6.25 4zm6.47-.78a.75.75 0 1 1 1.06 1.06L10.59 14.47L9 15l.53-1.59z"/>
              </svg>
            </button>
          </div>
        </div>

        <div class="contact-actions">
          <button class="contact-action-btn" tabindex="-1" aria-label={t('contacts.emailAll')} data-tooltip={t('contacts.emailAll')} onclick={() => {
            if (selectedList && selectedList.members.length > 0) {
              const first = selectedList.members[0];
              onEmailContact?.(selectedList.members.map(m => m.email).join('; '), selectedList.name);
            }
          }}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2" y="4" width="20" height="16" rx="2"/><path d="M2 7l10 6 10-6"/>
            </svg>
            <span>{t('contacts.emailAll')}</span>
          </button>
        </div>

        <div class="contact-info-section">
          <h3 class="contact-info-heading">{t('contacts.members')}</h3>
          {#each selectedList.members as member, i}
            {@const mc = contactByEmail(member.email)}
            <div class="list-member-row-detail">
              {#if mc?.photoUrl}
                <img class="contact-avatar-sm contact-avatar-img" src={mc.photoUrl} alt={member.name || member.email} />
              {:else}
                <span class="contact-avatar-sm" style="background: {mc?.color ?? 'var(--accent, #0078d4)'}">{getInitials(member.name || member.email)}</span>
              {/if}
              <div class="list-member-info">
                <span class="list-member-name">{member.name || member.email}</span>
                {#if member.name}
                  <span class="list-member-email">{member.email}</span>
                {/if}
              </div>
            </div>
          {/each}
          {#if selectedList.members.length === 0}
            <p class="contact-info-value" style="color: var(--text-tertiary)">{t('contacts.noMembersYet')}</p>
          {/if}
        </div>
      </div>
    {:else if selectedCategory === 'lists'}
      <div class="contact-empty-state">
        <svg height="48" width="48" viewBox="0 0 24 24">
          <g fill="currentColor">
            <path d="M14.4,13.2c-1,1.5-3.6,5.2-4.1,6l-0.7,1v0c-1,1.4-3.1,1.6-4.3,0.4l-3.5-4.2c-0.5-0.6-0.5-1.4-0.1-2l0.4-0.6
            c0.4-0.7,1-1.5,1.7-2.2c0.9-1,2.2-1.8,3.7-1.8c0.8,0,1.3-0.1,4.2,1.8C12.6,12.2,14.4,13.2,14.4,13.2z M7.5,9.7c1.1,0,2,0.5,2.7,1
            M7.4,11.2c-0.9,0-1.8,0.5-2.6,1.4c-0.7,0.8-1.3,1.7-1.9,2.6c-0.1,0.1,0,0.2,0,0.2l3.3,4l0.1,0.1c0.6,0.5,1.6,0.5,2-0.2l3.9-5.6
            c-0.2-0.1-0.4-0.2-0.6-0.3c-0.4-0.2-0.8-0.5-1.2-0.8c-0.4-0.3-0.7-0.5-1.1-0.7C8.8,11.5,8.1,11.2,7.4,11.2 M6.9,2.4c1.7,0,3,1.3,3,3
            s-1.3,3-3,3s-3-1.3-3-3S5.3,2.4,6.9,2.4 M16.9,2.4c1.7,0,3,1.3,3,3s-1.3,3-3,3s-3-1.3-3-3S15.3,2.4,16.9,2.4 M6.9,3.9
            c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5s1.5-0.7,1.5-1.5S7.8,3.9,6.9,3.9 M16.9,3.9c-0.8,0-1.5,0.7-1.5,1.5s0.7,1.5,1.5,1.5
            s1.5-0.7,1.5-1.5S17.8,3.9,16.9,3.9"/>
            <rect x="16.8" y="11.2" width="5.3" height="1.5"/>
            <rect x="14.9" y="15.5" width="7.2" height="1.5"/>
            <rect x="13.4" y="19.7" width="8.7" height="1.5"/>
          </g>
        </svg>
        <h3>{t('contacts.selectList')}</h3>
        <p>{t('contacts.selectListHint')}</p>
      </div>
    {:else if selectedContact}
      <div class="contact-detail-card">
        <!-- Header / Hero -->
        <div class="contact-hero">
          {#if selectedContact.photoUrl}
            <img class="contact-hero-avatar contact-hero-avatar-img" src={selectedContact.photoUrl} alt={selectedContact.name} />
          {:else}
            <div class="contact-hero-avatar" style="background: {selectedContact.color}">
              {selectedContact.initials}
            </div>
          {/if}
          <div class="contact-hero-text">
            <h2 class="contact-hero-name">{selectedContact.name}</h2>
            {#if selectedContact.jobTitle}
              <p class="contact-hero-title">{selectedContact.jobTitle}</p>
            {/if}
            {#if selectedContact.organization}
              <p class="contact-hero-company">{selectedContact.organization}</p>
            {/if}
          </div>
        </div>

        <!-- Quick Actions -->
        <div class="contact-actions">
          <button class="contact-action-btn" aria-label="Send Email" data-tooltip={t('contacts.sendEmail')} data-tooltip-position="bottom-start" onclick={() => onEmailContact?.(selectedContact.email, selectedContact.name)}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2" y="4" width="20" height="16" rx="2"/>
              <path d="M2 7l10 6 10-6"/>
            </svg>
            <span>{t('contacts.email')}</span>
          </button>
          {#if primaryPhone(selectedContact)}
            <button class="contact-action-btn" aria-label="Call" data-tooltip={t('contacts.call')} onclick={() => onCallContact?.(primaryPhone(selectedContact)!)}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72c.127.96.361 1.903.7 2.81a2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45c.907.339 1.85.573 2.81.7A2 2 0 0122 16.92z"/>
              </svg>
              <span>{t('contacts.call')}</span>
            </button>
          {/if}
          <button class="contact-action-btn" aria-label="Schedule Meeting" data-tooltip={t('contacts.scheduleMeeting')} onclick={() => onMeetContact?.(selectedContact!)}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3" y="4" width="18" height="18" rx="2" ry="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/><path d="M12 14l2 2 4-4"/>
            </svg>
            <span>{t('contacts.meet')}</span>
          </button>
        </div>

        <!-- Contact Information -->
        <div class="contact-info-section">
          <h3 class="contact-info-heading">{t('contacts.contactInfo')}</h3>

          {#each selectedContact.emails ?? [] as e (e.email + e.label)}
            <div class="contact-info-row">
              <div class="contact-info-label">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <rect x="2" y="4" width="20" height="16" rx="2"/><path d="M2 7l10 6 10-6"/>
                </svg>
                {labelText(e.label)}{#if e.isDefault} <span class="ct-default-tag">({t('contacts.defaultTag')})</span>{/if}
              </div>
              <button tabindex="-1" class="contact-info-value contact-info-link" onclick={() => onEmailContact?.(e.email, selectedContact!.name)}>{e.email}</button>
            </div>
          {/each}

          {#each selectedContact.phones ?? [] as p (p.number + p.label)}
            <div class="contact-info-row">
              <div class="contact-info-label">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  {#if p.label === 'cell'}
                    <rect x="5" y="2" width="14" height="20" rx="2"/><line x1="12" y1="18" x2="12.01" y2="18"/>
                  {:else}
                    <path d="M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72c.127.96.361 1.903.7 2.81a2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45c.907.339 1.85.573 2.81.7A2 2 0 0122 16.92z"/>
                  {/if}
                </svg>
                {labelText(p.label)}
                {#if p.isDefault} <span class="ct-default-tag">({t('contacts.defaultTag')})</span>{/if}
              </div>
              <span class="contact-info-value">{p.number} {#if p.subtypes && p.subtypes.length > 0}
                <span class="ct-subtype-tags">({p.subtypes.map((s: string) => t(`contacts.label.${s}`) || s).join('/')})</span>{/if}</span>             
            </div>
          {/each}

          {#if selectedContact.department}
            <div class="contact-info-row">
              <div class="contact-info-label">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>
                </svg>
                {t('contacts.department')}
              </div>
              <span class="contact-info-value">{selectedContact.department}</span>
            </div>
          {/if}

          {#if selectedContact.organization}
            <div class="contact-info-row">
              <div class="contact-info-label">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <rect x="2" y="7" width="20" height="14" rx="2"/><path d="M16 3h-8v4h8z"/>
                </svg>
                {t('contacts.organization')}
              </div>
              <span class="contact-info-value">{selectedContact.organization}</span>
            </div>
          {/if}

          {#if selectedContact.birthday}
            <div class="contact-info-row">
              <div class="contact-info-label">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <rect x="3" y="4" width="18" height="18" rx="2"/><line x1="3" y1="10" x2="21" y2="10"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="16" y1="2" x2="16" y2="6"/>
                </svg>
                {t('contacts.birthdayLabel')}
              </div>
              <span class="contact-info-value">{selectedContact.birthday}</span>
            </div>
          {/if}

          {#each selectedContact.addresses ?? [] as a (formatAddress(a) + a.label)}
            <div class="contact-info-row">
              <div class="contact-info-label">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0118 0z"/><circle cx="12" cy="10" r="3"/>
                </svg>
                {labelText(a.label)}{#if a.isDefault}<span class="ct-default-tag">({t('contacts.defaultTag')})</span>{/if}
              </div>
              <span class="contact-info-value">{formatAddress(a)}</span>
            </div>
          {/each}
        </div>

        {#if selectedContact.notes}
          <div class="contact-notes-section">
            <h3 class="contact-info-heading">{t('contacts.notesLabel')}</h3>
            <p class="contact-notes">{selectedContact.notes}</p>
          </div>
        {/if}
      </div>
    {:else}
      <div class="contact-empty-state">
        <svg width="48" height="48" viewBox="0 0 32 32">
            <path fill="var(--text-tertiary)" d="M9.973 13.504c1.516 0 2.758.633 3.805 1.31c.516.335 1.032.713 1.494 1.037c.591.415 1.199.78 1.844 1.102c.11.046.188.063.271.059c.065-.004.323-.037.76-.564l.968-1.378l.002-.002l.283-.353c1.5-1.65 4.27-1.624 5.725.14l4.35 5.284l.208.296c.362.618.402 1.361.131 2.01l-.162.317c-.586.957-1.508 2.489-2.725 3.773c-1.209 1.276-2.852 2.461-4.9 2.461c-1.516 0-2.758-.633-3.805-1.31c-.965-.625-1.861-1.348-2.869-1.907c-.43-.225-.573-.3-.74-.29c-.065.003-.324.034-.76.559l-.968 1.382l-.002.002c-1.32 1.873-4.08 2.074-5.699.54l-.308-.33l-4.35-5.28a2.26 2.26 0 0 1-.178-2.624l.502-.812c.564-.895 1.31-1.998 2.223-2.961c1.209-1.276 2.852-2.461 4.9-2.461m0 1.98c-1.246 0-2.4.725-3.46 1.844c-.79.832-1.449 1.8-1.983 2.646l-.492.797a.28.28 0 0 0 .015.33L8.55 26.54c.766.719 2.107.611 2.713-.248l5.233-7.46a4 4 0 0 1-.508-.218a19 19 0 0 1-1.852-1.14c-.509-.358-.958-.689-1.433-.997c-.936-.606-1.785-.991-2.729-.992m13.622.631c-.728-.884-2.212-.826-2.86.09l.003.002l-.746 1.061l.012.01l-4.484 6.394c.352.124.62.284.753.353l.42.235c.412.242.803.508 1.172.767c.509.357.959.687 1.433.994c.937.607 1.786.992 2.729.993c1.246 0 2.401-.724 3.46-1.842c1.052-1.11 1.873-2.46 2.476-3.446c.053-.123.053-.211-.016-.328zM9.5 3a4.5 4.5 0 1 1 0 9a4.5 4.5 0 0 1 0-9m13 0a4.5 4.5 0 1 1 0 9a4.5 4.5 0 0 1 0-9m-13 2a2.5 2.5 0 1 0 0 5a2.5 2.5 0 0 0 0-5m13 0a2.5 2.5 0 1 0 0 5a2.5 2.5 0 0 0 0-5"/>
        </svg>
        <h3>{t('contacts.selectContact')}</h3>
        <p>{t('contacts.selectContactHint')}</p>
      </div>
    {/if}
  </div>
</div>

<!-- Contact Edit/Create Modal -->
{#if showContactModal}
  <div class="ct-modal-overlay" onclick={closeContactModal} role="presentation">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="ct-modal" bind:this={ctModalEl} onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Escape') closeContactModal(); }}>
      <div class="ct-modal-header">
        <h2 class="ct-modal-title">{editingContactId ? t('contacts.editContactTitle') : t('contacts.newContactTitle')}</h2>
        <button class="ct-modal-close" tabindex="-1" onclick={closeContactModal} aria-label="Close" data-tooltip={t('common.close')}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      <div class="ct-modal-body" tabindex="-1">
        <!-- Photo picker -->
        <input autocomplete="off" type="file" accept="image/*" class="ct-photo-file-input" bind:this={photoFileInput} onchange={handlePhotoFileChange} />
        <div class="ct-photo-picker">
          <button type="button" class="ct-photo-avatar" onclick={triggerPhotoPicker} aria-label="Photo" data-tooltip={t('contacts.choosePhoto')}>
            {#if ctPhotoUrl}
              <img class="ct-photo-img" src={ctPhotoUrl} alt={t('contacts.contactAvatar')} />
            {:else}
                <svg class="ct-photo-placeholder" width="28" height="28" viewBox="0 0 32 32">
                    <path fill="var(--text-tertiary)" d="M23 9A7 7 0 1 1 9 9a7 7 0 0 1 14 0m-2 0a5 5 0 1 0-10 0a5 5 0 0 0 10 0M7.5 18A3.5 3.5 0 0 0 4 21.5v.5c0 2.393 1.523 4.417 3.685 5.793C9.859 29.177 12.802 30 16 30s6.14-.823 8.315-2.207C26.477 26.417 28 24.393 28 22v-.5a3.5 3.5 0 0 0-3.5-3.5zM6 21.5A1.5 1.5 0 0 1 7.5 20h17a1.5 1.5 0 0 1 1.5 1.5v.5c0 1.473-.94 2.949-2.759 4.106C21.434 27.256 18.877 28 16 28s-5.434-.744-7.241-1.894C6.939 24.95 6 23.472 6 22z"/>
                </svg>
            {/if}
            <span class="ct-photo-overlay">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M23 19a2 2 0 01-2 2H3a2 2 0 01-2-2V8a2 2 0 012-2h4l2-3h6l2 3h4a2 2 0 012 2z"/><circle cx="12" cy="13" r="4"/>
              </svg>
            </span>
          </button>
          <div class="ct-photo-actions">
            <button type="button" class="ct-photo-action-btn" onclick={triggerPhotoPicker}>
              {ctPhotoUrl ? t('contacts.changePhoto') : t('contacts.addPhoto')}
            </button>
            {#if ctPhotoUrl}
              <button type="button" class="ct-photo-action-btn" onclick={removePhoto}>{t('contacts.removePhoto')}</button>
            {/if}
          </div>
          <div class="ct-header-actions">
            <button type="button" class="ct-favorite-star" class:favorited={ctIsFavorite} onclick={() => ctIsFavorite = !ctIsFavorite} aria-label="Favorite" data-tooltip={ctIsFavorite ? t('contacts.removeFromFavorites') : t('contacts.addToFavorites')}>
              {#if ctIsFavorite}
                <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
              {:else}
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
              {/if}
            </button>
            {#if editingContactId}
              <button type="button" class="ct-header-trash" onclick={deleteContact} aria-label="Delete" data-tooltip={t('common.delete')}>
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2"/></svg>
              </button>
            {/if}
          </div>
        </div>

        <div class="ct-field-row">
          <div class="ct-field ct-field-half">
            <label class="ct-label" for="ct-first">{t('contacts.firstName')}</label>
            <input autocomplete="off" id="ct-first" type="text" class="ct-input" bind:value={ctFirstName} placeholder={t('contacts.first')} />
          </div>
          <div class="ct-field ct-field-half">
            <label class="ct-label" for="ct-last">{t('contacts.lastName')}</label>
            <input autocomplete="off" id="ct-last" type="text" class="ct-input" bind:value={ctLastName} placeholder={t('contacts.last')} />
          </div>
        </div>

        <div class="ct-field-row">
          <div class="ct-field ct-field-third">
            <label class="ct-label" for="ct-middle">{t('contacts.middleName')}</label>
            <input autocomplete="off" id="ct-middle" type="text" class="ct-input" bind:value={ctMiddleName} placeholder={t('contacts.middle')} />
          </div>
          <div class="ct-field ct-field-abbr">
            <label class="ct-label" for="ct-prefix">{t('contacts.prefixLabel')}</label>
            <input autocomplete="off" id="ct-prefix" type="text" class="ct-input" bind:value={ctPrefix} placeholder={t('contacts.prefix')} />
          </div>
          <div class="ct-field ct-field-abbr">
            <label class="ct-label" for="ct-suffix">{t('contacts.suffixLabel')}</label>
            <input autocomplete="off" id="ct-suffix" type="text" class="ct-input" bind:value={ctSuffix} placeholder={t('contacts.suffix')} />
          </div>
        </div>

        <div class="ct-field">
          <label class="ct-label" for="ct-org">{t('contacts.organization')}</label>
          <input autocomplete="off" id="ct-org" type="text" class="ct-input" bind:value={ctOrganization} placeholder={t('contacts.organization')} />
        </div>

        <div class="ct-field-row">
          <div class="ct-field ct-field-half">
            <label class="ct-label" for="ct-job">{t('contacts.jobTitle')}</label>
            <input autocomplete="off" id="ct-job" type="text" class="ct-input" bind:value={ctJobTitle} placeholder={t('contacts.jobTitle')} />
          </div>
          <div class="ct-field ct-field-half">
            <label class="ct-label" for="ct-dept">{t('contacts.department')}</label>
            <input autocomplete="off" id="ct-dept" type="text" class="ct-input" bind:value={ctDepartment} placeholder={t('contacts.department')} />
          </div>
        </div>

        <div class="ct-field">
          <label class="ct-label" for="ct-birthday">{t('contacts.birthdayLabel')}</label>
          <input autocomplete="off" id="ct-birthday" type="text" class="ct-input" bind:value={ctBirthday} placeholder={t('contacts.birthday')} />
        </div>
        
        <!-- Emails -->
        <div class="ct-field">
          <div class="ct-section-header">
            <label class="ct-label" for="ct-emai">{t('contacts.email')}</label>
            <button type="button" class="icon-action-btn" onclick={addEmailRow} aria-label="Add Email" data-tooltip={t('contacts.addEmail')}>+</button>
          </div>
          {#each ctEmails as row, i (i)}
            <div class="ct-multi-row">
              <select class="ct-input ct-label-select" bind:value={row.label}>
                {#each EMAIL_LABELS as lbl}<option value={lbl}>{labelText(lbl)}</option>{/each}
              </select>
              <input autocomplete="off" type="email" class="ct-input ct-multi-input" class:invalid={row.email.trim() && !isLikelyEmail(row.email)} bind:value={row.email} placeholder={t('contacts.emailPlaceholder')} />
              <button type="button" class="ct-move-btn" onclick={() => moveEmailRow(i, -1)} disabled={i === 0} aria-label="Move Up" data-tooltip={t('contacts.moveUp')}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="18 15 12 9 6 15" />
                </svg>
              </button>
              <button type="button" class="ct-move-btn" onclick={() => moveEmailRow(i, 1)} disabled={i === ctEmails.length - 1} aria-label="Move Down" data-tooltip={t('contacts.moveDown')}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>
              <button type="button" class="icon-action-btn" onclick={() => removeEmailRow(i)} aria-label="Remove" data-tooltip={t('common.remove')}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="3 6 5 6 21 6" /><path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6" /><path d="M10 11v6" /><path d="M14 11v6" /><path d="M9 6V4a1 1 0 011-1h4a1 1 0 011 1v2" />
                </svg>
              </button>
            </div>
          {/each}
        </div>

        <!-- Phones -->
        <div class="ct-field">
          <div class="ct-section-header">
            <label class="ct-label" for="ct-phone">{t('contacts.phone')}</label>
            <button type="button" class="icon-action-btn" onclick={addPhoneRow} aria-label="Add Phone" data-tooltip={t('contacts.addPhone')}>+</button>
          </div>
          {#each ctPhones as row, i (i)}
            <div class="ct-multi-row ct-multi-row-phone">
              <select class="ct-input ct-label-select" bind:value={row.label}>
                {#each PHONE_LABELS as lbl}<option value={lbl}>{labelText(lbl)}</option>{/each}
              </select>
              <input autocomplete="off" type="tel" class="ct-input ct-multi-input" bind:value={row.number} placeholder={t('contacts.phonePlaceholder')} onblur={() => { row.number = normalizePhone(row.number); }} />
              <button type="button" class="ct-move-btn" onclick={() => movePhoneRow(i, -1)} disabled={i === 0} aria-label="Move Up" data-tooltip={t('contacts.moveUp')}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="18 15 12 9 6 15" />
                </svg>
              </button>
              <button type="button" class="ct-move-btn" onclick={() => movePhoneRow(i, 1)} disabled={i === ctPhones.length - 1} aria-label="Move Down" data-tooltip={t('contacts.moveDown')}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="18 15 12 9 6 15" />
                </svg>
              </button>
              <button type="button" class="icon-action-btn" onclick={() => removePhoneRow(i)} aria-label="Remove" data-tooltip={t('common.remove')}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="3 6 5 6 21 6" /><path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6" /><path d="M10 11v6" /><path d="M14 11v6" /><path d="M9 6V4a1 1 0 011-1h4a1 1 0 011 1v2" />
                </svg>
              </button>
            </div>
            <div class="ct-subtype-row">
              {#each PHONE_SUBTYPES as st}
                <label class="ct-subtype-toggle">
                  <input autocomplete="off" type="checkbox" checked={row.subtypes.includes(st)} onchange={() => togglePhoneSubtype(i, st)} />
                  <span>{labelText(st)}</span>
                </label>
              {/each}
            </div>
          {/each}
        </div>

        <!-- Addresses -->
        <div class="ct-field">
          <div class="ct-section-header">
            <label class="ct-label" for="ct-address">{t('contacts.addressLabel')}</label>
            <button type="button" class="icon-action-btn" onclick={addAddressRow} aria-label="Add Address" data-tooltip={t('contacts.addAddress')}>+</button>
          </div>
          {#each ctAddresses as row, i (i)}
            <div class="ct-address-block">
              <div class="ct-multi-row">
                <select class="ct-input ct-label-select" bind:value={row.label}>
                  {#each ADDRESS_LABELS as lbl}<option value={lbl}>{labelText(lbl)}</option>{/each}
                </select>
                <button type="button" class="ct-move-btn" onclick={() => moveAddressRow(i, -1)} disabled={i === 0} aria-label="Move Up" data-tooltip={t('contacts.moveUp')}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="18 15 12 9 6 15" />
                  </svg>
                </button>
                <button type="button" class="ct-move-btn" onclick={() => moveAddressRow(i, 1)} disabled={i === ctAddresses.length - 1} aria-label="Move Down" data-tooltip={t('contacts.moveDown')}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="6 9 12 15 18 9" />
                  </svg>
                </button>
                <button type="button" class="icon-action-btn" onclick={() => removeAddressRow(i)} aria-label="Remove" data-tooltip={t('common.remove')}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="3 6 5 6 21 6" /><path d="M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6" /><path d="M10 11v6" /><path d="M14 11v6" /><path d="M9 6V4a1 1 0 011-1h4a1 1 0 011 1v2" />
                  </svg>
                </button>
              </div>
              <input autocomplete="off" type="text" class="ct-input" bind:value={row.street} placeholder={t('contacts.street')} />
              <div class="ct-field-row">
                <input autocomplete="off" type="text" class="ct-input ct-field-half" bind:value={row.city} placeholder={t('contacts.city')} />
                <input autocomplete="off" type="text" class="ct-input ct-field-half" bind:value={row.region} placeholder={t('contacts.region')} />
              </div>
              <div class="ct-field-row">
                <input autocomplete="off" type="text" class="ct-input ct-field-half" bind:value={row.postalCode} placeholder={t('contacts.postalCode')} />
                <input autocomplete="off" type="text" class="ct-input ct-field-half" bind:value={row.country} placeholder={t('contacts.country')} />
              </div>
            </div>
          {/each}
        </div>

        <div class="ct-field">
          <label class="ct-label" for="ct-notes">{t('contacts.notesLabel')}</label>
          <textarea autocomplete="off" id="ct-notes" class="ct-input ct-textarea" bind:value={ctNotes} placeholder={t('contacts.notes')} rows="3"></textarea>
        </div>
      </div>
      <div class="ct-modal-footer">
        <div class="ct-footer-spacer"></div>
        <button class="btn btn-secondary" onclick={closeContactModal}>{t('common.cancel')}</button>
        <button class="btn btn-primary" onclick={saveContact} disabled={!ctCanSave}>{t('common.save')}</button>
      </div>
    </div>
  </div>
{/if}

<!-- Contact List Edit/Create Modal -->
{#if showListModal}
  <div class="ct-modal-overlay" onclick={closeListModal} role="presentation">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="ct-modal ct-modal-wide" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" onkeydown={(e) => { if (e.key === 'Escape') closeListModal(); }}>
      <div class="ct-modal-header">
        <h2 class="ct-modal-title">{editingListId ? t('contacts.editListTitle') : t('contacts.newListTitle')}</h2>
        <button class="ct-modal-close" tabindex="-1" onclick={closeListModal} aria-label="Close" data-tooltip={t('common.close')}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      <div class="ct-modal-body">
        <div class="ct-field">
          <label class="ct-label" for="cl-name">{t('contacts.listName')}</label>
          <input autocomplete="off" id="cl-name" type="text" class="ct-input" bind:value={clName} placeholder={t('contacts.listNamePlaceholder')} />
        </div>
        <div class="ct-field">
          <div class="ct-label">{t('contacts.members')} ({clMembers.length})</div>
          <div class="cl-members-list">
            {#each clMembers as member, i}
              <div class="cl-member-row">
                <div class="cl-member-info">
                  <span class="cl-member-name">{member.name || member.email}</span>
                  {#if member.name}
                    <span class="cl-member-email">{member.email}</span>
                  {/if}
                </div>
                <button class="cl-member-remove" tabindex="-1" onclick={() => removeListMember(i)} aria-label="Remove" data-tooltip={t('common.remove')}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                </button>
              </div>
            {/each}
          </div>
        </div>

        <div class="ct-field">
          <div class="ct-label">{t('contacts.addMember')}</div>
          <div class="cl-add-row">
            <input autocomplete="off" type="text" class="ct-input cl-add-input" bind:value={clNewName} placeholder={t('contacts.nameOptional')} />
            <input autocomplete="off" type="email" class="ct-input cl-add-input" class:invalid={clNewEmail.trim() && !isLikelyEmail(clNewEmail)} bind:value={clNewEmail} placeholder={t('contacts.emailPlaceholder')} onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addListMember(); } }} />
            <button class="btn btn-secondary cl-add-btn" tabindex="-1" onclick={addListMember} disabled={!clNewEmail.trim() || !isLikelyEmail(clNewEmail)}>{t('common.add')}</button>
          </div>
          <button class="cl-pick-contacts-btn" tabindex="-1" onclick={() => (showContactPicker = !showContactPicker)}>
            <svg width="14" height="14" viewBox="0 0 24 24">
                <path fill="currentColor" d="M7.48 10.385c1.136 0 2.068.475 2.853.983c.387.25.774.534 1.12.777c.359.252.695.474 1.031.65c.166.083.363.23.556.22c.048-.002.242-.026.569-.42l.94-1.302c1.126-1.237 3.204-1.218 4.295.105l3.419 4.186c.271.463.3 1.02.097 1.507l-.12.237c-.44.718-1.132 1.867-2.045 2.83c-.906.957-2.139 1.845-3.674 1.846c-1.137 0-2.07-.475-2.854-.983c-.387-.25-.775-.535-1.121-.778a12 12 0 0 0-.777-.51l-.254-.141c-.322-.169-.43-.226-.555-.22c-.05.004-.248.03-.58.433l-.717 1.024v.002c-.99 1.405-3.062 1.555-4.275.405l-3.494-4.208a1.69 1.69 0 0 1-.133-1.968l.376-.609c.422-.67.982-1.498 1.667-2.22c.906-.957 2.14-1.846 3.676-1.846m0 1.485c-.935 0-1.801.544-2.596 1.383c-.734.774-1.299 1.68-1.857 2.583a.21.21 0 0 0 .012.247l3.264 3.96l.108.118c.574.54 1.58.459 2.035-.186l3.924-5.594a4 4 0 0 1-.575-.27c-.425-.223-.825-.49-1.194-.75c-.382-.268-.72-.516-1.076-.747c-.701-.454-1.338-.744-2.045-.744m10.216.474c-.546-.663-1.66-.62-2.144.067l.002.001l-.56.797l.009.006l-3.364 4.796c.264.093.466.213.566.265l.315.176c.308.181.602.381.879.575c.381.268.719.516 1.075.746c.702.455 1.34.744 2.047.744c.934 0 1.8-.543 2.595-1.381c.789-.832 1.404-1.846 1.856-2.584c.04-.093.04-.159-.012-.247zM7 3a3 3 0 1 1 0 6a3 3 0 0 1 0-6m10 0a3 3 0 1 1 0 6a3 3 0 0 1 0-6M7 4.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 0 0 0-3m10 0a1.5 1.5 0 1 0 0 3a1.5 1.5 0 0 0 0-3"/>
            </svg>
            {showContactPicker ? t('contacts.hideContacts') : t('contacts.pickFromContacts')}
          </button>
          {#if showContactPicker}
            <div class="cl-contact-picker">
              {#each contacts.filter((c) => !isMuted(c)) as contact}
                {@const alreadyAdded = clMembers.some(m => m.email.toLowerCase() === contact.email.toLowerCase())}
                <button class="cl-pick-item" class:already-added={alreadyAdded} tabindex="-1" onclick={() => addContactAsMember(contact)} disabled={alreadyAdded}>
                  {#if contact.photoUrl}
                    <img class="contact-avatar-sm contact-avatar-img" src={contact.photoUrl} alt={contact.name} />
                  {:else}
                    <span class="contact-avatar-sm" style="background: {contact.color}">{contact.initials}</span>
                  {/if}
                  <span class="cl-pick-name">{contact.name}</span>
                  <span class="cl-pick-email">{contact.email}</span>
                  {#if alreadyAdded}
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
      <div class="ct-modal-footer">
        {#if editingListId}
          <button class="btn btn-danger" tabindex="-1" onclick={deleteList}>{t('common.delete')}</button>
        {/if}
        <div class="ct-footer-spacer"></div>
        <button class="btn btn-secondary" tabindex="-1" onclick={closeListModal}>{t('common.cancel')}</button>
        <button class="btn btn-primary" tabindex="-1" onclick={saveList} disabled={!clName.trim()}>{t('common.save')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .contacts-view {
    display: flex;
    flex: 1;
    overflow: hidden;
    background-color: var(--bg-primary);
    gap: 4px;
  }

  .contacts-view button {
    outline: none;
  }
  
  /* ── 1st Level Nav ── */
  .contacts-nav {
    width: 15%;
    height: 100%;
    min-width: 225px;
    max-width: 275px;
    flex-shrink: 0;
    background: var(--bg-secondary);
    display: flex;
    flex-direction: column;
    border-radius: 4px;
    gap: 2px;
  }

  .contacts-nav-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    padding: 8px 24px;
  }

  .contacts-nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 16px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    border-left: 4px solid transparent;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
    cursor: pointer;
    text-align: left;
    outline: none;
  }

  .contacts-nav-item:hover {
    background: var(--bg-hover);
    border-left-color: var(--border-hover); 
  }

  .contacts-nav-item.selected {
    color: var(--text-primary);
    background: var(--bg-selected);
    font-weight: 600;
  }

  .contacts-nav-item.selected:hover {
    border-left-color: var(--accent);
    color: var(--text-secondary);
  }

  .contacts-nav.active .contacts-nav-item.selected {
    border-left-color: var(--accent-active);
  }

  /* ── 2nd Level Nav — Contact List ── */
  .contacts-list-pane {
    width: 25%;
    height: 100%;
    min-width: 300px;
    max-width: 500px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
    background-color: var(--bg-secondary);
  }

  /* Search */
  /* Scrollable contact list */
  .contacts-scroll {
    height: 100%;
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  /* Letter groups */
  .contact-letter-group {
    padding-bottom: 2px;
    display: flex;
    flex-direction: column;
    background-color: var(--bg-secondary);
  }

  .contact-letter {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    padding: 6px 16px;
    position: sticky;
    border-bottom: 1px solid var(--border-light);
    border-radius: 4px;
    top: 0;
    z-index: 1;
  }

  .contact-section-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    text-align: left;
    border: none;
    border-bottom: 1px solid var(--border-light);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 14px;
  }

  .contact-section-chevron {
    opacity: 0.7;
    transition: transform 0.15s ease;
  }

  .contact-section-chevron.expanded {
    transform: rotate(90deg);
  }

  /* Contact items in 2nd level list */
  .contact-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 8px;
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
    border-left: 4px solid transparent;
    border-bottom: 1px solid var(--border-light);
    outline: none;
  }

  .contact-item:hover {
    background: var(--bg-hover);
    border-left-color: var(--border-hover);
  }

  .contact-item.selected {
    background: var(--bg-selected);
  }

  .contact-item.selected:hover {
    border-left-color: var(--accent);
  }

  .contact-item.selected:hover .contact-item-name {
    color: var(--text-secondary);
  }

  .contacts-scroll.active .contact-item.selected {
    border-left-color: var(--accent-active);
  }

  .contact-avatar-sm {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
  }

  .contact-avatar-img {
    object-fit: cover;
  }

  .contact-item-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 1px;
  }

  .contact-item-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .contact-item-detail {
    font-size: 11px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Nav Counts ── */
  .nav-count {
    margin-left: auto;
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-active);
    min-width: 18px;
    text-align: center;
  }

  /* ── Show Muted Toggle ── */
  .contacts-muted-toggle {
    padding: 8px 12px;
    border-radius: 4px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-light);
  }

  .muted-toggle-label {
    display: flex;
    justify-content: flex-end;
    cursor: pointer;
    gap: 6px;
  }

  .muted-toggle-text {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .toggle-switch {
    position: relative;
    width: 32px;
    height: 16px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 8px;
    cursor: pointer;
    transition: background 0.2s;
    padding: 0;
  }

  .toggle-switch.on {
    background: var(--accent, #0078d4);
    border-color: var(--accent, #0078d4);
  }

  .toggle-knob {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 12px;
    height: 12px;
    background: white;
    border-radius: 50%;
    transition: transform 0.2s;
  }

  .toggle-switch.on .toggle-knob {
    transform: translateX(16px);
  }

  /* ── Contact Hover Actions ── */
  .contact-hover-actions {
    display: flex;
    align-self: start;
    gap: 2px;
    flex-shrink: 0;
    margin-left: auto;
  }

  .contact-hover-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 4px;
    color: var(--text-tertiary);
    opacity: 0;
    pointer-events: none;
    transition: background 0.1s, color 0.1s, opacity 0.1s;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .contact-hover-btn.selected {
    opacity: 1;
    pointer-events: auto;
    color: var(--accent-active);
  }

  .contact-item:hover .contact-hover-btn {
    opacity: 1;
    pointer-events: auto;
  }

  .contact-hover-btn:hover,
  .contact-hover-btn:focus {
    border-left-color: var(--border-hover);
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* ── Muted Contact ── */
  .contact-item.muted .contact-item-name,
  .contact-item.muted .contact-item-detail {
    opacity: 0.5;
  }

  .contacts-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 32px 16px;
    color: var(--text-tertiary);
    font-size: 13px;
  }

  /* ── Contact Detail Pane ── */
  .contact-detail-pane {
    flex: 1;
    overflow-y: auto;
    background: var(--bg-primary);
    margin: 1px;
  }

  .contact-detail-card {
    max-width: 800px;
    padding: 24px;
  }

  /* Hero */
  .contact-hero {
    display: flex;
    align-items: flex-start;
    padding-bottom: 16px;
    gap: 16px;
    border-bottom: 1px solid var(--border-light);
    border-left: 4px solid transparent;
  }

  .contact-hero-avatar {
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 24px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
  }

  .contact-hero-avatar-img {
    object-fit: cover;
  }

  .contact-hero-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-top: 4px;
  }

  .contact-hero-name {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .contact-hero-title {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .contact-hero-company {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .contact-hero-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .hero-action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 6px;
    color: var(--text-secondary);
    transition: background 0.1s, color 0.1s;
    cursor: pointer;
    outline: none;
  }

  .hero-action-btn:hover,
  .hero-action-btn:focus {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .hero-action-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .hero-action-btn:disabled:hover,
  .hero-action-btn:disabled:focus {
    background: transparent;
    color: var(--text-secondary);
  }

  /* Quick actions */
  .contact-actions {
    display: flex;
    justify-content: flex-start;
    gap: 8px;
    padding: 8px 0;
    border-bottom: 1px solid var(--border-light);
    flex-wrap: wrap;
  }

  .contact-action-btn {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 11px;
    color: var(--text-secondary);
    transition: background 0.1s;
    outline: none;
  }

  .contact-action-btn:hover,
  .contact-action-btn:focus {
    background: var(--bg-hover);
    color: var(--accent-active);
  }

  .contact-action-btn svg {
    color: var(--accent);
  }

  .contact-action-btn:hover svg,
  .contact-action-btn:focus svg {
    color: var(--accent-active);
  }

  /* Info section */
  .contact-info-section {
    padding: 16px 0;
  }

  .contact-info-heading {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 12px;
  }

  .contact-info-row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 6px 0;
  }

  .contact-info-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    min-width: 100px;
    flex-shrink: 0;
    align-items: end;
  }

  .contact-info-value {
    font-size: 13px;
    color: var(--text-primary);
    word-break: break-all;
  }

  .contact-info-link {
    color: var(--accent);
    text-decoration: none;
    outline: none;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    cursor: pointer;
    text-align: left;
  }

  .contact-info-link:hover,
  .contact-info-link:focus {
    color: var(--accent-active);
    text-decoration: underline;
  }

  /* Notes */
  .contact-notes-section {
    padding-top: 16px;
    border-top: 1px solid var(--border-light);
  }

  .contact-notes {
    font-size: 13px;
    color: var(--text-primary);
    line-height: 1.5;
  }

  /* Empty state */
  .contact-empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 8px;
    padding: 32px;
    text-align: center;
  }

  .contact-empty-state h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .contact-empty-state p {
    font-size: 13px;
    color: var(--text-secondary);
  }

  /* ── Contact Edit/Create Modal ── */
  .ct-modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .ct-modal {
    background: var(--bg-secondary);
    border-radius: 8px;
    box-shadow: var(--shadow-lg, 0 8px 32px rgba(0, 0, 0, 0.18));
    width: 480px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .ct-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px 12px;
    flex-shrink: 0;
  }

  .ct-modal-title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .ct-modal-close {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .ct-modal-close:hover {
    background: var(--bg-hover);
  }

  .ct-modal-body {
    padding: 0 20px 16px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .ct-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ct-field-half {
    flex: 1;
  }

  .ct-field-third {
    flex: 1;
  }

  .ct-field-row {
    display: flex;
    gap: 12px;
  }

  .ct-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .ct-input {
    padding: 7px 10px;
    font-size: 13px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    transition: border-color 0.15s;
    outline: none;
  }

  .ct-input:hover {
    border-color: var(--accent-hover);
  }

  .ct-input:focus {
    border-color: var(--accent-active);
  }

  .ct-input.invalid {
    border-color: #d83b01;
  }

  .ct-field-abbr {
    flex: 0 0 auto;
    width: 101px;
  }

  .ct-textarea {
    resize: vertical;
    min-height: 60px;
    font-family: inherit;
  }

  .ct-section-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 2px;
  }

  .ct-default-tag,
  .ct-subtype-tags {
    color: var(--text-tertiary);
  }

  .ct-multi-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .ct-multi-input {
    flex: 1;
    min-width: 0;
  }

  .ct-label-select {
    flex: 0 0 auto;
    width: 96px;
    padding: 7px 8px;
  }

  .ct-move-btn {
    width: 22px;
    height: 24px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-primary);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 10px;
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.1s, border-color 0.1s, opacity 0.1s;
  }

  .ct-move-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--accent-hover);
  }

  .ct-move-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .ct-subtype-row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 12px;
    padding: 0 0 8px 104px;
    margin-top: -2px;
  }

  .ct-subtype-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    user-select: none;
  }

  .ct-subtype-toggle input[type="checkbox"] {
    margin: 0;
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    border: 1.5px solid var(--text-tertiary);
    border-radius: 3px;
    background: transparent;
    cursor: pointer;
    position: relative;
    transition: border-color 0.1s;
    flex-shrink: 0;
  }

  .ct-subtype-toggle input[type="checkbox"]:hover,
  .ct-subtype-toggle input[type="checkbox"]:checked {
    border-color: var(--text-secondary);
  }

  .ct-subtype-toggle input[type="checkbox"]:checked::after {
    content: '';
    position: absolute;
    left: 4px;
    top: 1px;
    width: 4px;
    height: 8px;
    border: solid var(--text-secondary);
    border-width: 0 2px 2px 0;
    transform: rotate(45deg);
  }

  .ct-address-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    border: 1px solid var(--border-light);
    border-radius: 4px;
    margin-bottom: 6px;
  }

  .ct-modal-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--border-light);
    flex-shrink: 0;
  }

  .ct-footer-spacer {
    flex: 1;
  }

  .btn {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.1s;
    outline: none;
  }

  .btn:focus {
    box-shadow: 0 0 0 1px var(--accent-active);
  }

  .btn-primary {
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn-secondary {
    background: transparent;
    color: var(--text-primary);
    border: 1px solid var(--border);
  }

  .btn-secondary:hover {
    background: var(--bg-hover);
  }

  .btn-danger {
    background: transparent;
    color: var(--danger, #d13438);
    border: 1px solid var(--danger, #d13438);
  }

  .btn-danger:hover {
    background: rgba(209, 52, 56, 0.2);
  }

  /* ── Photo Picker ── */
  .ct-photo-file-input {
    display: none;
  }

  .ct-photo-picker {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-bottom: 4px;
  }

  .ct-header-actions {
    margin-left: auto;
    display: flex;
    align-items: flex-start;
    gap: 4px;
  }

  .ct-favorite-star {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-tertiary);
    padding: 4px;
    border-radius: 4px;
    align-self: flex-start;
    outline: none;
  }

  .ct-header-trash {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-tertiary);
    padding: 4px;
    border-radius: 4px;
    align-self: flex-start;
    outline: none;
    transition: color 0.1s;
  }

  .ct-header-trash:hover {
    color: #d13438;
  }

  .ct-favorite-star:hover {
    color: var(--accent-hover);
  }

  .ct-favorite-star:focus {
    color: var(--accent-active);
  }

  .ct-favorite-star.favorited {
    color: #f5b400;
  }

  .ct-photo-avatar {
    position: relative;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    flex-shrink: 0;
    overflow: hidden;
    border: 2px solid var(--border-light);
    transition: border-color 0.15s;
    outline: none;
  }

  .ct-photo-avatar:hover {
    border-color: var(--accent-hover);
  }

  .ct-photo-avatar:focus {
    border-color: var(--accent-active);
  }

  .ct-photo-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .ct-photo-placeholder {
    color: var(--text-tertiary);
  }

  .ct-photo-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
    color: white;
    opacity: 0;
    transition: opacity 0.15s;
    border-radius: 50%;
  }

  .ct-photo-avatar:hover .ct-photo-overlay {
    opacity: 1;
  }

  .ct-photo-actions {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ct-photo-action-btn {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 2px 0;
    text-align: left;
    outline: none;
  }

  .ct-photo-action-btn:hover {
    color: var(--accent-hover);
  }

  .ct-photo-action-btn:focus {
    color: var(--accent-active);
  }

  .icon-action-btn {
    width: 24px;
    height: 24px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-primary);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.1s, border-color 0.1s;
  }

  .icon-action-btn:hover {
    background: var(--bg-hover);
    border-color: var(--accent-hover);
  }

  /* ── Contact Lists ── */
  .list-member-row-detail {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 0;
  }

  .list-member-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .list-member-name {
    font-size: 13px;
    color: var(--text-primary);
  }

  .list-member-email {
    font-size: 11px;
    color: var(--text-secondary);
  }

  /* ── Contact List Modal ── */
  .ct-modal-wide {
    max-width: 520px;
  }

  .cl-members-list {
    max-height: 180px;
    overflow-y: auto;
    border: 1px solid var(--border-light);
    border-radius: 6px;
    padding: 4px;
  }

  .cl-member-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 5px 8px;
    border-radius: 4px;
  }

  .cl-member-row:hover {
    background: var(--bg-hover);
  }

  .cl-member-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .cl-member-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .cl-member-email {
    font-size: 11px;
    color: var(--text-secondary);
  }

  .cl-member-remove {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    color: var(--text-tertiary);
    flex-shrink: 0;
    opacity: 0;
    transition: opacity 0.1s;
  }

  .cl-member-row:hover .cl-member-remove {
    opacity: 1;
  }

  .cl-member-remove:hover {
    background: var(--bg-hover);
    color: var(--danger, #d13438);
  }

  .cl-add-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .cl-add-input {
    flex: 1;
    min-width: 0;
  }

  .cl-add-btn {
    flex-shrink: 0;
    white-space: nowrap;
  }

  .cl-pick-contacts-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    cursor: pointer;
    margin-top: 4px;
  }

  .cl-pick-contacts-btn:hover {
    text-decoration: underline;
  }

  .cl-contact-picker {
    max-height: 200px;
    overflow-y: auto;
    border: 1px solid var(--border-light);
    border-radius: 6px;
    padding: 4px;
    margin-top: 4px;
  }

  .cl-pick-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 5px 8px;
    border-radius: 4px;
    text-align: left;
    cursor: pointer;
    transition: background 0.1s;
  }

  .cl-pick-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .cl-pick-item.already-added {
    opacity: 0.5;
    cursor: default;
  }

  .cl-pick-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .cl-pick-email {
    font-size: 11px;
    color: var(--text-secondary);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
