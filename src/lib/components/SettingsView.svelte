<script lang="ts">
  import { onMount } from 'svelte';
  import type { Account, MailServerSettings, CalDavSettings, CardDavSettings, Theme } from '$lib/types';
  import { discoverMailSettings, startDavServer, stopDavServer, getDavServerStatus, addDavUser, removeDavUser, listDavUsers, getStorageInfo, setStorageQuota, getAccountOfflineDownload, setAccountOfflineDownload } from '$lib/data/dataService';
  import type { StorageInfo } from '$lib/data/dataService';
  import { t } from '$lib/i18n/index.svelte';
  import { isLikelyEmail } from '$lib/utils';
  import type { DiscoveredConfig, ServerConfig } from '$lib/data/dataService';

  interface Props {
    theme: Theme;
    accentColor: string;
    accounts: Account[];
    locale: string;
    languageNames: Record<string, string>;
    initialTab?: 'general' | 'accounts';
    requireAccount?: boolean;
    onChangeTheme: (theme: Theme) => void;
    onChangeAccentColor: (colorId: string) => void;
    onChangeLocale: (code: string) => void;
    onUpdateAccount: (account: Account) => void | Promise<void>;
    onDeleteAccount: (accountId: string) => void;
    onAddAccount: (account: { name: string; email: string; initials: string; color: string; avatarUrl?: string }) => Account | Promise<Account>;
    onReorderAccounts?: (orderedIds: string[]) => void | Promise<void>;
    onCompleteRequiredSetup?: () => void | Promise<void>;
    onClose: () => void;
  }

  let {
    theme,
    accentColor,
    accounts,
    locale,
    languageNames,
    initialTab = 'general',
    requireAccount = false,
    onChangeTheme,
    onChangeAccentColor,
    onChangeLocale,
    onUpdateAccount,
    onDeleteAccount,
    onAddAccount,
    onReorderAccounts,
    onCompleteRequiredSetup,
    onClose,
  }: Props = $props();

  function moveAccount(accountId: string, direction: -1 | 1) {
    const idx = accounts.findIndex(a => a.id === accountId);
    if (idx < 0) return;
    const newIdx = idx + direction;
    if (newIdx < 0 || newIdx >= accounts.length) return;
    const reordered = accounts.slice();
    [reordered[idx], reordered[newIdx]] = [reordered[newIdx], reordered[idx]];
    onReorderAccounts?.(reordered.map(a => a.id));
  }

  let activeTab = $state<'general' | 'accounts' | 'sync-server'>('general');
  let didInitTab = false;

  // ── Account selection & form ──
  let selectedAccountId = $state<string | null>(null);
  let showAddForm = $state(false);
  let saveSuccessLabel = $state<string | null>(null);
  let saveSuccessTimer: ReturnType<typeof setTimeout> | null = null;

  function flashSaveSuccess(label = t('settings.saved')) {
    if (saveSuccessTimer) clearTimeout(saveSuccessTimer);
    saveSuccessLabel = label;
    saveSuccessTimer = setTimeout(() => { saveSuccessLabel = null; saveSuccessTimer = null; }, 1500);
  }
  let addForm = $state({ firstName: '', lastName: '', email: '', initials: '', color: '#0078d4', avatarUrl: '' });

  let accountForm = $state({
    firstName: '',
    lastName: '',
    alias: '',
    email: '',
    initials: '',
    color: '#0078d4',
    protocol: 'imap' as 'imap' | 'pop3',
    incomingServer: '',
    incomingPort: 993,
    incomingUsername: '',
    incomingPassword: '',
    incomingSecurity: 'ssl' as 'ssl' | 'tls' | 'none',
    smtpServer: '',
    smtpPort: 587,
    smtpUsername: '',
    smtpPassword: '',
    smtpSecurity: 'tls' as 'ssl' | 'tls' | 'none',
    calDavUrl: '',
    calDavUsername: '',
    calDavPassword: '',
    cardDavUrl: '',
    cardDavUsername: '',
    cardDavPassword: '',
    signature: '',
    syncIntervalMinutes: 5,
    avatarUrl: '',
  });

  const AVATAR_PALETTE = [
    '#e74c3c', '#e91e63', '#9b59b6', '#673ab7', '#3f51b5', '#0078d4',
    '#2196f3', '#03a9f4', '#00bcd4', '#009688', '#4caf50', '#8bc34a',
    '#cddc39', '#ffeb3b', '#ffc107', '#ff9800', '#ff5722', '#795548',
    '#f06292', '#ba68c8', '#7986cb', '#64b5f6', '#4dd0e1', '#4db6ac',
    '#81c784', '#aed581', '#dce775', '#fff176', '#ffd54f', '#ffb74d',
    '#a1887f', '#90a4ae', '#78909c', '#607d8b', '#455a64', '#263238',
  ];
  let colorPickerOpen = $state<'add' | 'edit' | null>(null);
  let colorPickerPos = $state({ top: 0, left: 0 });

  function openColorPicker(which: 'add' | 'edit', e: MouseEvent) {
    const btn = e.currentTarget as HTMLElement;
    const rect = btn.getBoundingClientRect();
    // popup is ~160px wide (6*24 + 5*2 + 12 padding), position above and to the left of the button
    const popupW = 164;
    const popupH = 164;
    let left = rect.left - popupW + rect.width;
    let top = rect.top - popupH - 4;
    if (left < 4) left = 4;
    if (top < 4) top = rect.bottom + 4;
    colorPickerPos = { top, left };
    colorPickerOpen = colorPickerOpen === which ? null : which;
  }

  let showIncomingPassword = $state(false);
  let showSmtpPassword = $state(false);
  let showCalDavPassword = $state(false);
  let showCardDavPassword = $state(false);
  let autodiscoverState = $state<'idle' | 'loading' | 'done' | 'error'>('idle');
  let autodiscoverMessage = $state('');
  let addFormError = $state('');
  let accountFormError = $state('');
  let autodiscoverTimer: ReturnType<typeof setTimeout> | null = null;
  let autodiscoverNonce = 0;
  let addPhotoInput = $state<HTMLInputElement | undefined>();
  let editPhotoInput = $state<HTMLInputElement | undefined>();

  // DAV server state
  let davServerAddr = $state<string | null>(null);
  let davBindAddr = $state('0.0.0.0:5232');
  let davUsers = $state<[string, string][]>([]);
  let davNewEmail = $state('');
  let davNewPassword = $state('');
  let davNewAccountId = $state('');
  let showDavPassword = $state(false);
  let davError = $state('');
  let pendingDavAdds = $state<Array<{ email: string; password: string; acctId: string }>>([]);

  async function loadDavStatus() {
    davServerAddr = await getDavServerStatus();
    davUsers = await listDavUsers();
  }

  async function toggleDavServer() {
    davError = '';
    if (davServerAddr) {
      await stopDavServer();
      davServerAddr = null;
    } else {
      try {
        davServerAddr = await startDavServer(davBindAddr);
      } catch (e: any) {
        davError = e?.toString() ?? 'Failed to start server';
      }
    }
  }

  function handleAddDavUser() {
    if (!davNewEmail || !davNewPassword || !davNewAccountId) return;
    if (!isLikelyEmail(davNewEmail)) {
      davError = t('settings.invalidEmail');
      return;
    }
    if (davUsers.some(([e]) => e === davNewEmail) || pendingDavAdds.some(p => p.email === davNewEmail)) {
      davError = 'User already exists';
      return;
    }
    davError = '';
    pendingDavAdds = [...pendingDavAdds, { email: davNewEmail, password: davNewPassword, acctId: davNewAccountId }];
    davNewEmail = '';
    davNewPassword = '';
    davNewAccountId = '';
  }

  function removePendingDavAdd(email: string) {
    pendingDavAdds = pendingDavAdds.filter(p => p.email !== email);
  }

  async function handleRemoveDavUser(email: string) {
    await removeDavUser(email);
    davUsers = await listDavUsers();
  }

  async function saveLocalSync() {
    davError = '';
    for (const p of pendingDavAdds) {
      try {
        await addDavUser(p.email, p.password, p.acctId);
      } catch (e: any) {
        davError = e?.toString() ?? 'Failed to add user';
        davUsers = await listDavUsers();
        return;
      }
    }
    pendingDavAdds = [];
    davUsers = await listDavUsers();
    flashSaveSuccess();
  }

  function saveFromHeader() {
    if (activeTab === 'accounts') {
      if (showAddForm) saveAdd();
      else if (selectedAccount) saveAccount();
    } else if (activeTab === 'sync-server') {
      saveLocalSync();
    }
  }

  let selectedAccount = $derived(accounts.find(a => a.id === selectedAccountId) ?? null);

  const ACCENT_COLORS = [
    { id: 'blue', color: '#0078d4', label: 'Blue' },
    { id: 'green', color: '#35a37d', label: 'Green' },
    { id: 'purple', color: '#6b69d6', label: 'Purple' },
    { id: 'gold', color: '#d5b014', label: 'Gold' },
    { id: 'magenta', color: '#d0489d', label: 'Magenta' },
  ];

  function selectAccount(account: Account) {
    selectedAccountId = account.id;
    showAddForm = false;
    addFormError = '';
    accountFormError = '';
    loadOfflineDownloadFor(account.id);
    autodiscoverState = 'idle';
    autodiscoverMessage = '';
    if (autodiscoverTimer) {
      clearTimeout(autodiscoverTimer);
      autodiscoverTimer = null;
    }
    showIncomingPassword = false;
    showSmtpPassword = false;
    showCalDavPassword = false;
    showCardDavPassword = false;
    const ss = account.serverSettings;
    lastPropagated = {};
    const nameParts = account.name.split(' ');
    accountForm = {
      firstName: nameParts[0] ?? '',
      lastName: nameParts.slice(1).join(' '),
      alias: account.alias ?? '',
      email: account.email,
      initials: account.initials,
      color: account.color,
      protocol: ss?.protocol ?? 'imap',
      incomingServer: ss?.incomingServer ?? '',
      incomingPort: ss?.incomingPort ?? (ss?.protocol === 'pop3' ? 995 : 993),
      incomingUsername: ss?.incomingUsername ?? account.email,
      incomingPassword: ss?.incomingPassword ?? '',
      incomingSecurity: ss?.incomingSecurity ?? 'ssl',
      smtpServer: ss?.smtpServer ?? '',
      smtpPort: ss?.smtpPort ?? 587,
      smtpUsername: ss?.smtpUsername ?? account.email,
      smtpPassword: ss?.smtpPassword ?? '',
      smtpSecurity: ss?.smtpSecurity ?? 'tls',
      syncIntervalMinutes: ss?.syncIntervalMinutes ?? 5,
      calDavUrl: account.calDavSettings?.url ?? '',
      calDavUsername: account.calDavSettings?.username ?? '',
      calDavPassword: account.calDavSettings?.password ?? '',
      cardDavUrl: account.cardDavSettings?.url ?? '',
      cardDavUsername: account.cardDavSettings?.username ?? '',
      cardDavPassword: account.cardDavSettings?.password ?? '',
      signature: account.signature ?? '',
      avatarUrl: account.avatarUrl ?? '',
    };
  }

  function buildAccountName(form: { firstName: string; lastName: string }): string {
    return [form.firstName.trim(), form.lastName.trim()].filter(Boolean).join(' ');
  }

  function hasRequiredAccountDetails() {
    return Boolean(
      buildAccountName(accountForm) &&
      accountForm.email.trim() &&
      accountForm.initials.trim() &&
      accountForm.incomingServer.trim() &&
      hasValidPort(accountForm.incomingPort) &&
      accountForm.incomingUsername.trim() &&
      accountForm.incomingPassword.trim() &&
      accountForm.smtpServer.trim() &&
      hasValidPort(accountForm.smtpPort) &&
      accountForm.smtpUsername.trim() &&
      accountForm.smtpPassword.trim()
    );
  }

  async function saveAccount() {
    if (!selectedAccount) return;
    if (requireAccount && !hasRequiredAccountDetails()) {
      accountFormError = t('settings.completeSettings');
      return;
    }
    if (accountForm.email.trim() && !isLikelyEmail(accountForm.email)) {
      accountFormError = t('settings.invalidEmail');
      return;
    }
    accountFormError = '';
    const serverSettings: MailServerSettings = {
      protocol: accountForm.protocol,
      incomingServer: accountForm.incomingServer,
      incomingPort: accountForm.incomingPort,
      incomingUsername: accountForm.incomingUsername,
      incomingPassword: accountForm.incomingPassword,
      incomingSecurity: accountForm.incomingSecurity,
      smtpServer: accountForm.smtpServer,
      smtpPort: accountForm.smtpPort,
      smtpUsername: accountForm.smtpUsername,
      smtpPassword: accountForm.smtpPassword,
      smtpSecurity: accountForm.smtpSecurity,
      syncIntervalMinutes: accountForm.syncIntervalMinutes,
    };
    const calDavSettings: CalDavSettings | undefined = accountForm.calDavUrl.trim() ? {
      url: accountForm.calDavUrl,
      username: accountForm.calDavUsername || accountForm.email,
      password: accountForm.calDavPassword,
    } : undefined;
    const cardDavSettings: CardDavSettings | undefined = accountForm.cardDavUrl.trim() ? {
      url: accountForm.cardDavUrl,
      username: accountForm.cardDavUsername || accountForm.email,
      password: accountForm.cardDavPassword,
    } : undefined;
    await onUpdateAccount({
      ...selectedAccount,
      name: buildAccountName(accountForm),
      alias: accountForm.alias || undefined,
      email: accountForm.email,
      initials: accountForm.initials,
      color: accountForm.color,
      avatarUrl: accountForm.avatarUrl || undefined,
      signature: accountForm.signature,
      serverSettings,
      calDavSettings,
      cardDavSettings,
    });
    if (requireAccount) {
      await onCompleteRequiredSetup?.();
    } else {
      flashSaveSuccess();
    }
  }

  function confirmDelete(account: Account) {
    if (accounts.length <= 1) return;
    onDeleteAccount(account.id);
    if (selectedAccountId === account.id) {
      selectedAccountId = accounts.find(a => a.id !== account.id)?.id ?? null;
      if (selectedAccountId) {
        const next = accounts.find(a => a.id === selectedAccountId);
        if (next) selectAccount(next);
      }
    }
  }

  function openAddForm() {
    selectedAccountId = null;
    showAddForm = true;
    addForm = { firstName: '', lastName: '', email: '', initials: '', color: '#0078d4', avatarUrl: '' };
    addFormError = '';
    accountFormError = '';
  }

  function cancelAdd() {
    showAddForm = false;
    addFormError = '';
    if (requireAccount && accounts.length === 0) {
      onClose();
      return;
    }
    if (accounts.length > 0) selectAccount(accounts[0]);
  }

  async function saveAdd() {
    const addName = buildAccountName(addForm);
    if (!addName || !addForm.email.trim()) {
      addFormError = t('settings.nameEmailRequired');
      return;
    }
    if (!isLikelyEmail(addForm.email)) {
      addFormError = t('settings.invalidEmail');
      return;
    }
    addFormError = '';
    const initials = addForm.initials.trim() || autoInitials(addName);
    const createdAccount = await onAddAccount({ name: addName, email: addForm.email, initials, color: addForm.color, avatarUrl: addForm.avatarUrl || undefined });
    showAddForm = false;
    selectAccount(createdAccount);
    activeTab = 'accounts';
    flashSaveSuccess(t('settings.accountAdded'));
  }

  function autoInitials(name: string): string {
    return name.split(' ').filter(Boolean).map(w => w[0]).join('').toUpperCase().slice(0, 2);
  }

  function handleAddPhotoChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file || !file.type.startsWith('image/')) return;
    const reader = new FileReader();
    reader.onload = () => { addForm.avatarUrl = reader.result as string; };
    reader.readAsDataURL(file);
    input.value = '';
  }

  function handleEditPhotoChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file || !file.type.startsWith('image/')) return;
    const reader = new FileReader();
    reader.onload = () => { accountForm.avatarUrl = reader.result as string; };
    reader.readAsDataURL(file);
    input.value = '';
  }

  function defaultIncomingPort(protocol: 'imap' | 'pop3', security: 'ssl' | 'tls' | 'none') {
    if (protocol === 'imap') return security === 'ssl' ? 993 : 143;
    return security === 'ssl' ? 995 : 110;
  }

  function defaultSmtpPort(security: 'ssl' | 'tls' | 'none') {
    return security === 'ssl' ? 465 : (security === 'tls' ? 587 : 25);
  }

  function isKnownIncomingDefaultPort(port: number) {
    return port === 993 || port === 143 || port === 995 || port === 110;
  }

  function isKnownSmtpDefaultPort(port: number) {
    return port === 465 || port === 587 || port === 25;
  }

  function onProtocolChange() {
    if (isKnownIncomingDefaultPort(accountForm.incomingPort)) {
      accountForm.incomingPort = defaultIncomingPort(accountForm.protocol, accountForm.incomingSecurity);
    }
    queueAutoDiscover();
  }

  function onIncomingSecurityChange() {
    if (isKnownIncomingDefaultPort(accountForm.incomingPort)) {
      accountForm.incomingPort = defaultIncomingPort(accountForm.protocol, accountForm.incomingSecurity);
    }
  }

  function onSmtpSecurityChange() {
    if (isKnownSmtpDefaultPort(accountForm.smtpPort)) {
      accountForm.smtpPort = defaultSmtpPort(accountForm.smtpSecurity);
    }
  }

  function normalizeDomainFromServer(server: string): string {
    let s = server.trim().toLowerCase();
    if (!s) return '';
    s = s.replace(/^\w+:\/\//, '');
    s = s.split('/')[0];
    s = s.split(':')[0];

    const knownPrefixes = ['imap.', 'pop.', 'pop3.', 'smtp.', 'mail.', 'autodiscover.'];
    for (const p of knownPrefixes) {
      if (s.startsWith(p)) {
        s = s.slice(p.length);
        break;
      }
    }

    if (!/^[a-z0-9.-]+\.[a-z]{2,}$/i.test(s)) return '';
    return s;
  }

  function resolveDiscoveryEmail(): string {
    const incomingUser = accountForm.incomingUsername.trim();
    const acctEmail = accountForm.email.trim();
    if (incomingUser.includes('@')) return incomingUser;
    if (acctEmail.includes('@')) return acctEmail;
    const domain = normalizeDomainFromServer(accountForm.incomingServer);
    if (domain) return `user@${domain}`;
    return '';
  }

  function mapSocketType(value: string): 'ssl' | 'tls' | 'none' {
    const v = value.toLowerCase();
    if (v.includes('ssl')) return 'ssl';
    if (v.includes('starttls') || v.includes('tls')) return 'tls';
    return 'none';
  }

  // Propagate a username value downward to fields that are empty or were previously
  // auto-filled (i.e. still hold the same value — user hasn't customised them yet).
  type UsernameField = 'smtpUsername' | 'calDavUsername' | 'cardDavUsername';

  // Track the last value we propagated from each source so we can detect
  // whether a downstream field was manually edited or is still "following".
  let lastPropagated: Record<string, string> = {};

  function propagateUsername(newVal: string, sourceKey: string, ...fields: UsernameField[]) {
    const prev = lastPropagated[sourceKey] ?? '';
    for (const f of fields) {
      const cur = accountForm[f];
      // Propagate only if downstream is empty or still equals what we last pushed
      if (!cur || cur === prev) accountForm[f] = newVal;
    }
    lastPropagated[sourceKey] = newVal;
  }

  function expandUsernameTemplate(template: string, email: string): string {
    const at = email.lastIndexOf('@');
    const local = at >= 0 ? email.slice(0, at) : email;
    const domain = at >= 0 ? email.slice(at + 1) : '';
    return template
      .replaceAll('%EMAILADDRESS%', email)
      .replaceAll('%EMAILLOCALPART%', local)
      .replaceAll('%EMAILDOMAIN%', domain);
  }

  function hasValidPort(value: unknown): boolean {
    const n = Number(value);
    return Number.isFinite(n) && n > 0 && n <= 65535;
  }

  function tryApplyKnownProviderDefaults(domainRaw: string): boolean {
    const domain = domainRaw.toLowerCase();
    const map: Record<string, { incomingHost: string; incomingPort: number; incomingSecurity: 'ssl' | 'tls' | 'none'; smtpHost: string; smtpPort: number; smtpSecurity: 'ssl' | 'tls' | 'none' }> = {
      'gmail.com': { incomingHost: 'imap.gmail.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.gmail.com', smtpPort: 587, smtpSecurity: 'tls' },
      'googlemail.com': { incomingHost: 'imap.gmail.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.gmail.com', smtpPort: 587, smtpSecurity: 'tls' },
      'outlook.com': { incomingHost: 'outlook.office365.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.office365.com', smtpPort: 587, smtpSecurity: 'tls' },
      'hotmail.com': { incomingHost: 'outlook.office365.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.office365.com', smtpPort: 587, smtpSecurity: 'tls' },
      'live.com': { incomingHost: 'outlook.office365.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.office365.com', smtpPort: 587, smtpSecurity: 'tls' },
      'yahoo.com': { incomingHost: 'imap.mail.yahoo.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.mail.yahoo.com', smtpPort: 465, smtpSecurity: 'ssl' },
      'icloud.com': { incomingHost: 'imap.mail.me.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.mail.me.com', smtpPort: 587, smtpSecurity: 'tls' },
      'me.com': { incomingHost: 'imap.mail.me.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.mail.me.com', smtpPort: 587, smtpSecurity: 'tls' },
      'mac.com': { incomingHost: 'imap.mail.me.com', incomingPort: 993, incomingSecurity: 'ssl', smtpHost: 'smtp.mail.me.com', smtpPort: 587, smtpSecurity: 'tls' },
    };

    const hit = map[domain];
    if (!hit) return false;

    if (!accountForm.incomingServer.trim() || normalizeDomainFromServer(accountForm.incomingServer) === domain) {
      accountForm.incomingServer = hit.incomingHost;
    }
    if (!hasValidPort(accountForm.incomingPort)) {
      accountForm.incomingPort = hit.incomingPort;
    }
    accountForm.incomingSecurity = hit.incomingSecurity;

    if (!accountForm.smtpServer.trim()) {
      accountForm.smtpServer = hit.smtpHost;
    }
    if (!hasValidPort(accountForm.smtpPort)) {
      accountForm.smtpPort = hit.smtpPort;
    }
    accountForm.smtpSecurity = hit.smtpSecurity;

    return true;
  }

  function hasCoreMailServerInfo(): boolean {
    return Boolean(
      accountForm.incomingServer.trim() &&
      hasValidPort(accountForm.incomingPort) &&
      accountForm.smtpServer.trim() &&
      hasValidPort(accountForm.smtpPort)
    );
  }

  function pickIncomingServer(cfg: DiscoveredConfig): ServerConfig | null {
    const preferred = accountForm.protocol === 'pop3' ? 'pop3' : 'imap';
    return cfg.incoming.find((s) => s.protocol.toLowerCase() === preferred)
      ?? cfg.incoming.find((s) => s.protocol.toLowerCase() === 'imap')
      ?? cfg.incoming.find((s) => s.protocol.toLowerCase() === 'pop3')
      ?? null;
  }

  function maybeApplyDiscoveredConfig(cfg: DiscoveredConfig, email: string) {
    const incoming = pickIncomingServer(cfg);
    const smtp = cfg.outgoing.find((s) => s.protocol.toLowerCase() === 'smtp') ?? null;
    const discoveryDomain = email.includes('@') ? email.split('@')[1].toLowerCase() : '';
    const currentIncomingServer = accountForm.incomingServer.trim().toLowerCase();
    let changed = false;

    if (incoming) {
      if (!accountForm.incomingServer.trim() || (discoveryDomain && currentIncomingServer === discoveryDomain)) {
        accountForm.incomingServer = incoming.hostname;
        changed = true;
      }

      const defaultIncomingPort = accountForm.protocol === 'pop3'
        ? (accountForm.incomingSecurity === 'ssl' ? 995 : 110)
        : (accountForm.incomingSecurity === 'ssl' ? 993 : 143);
      if (!hasValidPort(accountForm.incomingPort) || accountForm.incomingPort === defaultIncomingPort) {
        accountForm.incomingPort = incoming.port;
        changed = true;
      }

      if (!accountForm.incomingSecurity || accountForm.incomingSecurity === 'ssl') {
        accountForm.incomingSecurity = mapSocketType(incoming.socketType);
        changed = true;
      }

      if (!accountForm.incomingUsername.trim()) {
        accountForm.incomingUsername = expandUsernameTemplate(incoming.usernameTemplate || '%EMAILADDRESS%', email);
        changed = true;
      }
    }

    if (smtp) {
      if (!accountForm.smtpServer.trim()) {
        accountForm.smtpServer = smtp.hostname;
        changed = true;
      }

      const defaultSmtpPort = accountForm.smtpSecurity === 'ssl' ? 465 : (accountForm.smtpSecurity === 'tls' ? 587 : 25);
      if (!hasValidPort(accountForm.smtpPort) || accountForm.smtpPort === defaultSmtpPort) {
        accountForm.smtpPort = smtp.port;
        changed = true;
      }

      if (!accountForm.smtpSecurity || accountForm.smtpSecurity === 'tls') {
        accountForm.smtpSecurity = mapSocketType(smtp.socketType);
        changed = true;
      }

      if (!accountForm.smtpUsername.trim()) {
        accountForm.smtpUsername = expandUsernameTemplate(smtp.usernameTemplate || '%EMAILADDRESS%', email);
        changed = true;
      }
    }

    if (changed) {
      autodiscoverState = 'done';
      autodiscoverMessage = t('settings.discoverSuccess', { source: cfg.source });
    }
  }

  function queueAutoDiscover() {
    const email = resolveDiscoveryEmail();
    const domain = normalizeDomainFromServer(accountForm.incomingServer) || (email.includes('@') ? email.split('@')[1] : '');

    // Immediate known-provider fill to make UX responsive.
    if (domain && tryApplyKnownProviderDefaults(domain)) {
      // Do not show noisy status here; defaults are a friendly silent bonus.
      autodiscoverState = 'idle';
      autodiscoverMessage = '';
      return;
    }

    if (!email) return;

    if (autodiscoverTimer) {
      clearTimeout(autodiscoverTimer);
    }

    autodiscoverTimer = setTimeout(async () => {
      const nonce = ++autodiscoverNonce;
      const quietMode = hasCoreMailServerInfo();
      if (!quietMode) {
        autodiscoverState = 'loading';
        autodiscoverMessage = t('settings.discovering');
      }

      try {
        const cfg = await discoverMailSettings(email);
        if (nonce !== autodiscoverNonce) return;
        if (cfg) {
          maybeApplyDiscoveredConfig(cfg, email);
        } else {
          autodiscoverState = 'idle';
          autodiscoverMessage = '';
        }
      } catch {
        if (nonce !== autodiscoverNonce) return;
        if (quietMode || hasCoreMailServerInfo()) {
          autodiscoverState = 'idle';
          autodiscoverMessage = '';
        } else {
          autodiscoverState = 'error';
          autodiscoverMessage = t('settings.discoverFailed');
        }
      }
    }, 600);
  }

  // Auto-select first account when switching to accounts tab
  $effect(() => {
    if (!didInitTab) {
      activeTab = requireAccount ? 'accounts' : initialTab;
      didInitTab = true;
      loadDavStatus();
    } else if (requireAccount && activeTab !== 'accounts') {
      activeTab = 'accounts';
    }
    if (activeTab === 'accounts' && !selectedAccountId && !showAddForm && accounts.length > 0) {
      selectAccount(accounts[0]);
    }
  });

  $effect(() => {
    if (requireAccount && accounts.length === 0 && !showAddForm) {
      openAddForm();
    }
  });

  let settingsFocus = $state<'tabs' | 'accounts'>('tabs');
  let settingsPanelEl = $state<HTMLDivElement | null>(null);

  let tabOrder: ('general' | 'accounts' | 'sync-server')[] = $derived(
    requireAccount ? ['accounts'] : ['general', 'accounts', 'sync-server']
  );

  function handleSettingsKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { onClose(); return; }
    const target = e.target as HTMLElement;
    const isEditing = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA' || target?.isContentEditable;

    const modCtrl = (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey;
    const keyS = e.key === 's' || e.key === 'S';
    const keyD = e.key === 'd' || e.key === 'D';

    if (modCtrl && keyS && activeTab !== 'general') {
      e.preventDefault();
      saveFromHeader();
      return;
    }
    if (modCtrl && keyD && activeTab === 'accounts' && selectedAccount && accounts.length > 1 && !showAddForm) {
      e.preventDefault();
      confirmDelete(selectedAccount);
      return;
    }
    if (!isEditing && e.key === 'Delete' && activeTab === 'accounts' && selectedAccount && accounts.length > 1 && !showAddForm) {
      e.preventDefault();
      confirmDelete(selectedAccount);
      return;
    }
    if (e.altKey && !e.ctrlKey && !e.metaKey && keyS && activeTab === 'sync-server') {
      e.preventDefault();
      if (e.shiftKey && davServerAddr) toggleDavServer();
      else if (!e.shiftKey && !davServerAddr) toggleDavServer();
      return;
    }

    if (isEditing) return;

    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      const down = e.key === 'ArrowDown';
      if (settingsFocus === 'tabs') {
        const curIdx = tabOrder.indexOf(activeTab);
        const nextIdx = down ? Math.min(curIdx + 1, tabOrder.length - 1) : Math.max(curIdx - 1, 0);
        if (curIdx !== nextIdx) activeTab = tabOrder[nextIdx];
      } else if (settingsFocus === 'accounts' && activeTab === 'accounts') {
        if (accounts.length === 0) return;
        const curIdx = accounts.findIndex(a => a.id === selectedAccountId);
        const nextIdx = down ? Math.min(curIdx + 1, accounts.length - 1) : Math.max(curIdx - 1, 0);
        if (curIdx !== nextIdx) selectAccount(accounts[nextIdx]);
      }
    }

    if (e.key === 'ArrowRight' && settingsFocus === 'tabs' && activeTab === 'accounts' && accounts.length > 0) {
      e.preventDefault();
      settingsFocus = 'accounts';
      if (!selectedAccountId) selectAccount(accounts[0]);
    }
    if (e.key === 'ArrowLeft' && settingsFocus === 'accounts') {
      e.preventDefault();
      settingsFocus = 'tabs';
    }
  }

  // ── Storage quota (General tab) ──
  let storageInfo = $state<StorageInfo | null>(null);
  let storageEditing = $state(false);
  let storageDraftMB = $state(100);
  let storageError = $state('');

  function bytesToMB(n: number): number {
    return Math.floor(n / (1024 * 1024));
  }

  function formatBytes(n: number): string {
    if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GB`;
    if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(0)} MB`;
    if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
    return `${n} B`;
  }

  async function refreshStorageInfo() {
    try {
      storageInfo = await getStorageInfo();
    } catch (err) {
      console.error('Failed to read storage info:', err);
    }
  }

  function beginEditStorage() {
    if (!storageInfo) return;
    storageError = '';
    const minMB = bytesToMB(storageInfo.minQuotaBytes);
    const maxMB = bytesToMB(storageInfo.maxQuotaBytes);
    const curMB = storageInfo.quotaBytes ? bytesToMB(storageInfo.quotaBytes) : minMB;
    storageDraftMB = Math.min(Math.max(curMB, minMB), maxMB);
    storageEditing = true;
  }

  function cancelEditStorage() {
    storageEditing = false;
    storageError = '';
  }

  function clampStorageDraft() {
    if (!storageInfo) return;
    const minMB = bytesToMB(storageInfo.minQuotaBytes);
    const maxMB = bytesToMB(storageInfo.maxQuotaBytes);
    const n = Number(storageDraftMB);
    if (!Number.isFinite(n)) {
      storageDraftMB = minMB;
    } else {
      storageDraftMB = Math.min(Math.max(Math.round(n), minMB), maxMB);
    }
  }

  async function saveStorageQuota() {
    if (!storageInfo) return;
    const minMB = bytesToMB(storageInfo.minQuotaBytes);
    const maxMB = bytesToMB(storageInfo.maxQuotaBytes);
    if (!Number.isFinite(storageDraftMB) || storageDraftMB < minMB || storageDraftMB > maxMB) {
      storageError = t('settings.storageMaxHint', {
        min: `${minMB} MB`,
        max: `${maxMB} MB`,
      });
      return;
    }
    try {
      await setStorageQuota(storageDraftMB * 1024 * 1024);
      storageEditing = false;
      storageError = '';
      await refreshStorageInfo();
    } catch (err) {
      storageError = String(err);
    }
  }

  async function clearStorageQuota() {
    try {
      await setStorageQuota(0);
      storageEditing = false;
      await refreshStorageInfo();
      // Any in-memory offline-download checkbox bound to the current account
      // is stale after a clear — re-sync it.
      if (selectedAccountId) {
        offlineDownloadEnabled = await getAccountOfflineDownload(selectedAccountId);
      }
    } catch (err) {
      console.error('Failed to clear storage quota:', err);
    }
  }

  // ── Per-account offline-download toggle ──
  let offlineDownloadEnabled = $state(false);
  let offlineDownloadBusy = $state(false);
  let offlineDownloadDisabled = $derived(
    offlineDownloadBusy || !storageInfo || storageInfo.quotaBytes == null
  );

  async function loadOfflineDownloadFor(accountId: string) {
    try {
      offlineDownloadEnabled = await getAccountOfflineDownload(accountId);
    } catch (err) {
      console.error('Failed to read offline-download setting:', err);
      offlineDownloadEnabled = false;
    }
  }

  async function toggleOfflineDownload(accountId: string, next: boolean) {
    offlineDownloadBusy = true;
    try {
      await setAccountOfflineDownload(accountId, next);
      offlineDownloadEnabled = next;
    } catch (err) {
      console.error('Failed to update offline-download setting:', err);
      // Revert local flag if backend refused (e.g., no quota set).
      offlineDownloadEnabled = !next;
    } finally {
      offlineDownloadBusy = false;
    }
  }

  onMount(() => {
    refreshStorageInfo();
    // Focus first nav item (General, or Accounts in requireAccount mode)
    setTimeout(() => {
      settingsPanelEl?.querySelector<HTMLButtonElement>('.settings-nav-item')?.focus();
    }, 0);

    // Trap Tab within the settings panel
    function trapTab(e: KeyboardEvent) {
      if (e.key !== 'Tab' || !settingsPanelEl) return;
      const focusable = Array.from(settingsPanelEl.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
      ));
      if (!focusable.length) { e.preventDefault(); return; }
      const first = focusable[0], last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (e.shiftKey) {
        if (!settingsPanelEl.contains(active) || active === first) { e.preventDefault(); last.focus(); }
      } else {
        if (!settingsPanelEl.contains(active) || active === last) { e.preventDefault(); first.focus(); }
      }
    }
    document.addEventListener('keydown', trapTab, true);
    return () => document.removeEventListener('keydown', trapTab, true);
  });
</script>

<div class="settings-overlay" onclick={onClose} role="presentation">
  <div class="settings-panel" bind:this={settingsPanelEl} onclick={(e) => e.stopPropagation()} onkeydown={handleSettingsKeydown} role="dialog" tabindex="-1">
    <!-- Left navigation -->
    <nav class="settings-nav">
      <h2 class="settings-nav-title">{requireAccount ? t('settings.setupAccount') : t('settings.settings')}</h2>
      {#if !requireAccount}
        <button
          class="settings-nav-item"
          class:selected={activeTab === 'general'}
          class:active={settingsFocus === 'tabs' && activeTab === 'general'}
          tabindex="-1"
          onclick={() => { settingsFocus = 'tabs'; activeTab = 'general'; }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M12.012 2.25c.734.008 1.465.093 2.182.253a.75.75 0 0 1 .582.649l.17 1.527a1.384 1.384 0 0 0 1.927 1.116l1.4-.615a.75.75 0 0 1 .85.174a9.8 9.8 0 0 1 2.205 3.792a.75.75 0 0 1-.272.825l-1.241.916a1.38 1.38 0 0 0 0 2.226l1.243.915a.75.75 0 0 1 .272.826a9.8 9.8 0 0 1-2.204 3.792a.75.75 0 0 1-.849.175l-1.406-.617a1.38 1.38 0 0 0-1.926 1.114l-.17 1.526a.75.75 0 0 1-.571.647a9.5 9.5 0 0 1-4.406 0a.75.75 0 0 1-.572-.647l-.169-1.524a1.382 1.382 0 0 0-1.925-1.11l-1.406.616a.75.75 0 0 1-.85-.175a9.8 9.8 0 0 1-2.203-3.796a.75.75 0 0 1 .272-.826l1.243-.916a1.38 1.38 0 0 0 0-2.226l-1.243-.914a.75.75 0 0 1-.272-.826a9.8 9.8 0 0 1 2.205-3.792a.75.75 0 0 1 .85-.174l1.4.615a1.387 1.387 0 0 0 1.93-1.118l.17-1.526a.75.75 0 0 1 .583-.65q1.074-.238 2.201-.252m0 1.5a9 9 0 0 0-1.354.117l-.11.977A2.886 2.886 0 0 1 6.526 7.17l-.899-.394A8.3 8.3 0 0 0 4.28 9.092l.797.587a2.88 2.88 0 0 1 .001 4.643l-.799.588c.32.842.776 1.626 1.348 2.322l.905-.397a2.882 2.882 0 0 1 4.017 2.318l.109.984c.89.15 1.799.15 2.688 0l.11-.984a2.88 2.88 0 0 1 4.018-2.322l.904.396a8.3 8.3 0 0 0 1.348-2.318l-.798-.588a2.88 2.88 0 0 1-.001-4.643l.797-.587a8.3 8.3 0 0 0-1.348-2.317l-.897.393a2.884 2.884 0 0 1-4.023-2.324l-.109-.976a9 9 0 0 0-1.334-.117M12 8.25a3.75 3.75 0 1 1 0 7.5a3.75 3.75 0 0 1 0-7.5m0 1.5a2.25 2.25 0 1 0 0 4.5a2.25 2.25 0 0 0 0-4.5"/>
          </svg>
          {t('settings.general')}
        </button>
      {/if}
      <button
        class="settings-nav-item"
        class:selected={activeTab === 'accounts'}
        class:active={settingsFocus === 'tabs' && activeTab === 'accounts'}
        tabindex="-1"
        onclick={() => { settingsFocus = 'tabs'; activeTab = 'accounts'; }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24">
          <path fill="currentColor" d="M17.755 14a2.25 2.25 0 0 1 2.248 2.25v.575c0 .894-.32 1.759-.9 2.438c-1.57 1.833-3.957 2.738-7.103 2.738s-5.532-.905-7.098-2.74a3.75 3.75 0 0 1-.898-2.434v-.578A2.25 2.25 0 0 1 6.253 14zm0 1.5H6.252a.75.75 0 0 0-.75.75v.577c0 .535.192 1.053.54 1.46c1.253 1.469 3.22 2.214 5.957 2.214c2.739 0 4.706-.745 5.963-2.213a2.25 2.25 0 0 0 .54-1.463v-.576a.75.75 0 0 0-.748-.749M12 2.005a5 5 0 1 1 0 10a5 5 0 0 1 0-10m0 1.5a3.5 3.5 0 1 0 0 7a3.5 3.5 0 0 0 0-7"/>
        </svg>
        {t('settings.accounts')}
      </button>
      {#if !requireAccount}
        <button
          class="settings-nav-item"
          class:selected={activeTab === 'sync-server'}
          class:active={settingsFocus === 'tabs' && activeTab === 'sync-server'}
          tabindex="-1"
          onclick={() => { settingsFocus = 'tabs'; activeTab = 'sync-server'; }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24">
            <path fill="currentColor" d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2m-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39"/>
          </svg>
          {t('settings.localSync')}
        </button>
      {/if}
    </nav>

    <!-- Accounts secondary sidebar (only when accounts tab active) -->
    {#if activeTab === 'accounts'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="accounts-sidebar" onmousedown={() => (settingsFocus = 'accounts')}>
        <div class="accounts-sidebar-header">
          <span class="accounts-sidebar-title">{t('settings.accounts')}</span>
          <button class="icon-action-btn" tabindex="-1" onclick={openAddForm} aria-label={t('settings.addAccount')} data-tooltip={t('settings.addAccount')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
          </button>
        </div>
        <div class="accounts-sidebar-list">
          {#each accounts as account, idx (account.id)}
            <div
              class="account-sidebar-row"
              class:selected={selectedAccountId === account.id && !showAddForm}
              class:active={settingsFocus === 'accounts' && selectedAccountId === account.id && !showAddForm}
            >
              <button
                class="account-sidebar-item"
                tabindex="-1"
                onclick={() => selectAccount(account)}
              >
                <span class="account-avatar-sm" style="background: {account.color}">
                  {#if account.avatarUrl}
                    <img class="account-avatar-sm-img" src={account.avatarUrl} alt={account.name} />
                  {:else}
                    {account.initials}
                  {/if}
                </span>
                <div class="account-sidebar-info">
                  <span class="account-sidebar-name">{account.name}</span>
                  <span class="account-sidebar-email">{account.email}</span>
                </div>
              </button>
              {#if accounts.length > 1}
                <div class="account-reorder">
                  <button
                    class="account-reorder-btn"
                    tabindex="-1"
                    disabled={idx === 0}
                    onclick={() => moveAccount(account.id, -1)}
                    data-tooltip={t('settings.moveUp')}
                    data-tooltip-position="bottom-end"
                    aria-label={t('settings.moveUp')}
                  >
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="18 15 12 9 6 15" />
                    </svg>
                  </button>
                  <button
                    class="account-reorder-btn"
                    tabindex="-1"
                    disabled={idx === accounts.length - 1}
                    onclick={() => moveAccount(account.id, 1)}
                    data-tooltip={t('settings.moveDown')}
                    data-tooltip-position="bottom-end"
                    aria-label={t('settings.moveDown')}
                  >
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                  </button>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Right content -->
    <div class="settings-content">
      <div class="settings-content-header">
        <h3 class="settings-content-title">
          {#if activeTab === 'general'}
            {t('settings.general')}
          {:else if activeTab === 'sync-server'}
            {t('settings.localSyncServer')}
          {:else if showAddForm}
            {t('settings.addAccount')}
          {:else if selectedAccount}
            {selectedAccount.name}
          {:else}
            {t('settings.accounts')}
          {/if}
        </h3>
        <div class="settings-header-right">
          {#if activeTab === 'accounts' && (showAddForm || selectedAccount)}
            <button class="header-action-btn" class:header-action-success={saveSuccessLabel}
              data-tooltip="{saveSuccessLabel ?? (requireAccount ? t('settings.saveAndContinue') : t('settings.saveChanges'))} (Ctrl+S)"
              aria-label="{requireAccount ? t('settings.saveAndContinue') : t('settings.saveChanges')}"
              onclick={saveFromHeader}>
              {#if saveSuccessLabel}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="20 6 9 17 4 12"/>
                </svg>
              {:else}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/>
                  <polyline points="17 21 17 13 7 13 7 21"/>
                  <polyline points="7 3 7 8 15 8"/>
                </svg>
              {/if}
            </button>
            {#if selectedAccount && !showAddForm && accounts.length > 1}
              <button class="header-action-btn header-action-danger"
                data-tooltip="{t('settings.removeAccount')} (Ctrl+D)"
                aria-label={t('settings.removeAccount')}
                onclick={() => confirmDelete(selectedAccount!)}>
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2"/>
                </svg>
              </button>
            {/if}
          {:else if activeTab === 'sync-server'}
            <button class="header-action-btn" class:header-action-success={saveSuccessLabel}
              data-tooltip="{saveSuccessLabel ?? t('settings.saveChanges')} (Ctrl+S)"
              aria-label={t('settings.saveChanges')}
              onclick={saveFromHeader}>
              {#if saveSuccessLabel}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="20 6 9 17 4 12"/>
                </svg>
              {:else}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/>
                  <polyline points="17 21 17 13 7 13 7 21"/>
                  <polyline points="7 3 7 8 15 8"/>
                </svg>
              {/if}
            </button>
            <button class="header-action-btn"
              data-tooltip="{davServerAddr ? t('settings.stop') : t('settings.start')} ({davServerAddr ? 'Alt+Shift+S' : 'Alt+S'})"
              aria-label={davServerAddr ? t('settings.stop') : t('settings.start')}
              onclick={toggleDavServer}>
              {#if davServerAddr}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                  <rect x="6" y="6" width="12" height="12" rx="1.5"/>
                </svg>
              {:else}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M8 5v14l11-7z"/>
                </svg>
              {/if}
            </button>
          {/if}
          <button class="close-btn" tabindex="-1" 
            onclick={onClose} 
            aria-label={requireAccount ? t('settings.cancelSetup') : t('settings.closeSettings')}
            data-tooltip={requireAccount ? t('settings.cancelSetup') : t('settings.closeSettings')}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      </div>

      <div class="settings-content-body">
        {#if activeTab === 'general'}
          <!-- ── Storage (top section) ── -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.storage')}</h4>
            <p class="dav-description">{t('settings.storageDescription')}</p>

            {#if storageInfo}
              {@const info = storageInfo}
              {@const effectiveQuota = storageEditing
                ? storageDraftMB * 1024 * 1024
                : (info.quotaBytes ?? 0)}
              {@const total = Math.max(1, info.freeBytes + info.usedBytes)}
              {@const usedPct = (info.usedBytes / total) * 100}
              {@const reservedBytes = Math.max(0, effectiveQuota - info.usedBytes)}
              {@const reservedPct = (reservedBytes / total) * 100}
              {@const reservedEndPct = usedPct + reservedPct}
              {@const freeUnreservedBytes = Math.max(0, info.freeBytes - reservedBytes)}

              <div class="storage-layout">
                <div
                  class="storage-donut"
                  style="--used-pct: {usedPct}%; --reserved-end: {reservedEndPct}%;"
                  role="img"
                  aria-label="Storage breakdown"
                >
                  <div class="storage-donut-hole">
                    {#if info.quotaBytes != null || storageEditing}
                      <div class="storage-donut-value">{formatBytes(effectiveQuota)}</div>
                      <div class="storage-donut-label">{t('settings.storageLimit')}</div>
                    {:else}
                      <div class="storage-donut-label">{t('settings.storageNotSet')}</div>
                    {/if}
                  </div>
                </div>

                <div class="storage-legend">
                  <div class="legend-row">
                    <span class="legend-sw sw-used"></span>
                    <span>{t('settings.storageLegendUsed', { size: formatBytes(info.usedBytes) })}</span>
                  </div>
                  {#if reservedBytes > 0}
                    <div class="legend-row">
                      <span class="legend-sw sw-reserved"></span>
                      <span>{t('settings.storageLegendReserved', { size: formatBytes(reservedBytes) })}</span>
                    </div>
                  {/if}
                  <div class="legend-row">
                    <span class="legend-sw sw-free"></span>
                    <span>{t('settings.storageLegendFree', { size: formatBytes(freeUnreservedBytes) })}</span>
                  </div>
                </div>
              </div>

              {#if !storageEditing}
                <div class="storage-actions">
                  {#if info.canEnable}
                    <button class="btn" onclick={beginEditStorage}>
                      {info.quotaBytes != null ? t('settings.storageChange') : t('settings.storageSet')}
                    </button>
                  {:else}
                    <div class="autodiscover-note error">{t('settings.storageNotEnoughFree')}</div>
                  {/if}
                  {#if info.quotaBytes != null}
                    <button class="btn btn-danger" onclick={clearStorageQuota}>{t('settings.storageClear')}</button>
                  {/if}
                </div>
              {:else}
                <div class="detail-form">
                  <div class="storage-slider-row">
                    <input
                      class="storage-slider"
                      type="range"
                      min={bytesToMB(info.minQuotaBytes)}
                      max={bytesToMB(info.maxQuotaBytes)}
                      step="1"
                      bind:value={storageDraftMB}
                    />
                    <span class="storage-slider-readout">
                      <input
                        class="storage-readout-input"
                        type="number"
                        min={bytesToMB(info.minQuotaBytes)}
                        max={bytesToMB(info.maxQuotaBytes)}
                        step="1"
                        bind:value={storageDraftMB}
                        onblur={clampStorageDraft}
                        onkeydown={(e) => { if (e.key === 'Enter') { clampStorageDraft(); (e.currentTarget as HTMLInputElement).blur(); } }}
                      />
                      <span class="storage-readout-unit">MB</span>
                    </span>
                  </div>
                  <div class="storage-hint">
                    {t('settings.storageMaxHint', {
                      min: formatBytes(info.minQuotaBytes),
                      max: formatBytes(info.maxQuotaBytes),
                    })}
                  </div>
                  {#if storageError}
                    <div class="autodiscover-note error">{storageError}</div>
                  {/if}
                  <div class="storage-actions">
                    <button class="btn btn-primary" onclick={saveStorageQuota}>{t('settings.storageSave')}</button>
                    <button class="btn" onclick={cancelEditStorage}>{t('settings.storageCancel')}</button>
                  </div>
                </div>
              {/if}
            {/if}
          </section>

          <!-- ── Appearance (theme + accent color) ── -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.appearance')}</h4>
            <div class="theme-picker">
              <button
                class="theme-option"
                class:selected={theme === 'light'}
                onclick={() => onChangeTheme('light')}
              >
                <div class="theme-preview light-preview">
                  <div class="tp-sidebar"></div>
                  <div class="tp-content">
                    <div class="tp-line"></div>
                    <div class="tp-line short"></div>
                  </div>
                </div>
                <span class="theme-label">{t('settings.light')}</span>
              </button>
              <button
                class="theme-option"
                class:selected={theme === 'dark'}
                onclick={() => onChangeTheme('dark')}
              >
                <div class="theme-preview dark-preview">
                  <div class="tp-sidebar"></div>
                  <div class="tp-content">
                    <div class="tp-line"></div>
                    <div class="tp-line short"></div>
                  </div>
                </div>
                <span class="theme-label">{t('settings.dark')}</span>
              </button>
              <button
                class="theme-option"
                class:selected={theme === 'system'}
                onclick={() => onChangeTheme('system')}
              >
                <div class="theme-preview system-preview">
                  <div class="system-half-light">
                    <div class="tp-sidebar"></div>
                    <div class="tp-content">
                      <div class="tp-line"></div>
                      <div class="tp-line short"></div>
                    </div>
                  </div>
                  <div class="system-half-dark">
                    <div class="tp-content">
                      <div class="tp-line"></div>
                      <div class="tp-line short"></div>
                    </div>
                  </div>
                </div>
                <span class="theme-label">{t('settings.system')}</span>
              </button>
            </div>
            <div class="color-grid">
              {#each ACCENT_COLORS as c (c.id)}
                <button
                  class="color-swatch"
                  class:selected={accentColor === c.id}
                  style="background: {c.color}"
                  aria-label={c.label}
                  data-tooltip={c.label}
                  onclick={() => onChangeAccentColor(c.id)}
                >
                </button>
              {/each}
            </div>
          </section>

          <!-- ── Language Section ── -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.language')}</h4>
            <div class="language-picker">
              {#each Object.entries(languageNames) as [code, name] (code)}
                <button
                  class="language-option"
                  class:selected={locale === code}
                  onclick={() => onChangeLocale(code)}
                >
                  {name}
                </button>
              {/each}
            </div>
          </section>

        {:else if activeTab === 'sync-server'}
          <!-- ── Local Sync Server ── -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.server')}</h4>
            <p class="dav-description">{t('settings.serverDescription')}</p>

            <div class="dav-detail-form">
              <div class="dav-status-row">
                <span class="dav-status-badge" class:dav-running={!!davServerAddr}>
                  {davServerAddr ? t('settings.running', { addr: davServerAddr }) : t('settings.stopped')}
                </span>
              </div>

              {#if !davServerAddr}
                <label class="form-row">
                  <span class="form-label">{t('settings.bindAddress')}</span>
                  <input class="form-input" type="text" bind:value={davBindAddr} placeholder="0.0.0.0:5232" />
                </label>
              {/if}

              {#if davError}
                <div class="autodiscover-note error">{davError}</div>
              {/if}
            </div>
          </section>

          <section class="settings-section">
            <h4 class="section-title">{t('settings.authorizedUsers')}</h4>
            <p class="dav-description">{t('settings.authorizedUsersDesc')}</p>

            <div class="detail-form">
              {#if davUsers.length > 0 || pendingDavAdds.length > 0}
                <div class="dav-users-list">
                  {#each davUsers as [email, acctId]}
                    <div class="dav-user-row">
                      <span class="dav-user-email">{email}</span>
                      <span class="dav-user-account">{accounts.find(a => a.id === acctId)?.name ?? acctId}</span>
                      <button class="btn btn-danger btn-sm" onclick={() => handleRemoveDavUser(email)}>{t('common.remove')}</button>
                    </div>
                  {/each}
                  {#each pendingDavAdds as p (p.email)}
                    <div class="dav-user-row dav-user-pending">
                      <span class="dav-user-email">{p.email}</span>
                      <span class="dav-user-account">{accounts.find(a => a.id === p.acctId)?.name ?? p.acctId}</span>
                      <button class="btn btn-danger btn-sm" onclick={() => removePendingDavAdd(p.email)}>{t('common.remove')}</button>
                    </div>
                  {/each}
                </div>
              {:else}
                <p class="dav-no-users">{t('settings.noUsersConfigured')}</p>
              {/if}

              <div class="dav-add-user">
                <span class="form-label">{t('settings.addUser')}</span>
                <div class="dav-add-user-form">
                  <input class="form-input" type="email" bind:value={davNewEmail} placeholder="user@example.com" />
                  <div class="password-wrapper">
                    {#if showDavPassword}
                      <input class="form-input password-input" type="text" bind:value={davNewPassword} placeholder={t('settings.password')} />
                    {:else}
                      <input class="form-input password-input" type="password" bind:value={davNewPassword} placeholder={t('settings.password')} />
                    {/if}
                    <button class="password-toggle" type="button" onclick={() => showDavPassword = !showDavPassword} aria-label={showDavPassword ? t('settings.hidePassword') : t('settings.showPassword')}>
                      {#if showDavPassword}
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                          <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
                          <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
                          <line x1="1" y1="1" x2="23" y2="23" />
                        </svg>
                      {:else}
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                          <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                          <circle cx="12" cy="12" r="3" />
                        </svg>
                      {/if}
                    </button>
                  </div>
                  <select class="form-input" bind:value={davNewAccountId}>
                    <option value="">{t('settings.selectAccountOption')}</option>
                    {#each accounts as acct}
                      <option value={acct.id}>{acct.name} ({acct.email})</option>
                    {/each}
                  </select>
                  <button class="btn btn-primary btn-sm" onclick={handleAddDavUser} disabled={!davNewEmail || !davNewPassword || !davNewAccountId}>{t('common.add')}</button>
                </div>
              </div>
            </div>
          </section>

        {:else if showAddForm}
          <!-- ── Add Account Form ── -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.newAccount')}</h4>
            {#if requireAccount}
              <p class="setup-note">{t('settings.setupNote')}</p>
            {/if}
            <div class="detail-form">
              <!-- Avatar picker -->
              <input type="file" accept="image/*" class="acct-photo-file-input" bind:this={addPhotoInput} onchange={handleAddPhotoChange} />
              <div class="acct-photo-picker">
                <button type="button" class="acct-photo-avatar"
                  onclick={() => addForm.avatarUrl ? addForm.avatarUrl = '' : addPhotoInput?.click()}
                  data-tooltip-position="right"
                  data-tooltip={addForm.avatarUrl ? t('common.remove') : t('settings.addPhoto')}
                  aria-label={addForm.avatarUrl ? t('common.remove') : t('settings.addPhoto')}>
                  {#if addForm.avatarUrl}
                    <img class="acct-photo-img" src={addForm.avatarUrl} alt="Account avatar" />
                  {:else}
                    <span class="acct-photo-initials" style="background: {addForm.color}">{addForm.initials || autoInitials(buildAccountName(addForm)) || ''}</span>
                  {/if}
                  <span class="acct-photo-overlay">
                    {#if addForm.avatarUrl}
                      <svg width="20" height="20" viewBox="0 0 24 24">
                        <path fill="currentColor" d="M3.28 2.22a.75.75 0 1 0-1.06 1.06l1.915 1.916A3.25 3.25 0 0 0 2 8.25v9.5A3.25 3.25 0 0 0 5.25 21h13.5a3.2 3.2 0 0 0 1.024-.165l.945.945a.75.75 0 0 0 1.061-1.06zM18.44 19.5H5.25a1.75 1.75 0 0 1-1.75-1.75v-9.5c0-.966.784-1.75 1.75-1.75h.19l3.11 3.11a4.5 4.5 0 0 0 6.34 6.34zm-8.822-8.822l4.205 4.205a3 3 0 0 1-4.206-4.206m1.628-2.615l1.54 1.541a3 3 0 0 1 2.111 2.11l1.54 1.541a4.5 4.5 0 0 0-5.192-5.192m9.255.187v9.068l1.364 1.365q.135-.446.136-.933v-9.5A3.25 3.25 0 0 0 18.75 5h-2.07l-.815-1.387a2.25 2.25 0 0 0-1.94-1.11h-3.803a2.25 2.25 0 0 0-1.917 1.073l-.55.896l1.09 1.091l.738-1.202l.065-.09a.75.75 0 0 1 .574-.268h3.803a.75.75 0 0 1 .646.37l1.032 1.757a.75.75 0 0 0 .647.37h2.5c.966 0 1.75.784 1.75 1.75" />
                      </svg>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M23 19a2 2 0 01-2 2H3a2 2 0 01-2-2V8a2 2 0 012-2h4l2-3h6l2 3h4a2 2 0 012 2z"/><circle cx="12" cy="13" r="4"/>
                      </svg>
                    {/if}
                  </span>
                </button>
              </div>

              <div class="form-row-inline">
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.firstName')}</span>
                  <input class="form-input" type="text" bind:value={addForm.firstName} placeholder="John" oninput={() => addForm.initials = autoInitials(buildAccountName(addForm))} />
                </label>
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.lastName')}</span>
                  <input class="form-input" type="text" bind:value={addForm.lastName} placeholder="Doe" oninput={() => addForm.initials = autoInitials(buildAccountName(addForm))} />
                </label>
              </div>
              <label class="form-row">
                <span class="form-label">{t('settings.email')}</span>
                <input class="form-input" type="email" bind:value={addForm.email} placeholder="john@example.com" />
              </label>
              <div class="form-row-inline">
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.initials')}</span>
                  <input class="form-input" type="text" bind:value={addForm.initials} maxlength="2" placeholder="JD" />
                </label>
                <div class="form-row" style="position:relative;">
                  <span class="form-label">{t('settings.color')}</span>
                  <button
                    type="button"
                    class="color-swatch-btn"
                    aria-label="{t('settings.color')}"
                    style="background:{addForm.color}"
                    onclick={(e) => openColorPicker('add', e)}
                  ></button>
                  {#if colorPickerOpen === 'add'}
                    <div class="color-picker-popup" style="top:{colorPickerPos.top}px;left:{colorPickerPos.left}px">
                      {#each AVATAR_PALETTE as hex}
                        <button
                          type="button"
                          class="color-picker-cell"
                          class:selected={addForm.color === hex}
                          style="background:{hex}"
                          aria-label="{hex}"
                          onclick={() => { addForm.color = hex; colorPickerOpen = null; }}
                        ></button>
                      {/each}
                    </div>
                  {/if}
                </div>
              </div>
              {#if addFormError}
                <div class="autodiscover-note error">{addFormError}</div>
              {/if}
              <div class="form-actions">
                <button class="btn btn-secondary" onclick={cancelAdd}>{requireAccount ? t('settings.cancelAndExit') : t('common.cancel')}</button>
              </div>
            </div>
          </section>

        {:else if selectedAccount}
          <!-- ── Account Detail ── -->
          {#if requireAccount}
            <section class="settings-section">
              <p class="setup-note">{t('settings.setupFinishNote')}</p>
            </section>
          {/if}

          <!-- General -->
          <section class="settings-section">
            <div class="detail-form">
              <!-- Avatar picker -->
              <input type="file" accept="image/*" class="acct-photo-file-input" bind:this={editPhotoInput} onchange={handleEditPhotoChange} />
              <div class="acct-photo-picker">
                <button type="button" class="acct-photo-avatar"
                  onclick={() => accountForm.avatarUrl ? accountForm.avatarUrl = '' : editPhotoInput?.click()}
                  data-tooltip-position="right"
                  data-tooltip={accountForm.avatarUrl ? t('common.remove') : t('settings.addPhoto')}
                  aria-label={accountForm.avatarUrl ? t('common.remove') : t('settings.addPhoto')}>
                  {#if accountForm.avatarUrl}
                    <img class="acct-photo-img" src={accountForm.avatarUrl} alt="Account avatar" />
                  {:else}
                    <span class="acct-photo-initials" style="background: {accountForm.color}">{accountForm.initials}</span>
                  {/if}
                  <span class="acct-photo-overlay">
                    {#if accountForm.avatarUrl}
                      <svg width="20" height="20" viewBox="0 0 24 24">
                        <path fill="currentColor" d="M3.28 2.22a.75.75 0 1 0-1.06 1.06l1.915 1.916A3.25 3.25 0 0 0 2 8.25v9.5A3.25 3.25 0 0 0 5.25 21h13.5a3.2 3.2 0 0 0 1.024-.165l.945.945a.75.75 0 0 0 1.061-1.06zM18.44 19.5H5.25a1.75 1.75 0 0 1-1.75-1.75v-9.5c0-.966.784-1.75 1.75-1.75h.19l3.11 3.11a4.5 4.5 0 0 0 6.34 6.34zm-8.822-8.822l4.205 4.205a3 3 0 0 1-4.206-4.206m1.628-2.615l1.54 1.541a3 3 0 0 1 2.111 2.11l1.54 1.541a4.5 4.5 0 0 0-5.192-5.192m9.255.187v9.068l1.364 1.365q.135-.446.136-.933v-9.5A3.25 3.25 0 0 0 18.75 5h-2.07l-.815-1.387a2.25 2.25 0 0 0-1.94-1.11h-3.803a2.25 2.25 0 0 0-1.917 1.073l-.55.896l1.09 1.091l.738-1.202l.065-.09a.75.75 0 0 1 .574-.268h3.803a.75.75 0 0 1 .646.37l1.032 1.757a.75.75 0 0 0 .647.37h2.5c.966 0 1.75.784 1.75 1.75" />
                      </svg>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M23 19a2 2 0 01-2 2H3a2 2 0 01-2-2V8a2 2 0 012-2h4l2-3h6l2 3h4a2 2 0 012 2z"/><circle cx="12" cy="13" r="4"/>
                      </svg>
                    {/if}
                  </span>
                </button>
              </div>

              <div class="form-row-inline">
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.firstName')}</span>
                  <input class="form-input" type="text" bind:value={accountForm.firstName} oninput={() => accountForm.initials = autoInitials(buildAccountName(accountForm))} />
                </label>
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.lastName')}</span>
                  <input class="form-input" type="text" bind:value={accountForm.lastName} oninput={() => accountForm.initials = autoInitials(buildAccountName(accountForm))} />
                </label>
              </div>
              <label class="form-row">
                <span class="form-label">{t('settings.alias')}</span>
                <input class="form-input" type="text" bind:value={accountForm.alias} placeholder={t('settings.aliasPlaceholder')} />
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.emailAddress')}</span>
                <input class="form-input" type="email" bind:value={accountForm.email} oninput={queueAutoDiscover} />
              </label>
              <div class="form-row-inline">
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.initials')}</span>
                  <input class="form-input" type="text" bind:value={accountForm.initials} maxlength="2" />
                </label>
                <div class="form-row" style="position:relative;">
                  <span class="form-label">{t('settings.color')}</span>
                  <button
                    type="button"
                    class="color-swatch-btn"
                    style="background:{accountForm.color}"
                    aria-label="{t('settings.color')}"
                    onclick={(e) => openColorPicker('edit', e)}
                  ></button>
                  {#if colorPickerOpen === 'edit'}
                    <div class="color-picker-popup" style="top:{colorPickerPos.top}px;left:{colorPickerPos.left}px">
                      {#each AVATAR_PALETTE as hex}
                        <button
                          type="button"
                          aria-label="{t('settings.color')} {hex}"
                          class="color-picker-cell"
                          class:selected={accountForm.color === hex}
                          style="background:{hex}"
                          onclick={() => { accountForm.color = hex; colorPickerOpen = null; }}
                        ></button>
                      {/each}
                    </div>
                  {/if}
                </div>
              </div>
              <div class="form-row">
                <span class="form-label">{t('settings.signature')}</span>
                <textarea class="form-input signature-input" bind:value={accountForm.signature} placeholder={t('settings.signaturePlaceholder')}></textarea>
              </div>
            </div>
          </section>

          <!-- Incoming Mail Server -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.incomingMailServer')}</h4>
            <div class="detail-form">
              <label class="form-row">
                <span class="form-label">{t('settings.protocol')}</span>
                <select class="form-input" bind:value={accountForm.protocol} onchange={onProtocolChange}>
                  <option value="imap">IMAP</option>
                  <option value="pop3">POP3</option>
                </select>
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.server')}</span>
                <input class="form-input" type="text" bind:value={accountForm.incomingServer} oninput={queueAutoDiscover} placeholder={accountForm.protocol === 'imap' ? 'imap.example.com' : 'pop.example.com'} />
              </label>
              <div class="form-row-inline">
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.port')}</span>
                  <input class="form-input port-input" type="number" bind:value={accountForm.incomingPort} />
                </label>
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.security')}</span>
                  <select class="form-input" bind:value={accountForm.incomingSecurity} onchange={onIncomingSecurityChange}>
                    <option value="ssl">SSL/TLS</option>
                    <option value="tls">STARTTLS</option>
                    <option value="none">None</option>
                  </select>
                </label>
              </div>
              <label class="form-row">
                <span class="form-label">{t('settings.username')}</span>
                <input class="form-input" type="text" bind:value={accountForm.incomingUsername} oninput={() => { queueAutoDiscover(); propagateUsername(accountForm.incomingUsername, 'imap', 'smtpUsername', 'calDavUsername', 'cardDavUsername'); }} />
              </label>
              {#if autodiscoverState !== 'idle'}
                <div class="autodiscover-note" class:error={autodiscoverState === 'error'}>
                  {autodiscoverMessage}
                </div>
              {/if}
              <label class="form-row">
                <span class="form-label">{t('settings.password')}</span>
                <div class="password-wrapper">
                  {#if showIncomingPassword}
                    <input class="form-input password-input" type="text" bind:value={accountForm.incomingPassword} />
                  {:else}
                    <input class="form-input password-input" type="password" bind:value={accountForm.incomingPassword} />
                  {/if}
                  <button 
                    class="password-toggle" 
                    type="button" 
                    onclick={() => showIncomingPassword = !showIncomingPassword} 
                    data-tooltip={showIncomingPassword ? t('settings.hidePassword') : t('settings.showPassword')}>
                    {#if showIncomingPassword}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
                        <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
                        <line x1="1" y1="1" x2="23" y2="23" />
                      </svg>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                        <circle cx="12" cy="12" r="3" />
                      </svg>
                    {/if}
                  </button>
                </div>
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.syncInterval')}</span>
                <select class="form-input" bind:value={accountForm.syncIntervalMinutes}>
                  <option value={1}>{t('settings.every1Minute')}</option>
                  <option value={2}>{t('settings.everyNMinutes', { n: 2 })}</option>
                  <option value={5}>{t('settings.everyNMinutes', { n: 5 })}</option>
                  <option value={10}>{t('settings.everyNMinutes', { n: 10 })}</option>
                  <option value={15}>{t('settings.everyNMinutes', { n: 15 })}</option>
                  <option value={30}>{t('settings.everyNMinutes', { n: 30 })}</option>
                  <option value={60}>{t('settings.everyNMinutes', { n: 60 })}</option>
                </select>
              </label>
            </div>
          </section>

          <!-- Offline download toggle (per-account; requires app-wide storage quota) -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.offlineDownload')}</h4>
            <p class="dav-description">{t('settings.offlineDownloadDesc')}</p>
            <div class="detail-form">
              <div class="form-row toggle-row">
                <button
                  type="button"
                  class="toggle-switch"
                  class:on={offlineDownloadEnabled}
                  disabled={offlineDownloadDisabled}
                  aria-pressed={offlineDownloadEnabled}
                  aria-label={t('settings.offlineDownload')}
                  onclick={() => selectedAccountId && toggleOfflineDownload(selectedAccountId, !offlineDownloadEnabled)}
                >
                  <span class="toggle-knob"></span>
                </button>
                <span class="form-label toggle-label">{t('settings.offlineDownload')}</span>
              </div>
              {#if !storageInfo || storageInfo.quotaBytes == null}
                <div class="autodiscover-note">{t('settings.offlineDownloadRequiresQuota')}</div>
              {/if}
            </div>
          </section>

          <!-- Outgoing Mail Server (SMTP) -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.outgoingMailServer')}</h4>
            <div class="detail-form">
              <label class="form-row">
                <span class="form-label">{t('settings.smtpServer')}</span>
                <input class="form-input" type="text" bind:value={accountForm.smtpServer} placeholder="smtp.example.com" />
              </label>
              <div class="form-row-inline">
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.port')}</span>
                  <input class="form-input port-input" type="number" bind:value={accountForm.smtpPort} />
                </label>
                <label class="form-row" style="flex:1">
                  <span class="form-label">{t('settings.security')}</span>
                  <select class="form-input" bind:value={accountForm.smtpSecurity} onchange={onSmtpSecurityChange}>
                    <option value="ssl">SSL/TLS</option>
                    <option value="tls">STARTTLS</option>
                    <option value="none">None</option>
                  </select>
                </label>
              </div>
              <label class="form-row">
                <span class="form-label">{t('settings.username')}</span>
                <input class="form-input" type="text" bind:value={accountForm.smtpUsername} oninput={() => propagateUsername(accountForm.smtpUsername, 'smtp', 'calDavUsername', 'cardDavUsername')} />
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.password')}</span>
                <div class="password-wrapper">
                  {#if showSmtpPassword}
                    <input class="form-input password-input" type="text" bind:value={accountForm.smtpPassword} />
                  {:else}
                    <input class="form-input password-input" type="password" bind:value={accountForm.smtpPassword} />
                  {/if}
                  <button class="password-toggle" type="button" onclick={() => showSmtpPassword = !showSmtpPassword} title={showSmtpPassword ? t('settings.hidePassword') : t('settings.showPassword')}>
                    {#if showSmtpPassword}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
                        <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
                        <line x1="1" y1="1" x2="23" y2="23" />
                      </svg>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                        <circle cx="12" cy="12" r="3" />
                      </svg>
                    {/if}
                  </button>
                </div>
              </label>
            </div>
          </section>

          <!-- CalDAV (Calendar Sync) -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.calDav')}</h4>
            <div class="detail-form">
              <label class="form-row">
                <span class="form-label">{t('settings.calDavUrl')}</span>
                <input class="form-input" type="url" bind:value={accountForm.calDavUrl} placeholder="https://calendar.example.com/dav/" />
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.username')}</span>
                <input class="form-input" type="text" bind:value={accountForm.calDavUsername} placeholder={accountForm.email || 'username'} oninput={() => propagateUsername(accountForm.calDavUsername, 'caldav', 'cardDavUsername')} />
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.password')}</span>
                <div class="password-wrapper">
                  {#if showCalDavPassword}
                    <input class="form-input password-input" type="text" bind:value={accountForm.calDavPassword} />
                  {:else}
                    <input class="form-input password-input" type="password" bind:value={accountForm.calDavPassword} />
                  {/if}
                  <button class="password-toggle" type="button" onclick={() => showCalDavPassword = !showCalDavPassword} aria-label={showCalDavPassword ? t('settings.hidePassword') : t('settings.showPassword')} data-tooltip={showCalDavPassword ? t('settings.hidePassword') : t('settings.showPassword')} data-tooltip-position="bottom">
                    {#if showCalDavPassword}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
                        <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
                        <line x1="1" y1="1" x2="23" y2="23" />
                      </svg>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                        <circle cx="12" cy="12" r="3" />
                      </svg>
                    {/if}
                  </button>
                </div>
              </label>
            </div>
          </section>

          <!-- CardDAV (Contacts Sync) -->
          <section class="settings-section">
            <h4 class="section-title">{t('settings.cardDav')}</h4>
            <div class="detail-form">
              <label class="form-row">
                <span class="form-label">{t('settings.cardDavUrl')}</span>
                <input class="form-input" type="url" bind:value={accountForm.cardDavUrl} placeholder="https://contacts.example.com/dav/" />
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.username')}</span>
                <input class="form-input" type="text" bind:value={accountForm.cardDavUsername} placeholder={accountForm.email || 'username'} />
              </label>
              <label class="form-row">
                <span class="form-label">{t('settings.password')}</span>
                <div class="password-wrapper">
                  {#if showCardDavPassword}
                    <input class="form-input password-input" type="text" bind:value={accountForm.cardDavPassword} />
                  {:else}
                    <input class="form-input password-input" type="password" bind:value={accountForm.cardDavPassword} />
                  {/if}
                  <button class="password-toggle" type="button" onclick={() => showCardDavPassword = !showCardDavPassword} aria-label={showCardDavPassword ? t('settings.hidePassword') : t('settings.showPassword')} data-tooltip={showCardDavPassword ? t('settings.hidePassword') : t('settings.showPassword')} data-tooltip-position="bottom">
                    {#if showCardDavPassword}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
                        <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
                        <line x1="1" y1="1" x2="23" y2="23" />
                      </svg>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                        <circle cx="12" cy="12" r="3" />
                      </svg>
                    {/if}
                  </button>
                </div>
              </label>
            </div>
          </section>

          {#if accountFormError}
            <section class="settings-section">
              <div class="autodiscover-note error">{accountFormError}</div>
            </section>
          {/if}

        {:else}
          <div class="empty-state">
            <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round" style="opacity: 0.3">
              <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
              <circle cx="12" cy="7" r="4" />
            </svg>
            <span>{t('settings.selectAccountToView')}</span>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .settings-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .settings-panel {
    width: 92%;
    max-width: 960px;
    height: 80vh;
    background: var(--bg-primary);
    border-radius: 8px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
    display: flex;
    overflow: hidden;
    outline: none;
  }

  .setup-note {
    margin: 0 0 16px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  /* ── Left Nav ── */
  .settings-nav {
    width: 180px;
    flex-shrink: 0;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border-light);
    padding: 20px 4px 0 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .settings-nav-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    padding: 0 16px 16px;
  }

  .settings-nav-item {
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

  .settings-nav-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .settings-nav-item.selected {
    color: var(--accent-active);
    border-left-color: var(--accent);
    background: var(--bg-hover);
    font-weight: 600;
  }

  .settings-nav-item.selected.active {
    border-left-color: var(--accent-active);
  }

  /* ── Accounts Sidebar (secondary bar) ── */
  .accounts-sidebar {
    width: 200px;
    flex-shrink: 0;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border-light);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .accounts-sidebar-header {
    display: flex;
    align-items: center;
    padding: 20px 12px 12px;
    flex-shrink: 0;
    gap: 8px;
  }

  .accounts-sidebar-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.5px;
  }

  .icon-action-btn {
    width: 24px;
    height: 24px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
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

  .accounts-sidebar-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding-right: 2px;
  }

  .account-sidebar-row {
    display: flex;
    align-items: stretch;
    border-left: 4px solid transparent;
    transition: background 0.1s;
  }

  .account-sidebar-row:hover {
    background: var(--bg-hover);
  }

  .account-sidebar-row.selected {
    background: var(--bg-selected);
  }

  .account-sidebar-row.active {
    border-left-color: var(--accent-active);
    background: var(--bg-hover);
  }

  .account-sidebar-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    cursor: pointer;
    text-align: left;
    outline: none;
    flex: 1;
    min-width: 0;
    background: transparent;
  }

  .account-reorder {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
    padding-right: 4px;
    opacity: 0;
    transition: opacity 0.1s;
  }

  .account-sidebar-row:hover .account-reorder,
  .account-sidebar-row.selected .account-reorder,
  .account-sidebar-row.active .account-reorder {
    opacity: 1;
  }

  .account-reorder-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 3px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 0;
  }

  .account-reorder-btn:hover:not(:disabled) {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .account-reorder-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .account-avatar-sm {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
    overflow: hidden;
  }

  .account-avatar-sm-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .account-sidebar-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow: hidden;
  }

  .account-sidebar-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .account-sidebar-email {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── Right Content ── */
  .settings-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
  }

  .settings-content-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px 16px;
    border-bottom: 1px solid var(--border-light);
    flex-shrink: 0;
  }

  .settings-content-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 6px;
    color: var(--text-secondary);
    transition: background 0.1s, color 0.1s;
  }

  .settings-header-right {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .header-action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 6px;
    border: none;
    background: none;
    cursor: pointer;
    color: var(--text-secondary);
    transition: background 0.1s, color 0.1s;
  }

  .header-action-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .header-action-btn.header-action-danger:hover {
    color: #d13438;
  }

  .header-action-btn.header-action-success {
    color: #20af7a;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .settings-content-body {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 5px 20px;
  }

  /* ── Section ── */
  .settings-section {
    padding: 20px 0;
  }

  .settings-section + .settings-section {
    border-top: 1px solid var(--border-light);
  }

  .section-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 12px;
  }

  /* ── Theme picker ── */
  .theme-picker {
    display: flex;
    gap: 12px;
  }

  .theme-option {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 8px;
    border-radius: 8px;
    border: 2px solid var(--border-light);
    transition: border-color 0.15s, background 0.15s;
    cursor: pointer;
  }

  .theme-option:hover {
    border-color: var(--border);
  }

  .theme-option.selected {
    border-color: var(--accent);
    background: var(--bg-selected);
  }

  .theme-preview {
    width: 100px;
    height: 64px;
    border-radius: 6px;
    display: flex;
    overflow: hidden;
    border: 1px solid var(--border-light);
  }

  .light-preview { background: #ffffff; }
  .light-preview .tp-sidebar { width: 24px; background: #f3f3f3; border-right: 1px solid #e0e0e0; }
  .light-preview .tp-content { flex: 1; padding: 10px 8px; display: flex; flex-direction: column; gap: 6px; }
  .light-preview .tp-line { height: 4px; border-radius: 2px; background: #e0e0e0; }
  .light-preview .tp-line.short { width: 60%; }

  .dark-preview { background: #1e1e1e; }
  .dark-preview .tp-sidebar { width: 24px; background: #252525; border-right: 1px solid #3a3a3a; }
  .dark-preview .tp-content { flex: 1; padding: 10px 8px; display: flex; flex-direction: column; gap: 6px; }
  .dark-preview .tp-line { height: 4px; border-radius: 2px; background: #3a3a3a; }
  .dark-preview .tp-line.short { width: 60%; }

  .system-preview { background: #ffffff; position: relative; overflow: hidden; }
  .system-preview .system-half-light { display: flex; width: 100%; height: 100%; position: absolute; top: 0; left: 0; clip-path: polygon(0 0, 60% 0, 40% 100%, 0 100%); background: #ffffff; }
  .system-half-light .tp-sidebar { width: 24px; background: #f3f3f3; border-right: 1px solid #e0e0e0; }
  .system-half-light .tp-content { flex: 1; padding: 10px 8px; display: flex; flex-direction: column; gap: 6px; }
  .system-half-light .tp-line { height: 4px; border-radius: 2px; background: #e0e0e0; }
  .system-half-light .tp-line.short { width: 60%; }
  .system-preview .system-half-dark { display: flex; width: 100%; height: 100%; position: absolute; top: 0; left: 0; clip-path: polygon(60% 0, 100% 0, 100% 100%, 40% 100%); background: #1e1e1e; }
  .system-half-dark .tp-content { flex: 1; padding: 10px 8px; display: flex; flex-direction: column; gap: 6px; }
  .system-half-dark .tp-line { height: 4px; border-radius: 2px; background: #3a3a3a; }
  .system-half-dark .tp-line.short { width: 60%; }

  .theme-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  /* ── Language picker ── */
  .language-picker {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .language-option {
    padding: 6px 16px;
    border-radius: 6px;
    border: 1px solid var(--border-light);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    transition: all 0.15s;
    font-family: inherit;
  }

  .language-option:hover {
    background: var(--bg-hover);
  }

  .language-option.selected {
    border-color: var(--accent);
    background: var(--accent-light);
    color: var(--text-primary);
    font-weight: 600;
  }

  /* ── Color swatches ── */
  .color-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .color-swatch {
    width: 48px;
    height: 48px;
    border-radius: 1px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: border-color 0.15s, transform 0.1s, box-shadow 0.15s;
  }

  .color-swatch:hover {
    transform: scale(1.08);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  }

  .color-swatch.selected {
    border-color: var(--text-primary);
    box-shadow: 0 0 0 2px var(--bg-primary), 0 0 0 4px var(--accent);
  }

  /* ── Detail form ── */
  .dav-detail-form, .detail-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .form-row {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .form-row-inline {
    display: flex;
    gap: 12px;
  }

  .autodiscover-note {
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-light);
    border-radius: 4px;
    padding: 6px 10px;
  }

  .autodiscover-note.error {
    color: #b42318;
    border-color: #fecdca;
    background: #fef3f2;
  }

  .form-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.3px;
  }

  .form-input {
    width: 100%;
    padding: 6px 10px;
    font-size: 13px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-primary);
    color: var(--text-primary);
    outline: none;
    transition: border-color 0.15s;
  }

  .dav-add-user-form .form-input, .dav-detail-form .form-input {
    width: 500px;
  }

  .form-input:focus {
    border-color: var(--accent);
  }

  .form-input[type="number"],
  .storage-readout-input[type="number"] {
    appearance: textfield;
    -moz-appearance: textfield;
  }

  .form-input[type="number"]::-webkit-outer-spin-button,
  .form-input[type="number"]::-webkit-inner-spin-button,
  .storage-readout-input[type="number"]::-webkit-outer-spin-button,
  .storage-readout-input[type="number"]::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  select.form-input {
    cursor: pointer;
    appearance: auto;
  }

  .signature-input {
    min-height: 100px;
    resize: vertical;
    font-family: inherit;
    line-height: 1.5;
  }

  .color-swatch-btn {
    width: 30px;
    height: 30px;
    border-radius: 4px;
    border: 2px solid var(--border);
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
  }
  .color-swatch-btn:hover {
    border-color: var(--text-primary);
  }

  .color-picker-popup {
    position: fixed;
    z-index: 10000;
    display: grid;
    grid-template-columns: repeat(6, 24px);
    grid-template-rows: repeat(6, 24px);
    gap: 2px;
    padding: 6px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 4px 12px rgba(0,0,0,0.25);
  }

  .color-picker-cell {
    width: 24px;
    height: 24px;
    border-radius: 3px;
    border: 2px solid transparent;
    cursor: pointer;
    padding: 0;
  }
  .color-picker-cell:hover {
    border-color: var(--text-primary);
    transform: scale(1.15);
  }
  .color-picker-cell.selected {
    border-color: white;
    box-shadow: 0 0 0 1px var(--text-primary);
  }

  /* ── Password field ── */
  .password-wrapper {
    position: relative;
    display: flex;
    align-items: center;
  }

  .password-input {
    padding-right: 36px;
  }

  .password-toggle {
    position: absolute;
    right: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 4px;
    color: var(--text-tertiary);
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

    .dav-add-user-form  .password-toggle {
      left: 470px;
    }

  .password-toggle:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* ── Action buttons ── */
  .form-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .btn {
    padding: 6px 16px;
    font-size: 12px;
    font-weight: 500;
    border-radius: 4px;
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

  .btn-primary {
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .dav-add-user-form .btn-primary {
    width: 50px;
    margin-top: 12px;
  }

  .btn-primary:hover {
    background: var(--accent-hover);
  }

  .btn-primary:active {
    background: var(--accent-active);
    transform: scale(0.97);
  }

  .btn-secondary {
    background: var(--bg-tertiary);
    color: var(--text-primary);
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
    background: rgba(209, 52, 56, 0.08);
  }

  /* ── Empty state ── */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    color: var(--text-tertiary);
    font-size: 13px;
  }

  /* ── Account Photo Picker ── */
  .acct-photo-file-input {
    display: none;
  }

  .acct-photo-picker {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-bottom: 4px;
  }

  .acct-photo-avatar {
    position: relative;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    flex-shrink: 0;
    border: 2px solid var(--border-light);
    transition: border-color 0.15s;
    padding: 0;
  }

  .acct-photo-avatar:hover {
    border-color: var(--accent);
  }

  .acct-photo-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
  }

  .acct-photo-initials {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 18px;
    font-weight: 600;
    color: white;
    border-radius: 50%;
  }

  .acct-photo-overlay {
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

  .acct-photo-avatar:hover .acct-photo-overlay {
    opacity: 1;
  }

  /* Storage quota section */
  .storage-layout {
    display: flex;
    align-items: center;
    gap: 20px;
    margin: 8px 0 12px 0;
  }
  .storage-donut {
    --used-pct: 0%;
    --reserved-end: 0%;
    position: relative;
    width: 120px;
    height: 120px;
    border-radius: 50%;
    background: conic-gradient(
      var(--accent) 0 var(--used-pct),
      var(--accent-light) var(--used-pct) var(--reserved-end),
      var(--border-light) var(--reserved-end) 100%
    );
    flex-shrink: 0;
  }
  .storage-donut-hole {
    position: absolute;
    inset: 18%;
    border-radius: 50%;
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 4px;
  }
  .storage-donut-value {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .storage-donut-label {
    font-size: 11px;
    color: var(--text-secondary);
    margin-top: 2px;
  }
  .storage-legend {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    color: var(--text-primary);
  }
  .legend-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .legend-sw {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    flex-shrink: 0;
  }
  .sw-used { background: var(--accent); }
  .sw-reserved { background: var(--accent-light); }
  .sw-free { background: var(--border-light); }

  .storage-slider-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .storage-slider {
    flex: 1;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: var(--border-light);
    outline: none;
    cursor: pointer;
  }
  .storage-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--accent);
    cursor: pointer;
    border: none;
  }
  .storage-slider::-moz-range-thumb {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--accent);
    cursor: pointer;
    border: none;
  }
  .storage-slider-readout {
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
    min-width: 90px;
    justify-content: flex-end;
  }
  .storage-readout-input {
    width: 70px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border-light);
    padding: 2px 0;
    text-align: right;
    outline: none;
    font-family: inherit;
  }
  .storage-readout-input:focus {
    border-bottom-color: var(--accent);
  }
  .storage-readout-unit {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .storage-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .storage-hint {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 4px;
  }
  .toggle-row {
    flex-direction: row;
    align-items: center;
    gap: 10px;
  }
  .toggle-label {
    cursor: default;
  }
  .toggle-switch {
    position: relative;
    width: 32px;
    height: 18px;
    border-radius: 9px;
    border: none;
    padding: 0;
    cursor: pointer;
    background: var(--border);
    flex-shrink: 0;
    transition: background 0.15s;
    outline: none;
  }
  .toggle-switch.on {
    background: var(--accent);
  }
  .toggle-switch:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: white;
    transition: transform 0.15s;
    pointer-events: none;
  }
  .toggle-switch.on .toggle-knob {
    transform: translateX(14px);
  }

  /* DAV server section */
  .dav-description {
    font-size: 12px;
    color: var(--text-secondary);
    margin: 0 0 8px 0;
    line-height: 1.4;
  }
  .dav-status-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .dav-status-badge {
    font-size: 12px;
    padding: 4px 10px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .dav-status-badge.dav-running {
    background: #dff6dd;
    color: #107c10;
  }
  .btn-sm {
    padding: 4px 10px;
    font-size: 11px;
  }
  .dav-users-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .dav-user-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    background: var(--bg-secondary);
    border-radius: 4px;
    font-size: 12px;
  }
  .dav-user-email {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dav-user-pending {
    font-style: italic;
    opacity: 0.75;
  }
  .dav-user-account {
    color: var(--text-secondary);
    font-size: 11px;
  }
  .dav-no-users {
    font-size: 12px;
    color: var(--text-tertiary);
    margin: 0;
  }
  .dav-add-user {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .dav-add-user-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
