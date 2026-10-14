<script lang="ts">
  import type { Account, Mailbox, SavedSearch, Tag, View } from '../../lib/api';
  import { call, toAppError, SAVED_SEARCH_NAME_CHARS } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import CertDialog, { type CertTarget } from '../../lib/mail/CertDialog.svelte';
  import { flatten, folderIcon, folderLabel, folderTree } from '../../lib/mail/folders';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import NamePrompt from '../../lib/ui/NamePrompt.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { mail, openSettings, reloadMailboxes, searchAll, setView, syncLabel } from '../../lib/stores/mail.svelte';
  import { settings, updateSettings } from '../../lib/stores/settings.svelte';
  import { tagStore } from '../../lib/stores/tags.svelte';
  import { installUpdate, update } from '../../lib/stores/update.svelte';

  interface NavItem {
    kind: Exclude<View['kind'], 'mailbox' | 'tag'>;
    icon: IconName;
    label: MessageKey;
    count?: () => number;
  }

  const items: NavItem[] = [
    { kind: 'inbox', icon: 'inbox', label: 'nav.inbox', count: () => mail.counts.inboxUnread },
    { kind: 'snoozed', icon: 'clock', label: 'nav.snoozed', count: () => mail.counts.snoozed },
    { kind: 'starred', icon: 'star', label: 'nav.starred' },
    { kind: 'drafts', icon: 'draft', label: 'nav.drafts', count: () => mail.counts.drafts },
    { kind: 'sent', icon: 'send', label: 'nav.sent' },
    { kind: 'archive', icon: 'archive', label: 'nav.archive' },
    { kind: 'trash', icon: 'trash', label: 'nav.trash' },
  ];

  function domainOf(email: string) {
    return email.split('@')[1] ?? email;
  }

  // ------------------------------------------------ açık hesaplar ve klasörler
  // Kişisel kolaylık: yalnız bu bilgisayarda, tarayıcı deposunda tutulur.

  const OPEN_KEY = 'mailhere.side.open';
  const SHUT_KEY = 'mailhere.side.collapsed';

  function load(key: string): Set<string> | null {
    try {
      const raw = localStorage.getItem(key);
      return raw ? new Set(JSON.parse(raw) as string[]) : null;
    } catch {
      return null;
    }
  }

  function save(key: string, set: Set<string>) {
    try {
      localStorage.setItem(key, JSON.stringify([...set]));
    } catch {
      /* depo kapalıysa yalnız bu oturumda geçerli */
    }
  }

  // İlk açılışta tek hesap varsa klasörleri açık gelir.
  let open = $state<Set<string>>(load(OPEN_KEY) ?? new Set(mail.accounts.length === 1 ? [mail.accounts[0].id] : []));
  let shut = $state<Set<string>>(load(SHUT_KEY) ?? new Set());

  function toggleAccount(id: string) {
    const next = new Set(open);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    open = next;
    save(OPEN_KEY, next);
  }

  function toggleFolder(id: string) {
    const next = new Set(shut);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    shut = next;
    save(SHUT_KEY, next);
  }

  const trees = $derived(
    Object.fromEntries(mail.accounts.map((a) => [a.id, flatten(folderTree(mail.mailboxes, a.id), shut)])),
  );

  function openFolder(account: Account, b: Mailbox) {
    // Gelen kutusu ertelenenleri gizleyen görünümle açılır; diğerleri klasörün kendisi.
    if (b.role === 'inbox') setView({ kind: 'inbox' }, account.id);
    else setView({ kind: 'mailbox', id: b.id }, account.id);
  }

  function folderActive(account: Account, b: Mailbox): boolean {
    if (mail.screen !== 'mail' || mail.search != null || mail.accountId !== account.id) return false;
    return b.role === 'inbox' ? mail.view.kind === 'inbox' : mail.view.kind === 'mailbox' && mail.view.id === b.id;
  }

  // ------------------------------------------------------------ sağ tık menüsü

  let menu = $state<{ at: { x: number; y: number }; title: string; items: MenuItem[] } | null>(null);

  async function hide(b: Mailbox) {
    try {
      await call('mailbox_set_hidden', { id: b.id, hidden: true });
      await reloadMailboxes();
      showToast({
        text: t('side.hidden', { name: folderLabel(b) }),
        action: {
          label: t('act.undo'),
          run: async () => {
            await call('mailbox_set_hidden', { id: b.id, hidden: false });
            await reloadMailboxes();
          },
        },
      });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  function folderMenu(e: MouseEvent | KeyboardEvent, account: Account, b: Mailbox) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const at = e instanceof MouseEvent && e.clientX ? { x: e.clientX, y: e.clientY } : { x: r.left + 24, y: r.bottom };
    menu = {
      at,
      title: folderLabel(b),
      items: [
        { label: t('side.open'), icon: 'folder', onclick: () => openFolder(account, b) },
        { label: t('side.hide'), icon: 'eyeOff', disabled: b.role === 'inbox', onclick: () => void hide(b) },
        { label: t('side.newRule'), icon: 'rules', disabled: b.role === 'inbox', onclick: () => openSettings('rules') },
        { label: t('side.manage'), icon: 'settings', onclick: () => openSettings(`account:${account.id}`) },
      ],
    };
  }

  function accountMenu(e: MouseEvent | KeyboardEvent, account: Account) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const at = e instanceof MouseEvent && e.clientX ? { x: e.clientX, y: e.clientY } : { x: r.left + 24, y: r.bottom };
    menu = {
      at,
      title: account.name,
      items: [
        { label: open.has(account.id) ? t('side.collapse') : t('side.expand'), icon: 'folder', onclick: () => toggleAccount(account.id) },
        { label: t('side.syncNow'), icon: 'refresh', onclick: () => void call('sync_now') },
        { label: t('side.accountSettings'), icon: 'settings', onclick: () => openSettings(`account:${account.id}`) },
      ],
    };
  }

  /** Klavyeyle menü: Shift+F10 ya da menü tuşu. */
  function menuKey(e: KeyboardEvent, show: (e: KeyboardEvent) => void) {
    if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) show(e);
  }

  // ------------------------------------------------ etiketler ve kayıtlı aramalar

  function pointOf(e: MouseEvent | KeyboardEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return e instanceof MouseEvent && e.clientX ? { x: e.clientX, y: e.clientY } : { x: r.left + 24, y: r.bottom };
  }

  function tagActive(tg: Tag): boolean {
    return mail.screen === 'mail' && mail.search == null && mail.view.kind === 'tag' && mail.view.id === tg.id;
  }

  function tagMenu(e: MouseEvent | KeyboardEvent, tg: Tag) {
    e.preventDefault();
    menu = {
      at: pointOf(e),
      title: tg.name,
      items: [
        { label: t('tag.open'), icon: 'tag', onclick: () => setView({ kind: 'tag', id: tg.id }) },
        { label: t('tag.edit'), icon: 'settings', onclick: () => openSettings('tags') },
      ],
    };
  }

  let rename = $state<{ at: { x: number; y: number }; search: SavedSearch } | null>(null);

  async function renameSearch(s: SavedSearch, name: string) {
    rename = null;
    try {
      await updateSettings({ savedSearches: settings.value.savedSearches.map((x) => (x.id === s.id ? { ...x, name } : x)) });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  async function deleteSearch(s: SavedSearch) {
    const before = settings.value.savedSearches;
    try {
      await updateSettings({ savedSearches: before.filter((x) => x.id !== s.id) });
      showToast({ text: t('saved.deleted', { name: s.name }), action: { label: t('act.undo'), run: () => updateSettings({ savedSearches: before }) } });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  function searchMenu(e: MouseEvent | KeyboardEvent, s: SavedSearch) {
    e.preventDefault();
    const at = pointOf(e);
    menu = {
      at,
      title: s.name,
      items: [
        { label: t('saved.run'), icon: 'search', onclick: () => searchAll(s.query) },
        { label: t('saved.rename'), icon: 'pen', onclick: () => (rename = { at, search: s }) },
        { label: t('saved.delete'), icon: 'trash', danger: true, onclick: () => void deleteSearch(s) },
      ],
    };
  }

  const sync = $derived(syncLabel());
  let cert = $state<CertTarget[] | null>(null);

  /** Eşitlemenin takıldığı sunucunun sertifikasını gösterir. */
  function reviewCert() {
    const u = sync.untrusted;
    if (!u) return;
    const servers = mail.accounts.flatMap((a) => [a.imap, a.smtp]).filter((s) => s != null);
    const security = servers.find((s) => s.host === u.host && s.port === u.port)?.security ?? 'tls';
    cert = [{ host: u.host, port: u.port, security }];
  }

  function certClosed(trusted: boolean) {
    cert = null;
    if (trusted) void call('sync_now');
  }
</script>

<nav class="side" aria-label={t('nav.label')}>
  <button class="btn-primary compose" onclick={() => call('open_compose', { prefill: { mode: 'new', messageId: null } })}>
    <Icon name="pen" />
    <span>{t('act.compose')}</span>
    <span class="kbd on-accent">C</span>
  </button>

  {#each items as item (item.kind)}
    {@const active = mail.screen === 'mail' && mail.search == null && mail.view.kind === item.kind && mail.accountId == null}
    {@const n = item.count?.() ?? 0}
    <button class="nav" class:active aria-current={active ? 'page' : undefined} onclick={() => setView({ kind: item.kind })}>
      <Icon name={item.icon} />
      <span class="trunc">{t(item.label)}</span>
      {#if n > 0}<span class="count num">{n}</span>{/if}
    </button>
    {#if item.kind === 'drafts' && (mail.counts.scheduled > 0 || mail.screen === 'scheduled')}
      {@const on = mail.screen === 'scheduled'}
      <button class="nav" class:active={on} aria-current={on ? 'page' : undefined} onclick={() => (mail.screen = 'scheduled')}>
        <Icon name="calendarClock" />
        <span class="trunc">{t('nav.scheduled')}</span>
        {#if mail.counts.scheduled > 0}<span class="count num">{mail.counts.scheduled}</span>{/if}
      </button>
    {/if}
  {/each}

  {#if tagStore.list.length}
    <div class="group">
      <span>{t('tag.title')}</span>
      <button class="btn-icon small" aria-label={t('tag.manage')} title={t('tag.manage')} onclick={() => openSettings('tags')}>
        <Icon name="settings" size={14} />
      </button>
    </div>
    {#each tagStore.list as tg (tg.id)}
      {@const on = tagActive(tg)}
      <button
        class="nav"
        class:active={on}
        aria-current={on ? 'page' : undefined}
        onclick={() => setView({ kind: 'tag', id: tg.id })}
        oncontextmenu={(e) => tagMenu(e, tg)}
        onkeydown={(e) => menuKey(e, (ev) => tagMenu(ev, tg))}
      >
        <span class="tdot" style:background={tg.color}></span>
        <span class="trunc">{tg.name}</span>
      </button>
    {/each}
  {/if}

  {#if settings.value.savedSearches.length}
    <div class="group"><span>{t('saved.nav')}</span></div>
    {#each settings.value.savedSearches as s (s.id)}
      {@const on = mail.screen === 'mail' && mail.search === s.query}
      <button
        class="nav"
        class:active={on}
        aria-current={on ? 'page' : undefined}
        title={t('saved.query', { q: s.query })}
        onclick={() => searchAll(s.query)}
        oncontextmenu={(e) => searchMenu(e, s)}
        onkeydown={(e) => menuKey(e, (ev) => searchMenu(ev, s))}
      >
        <Icon name="bookmark" />
        <span class="trunc">{s.name}</span>
      </button>
    {/each}
  {/if}

  <div class="group">
    <span>{t('nav.accounts')}</span>
    <button class="btn-icon small" aria-label={t('act.addAccount')} title={t('act.addAccount')} onclick={() => (mail.screen = 'onboarding')}>
      <Icon name="plus" size={14} />
    </button>
  </div>
  {#each mail.accounts as account (account.id)}
    {@const expanded = open.has(account.id)}
    {@const n = mail.counts.perAccountUnread[account.id] ?? 0}
    {@const active = mail.screen === 'mail' && mail.search == null && mail.view.kind === 'inbox' && mail.accountId === account.id && !expanded}
    <div class="acc" class:active>
      <button
        class="disclose"
        aria-expanded={expanded}
        aria-label={t(expanded ? 'side.collapseNamed' : 'side.expandNamed', { name: account.name })}
        onclick={() => toggleAccount(account.id)}
      >
        <Icon name="chevronRight" size={14} class={expanded ? 'turned' : ''} />
      </button>
      <button
        class="acc-main"
        title={`${account.name} · ${account.email}`}
        aria-current={active ? 'page' : undefined}
        onclick={() => setView({ kind: 'inbox' }, account.id)}
        oncontextmenu={(e) => accountMenu(e, account)}
        onkeydown={(e) => menuKey(e, (ev) => accountMenu(ev, account))}
      >
        <span class="dot" style:background={account.color}></span>
        <!-- Ad sığmazsa kısalır; alan adı yer kalmazsa alt satıra iner ve gizlenir. -->
        <span class="acc-text">
          <span class="trunc acc-name">{account.name}</span>
          <span class="trunc domain">{account.kind === 'jmap' ? 'JMAP' : domainOf(account.email)}</span>
        </span>
        {#if n > 0 && !expanded}<span class="count num">{n}</span>{/if}
      </button>
    </div>
    {#if expanded}
      <div class="tree" role="group" aria-label={t('side.folders', { name: account.name })}>
        {#each trees[account.id] ?? [] as node (node.mailbox.id)}
          {@const b = node.mailbox}
          {@const on = folderActive(account, b)}
          <div class="folder" class:active={on} style:--depth={node.depth}>
            {#if node.children.length}
              <button
                class="disclose small"
                aria-expanded={!shut.has(b.id)}
                aria-label={t(shut.has(b.id) ? 'side.expandNamed' : 'side.collapseNamed', { name: folderLabel(b) })}
                onclick={() => toggleFolder(b.id)}
              >
                <Icon name="chevronRight" size={12} class={shut.has(b.id) ? '' : 'turned'} />
              </button>
            {:else}
              <span class="disclose small" aria-hidden="true"></span>
            {/if}
            <button
              class="folder-main"
              aria-current={on ? 'page' : undefined}
              onclick={() => openFolder(account, b)}
              oncontextmenu={(e) => folderMenu(e, account, b)}
              onkeydown={(e) => menuKey(e, (ev) => folderMenu(ev, account, b))}
            >
              <Icon name={folderIcon(b)} size={16} />
              <span class="trunc grow">{folderLabel(b)}</span>
              {#if b.unread > 0 && b.role !== 'sent' && b.role !== 'drafts' && b.role !== 'trash'}<span class="count num">{b.unread}</span>{/if}
            </button>
          </div>
        {/each}
        <button class="link manage" onclick={() => openSettings(`account:${account.id}`)}>{t('side.manageFolders')}</button>
      </div>
    {/if}
  {/each}

  {#if update.info && !update.dismissed}
    <div class="update" role="status">
      <div class="u-head">
        <Icon name="arrowDown" size={16} />
        <strong class="trunc" title={t('update.available', { version: update.info.version })}>{t('update.short', { version: update.info.version })}</strong>
        <button class="btn-icon small" aria-label={t('update.later')} title={t('update.later')} onclick={() => (update.dismissed = true)}>
          <Icon name="x" size={14} />
        </button>
      </div>
      {#if update.info.notes}<p class="u-notes">{update.info.notes}</p>{/if}
      {#if update.error}<p class="u-notes err">{update.error}</p>{/if}
      <button class="btn-primary small u-install" disabled={update.installing} onclick={installUpdate}>
        {update.installing ? t('update.installing') : t('update.install')}
      </button>
    </div>
  {/if}

  <div class="foot">
    <span class="state {sync.tone}"></span>
    {#if sync.untrusted}
      <button class="link trunc foot-text warn" onclick={reviewCert}>{sync.text}</button>
    {:else}
      <span class="trunc foot-text">{sync.text}</span>
    {/if}
    <button
      class="btn-icon"
      class:on={mail.screen === 'settings'}
      aria-label={t('act.settings')}
      title={t('act.settings')}
      onclick={() => (mail.screen === 'settings' ? (mail.screen = 'mail') : openSettings())}
    >
      <Icon name="settings" />
    </button>
  </div>
</nav>

{#if cert}<CertDialog targets={cert} onclose={certClosed} />{/if}
{#if menu}<Menu at={menu.at} title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}
{#if rename}
  <NamePrompt
    at={rename.at}
    title={t('saved.rename')}
    label={t('saved.name')}
    value={rename.search.name}
    maxlength={SAVED_SEARCH_NAME_CHARS}
    onsave={(name) => rename && renameSearch(rename.search, name)}
    onclose={() => (rename = null)}
  />
{/if}

<style>
  .side {
    flex: none;
    width: 232px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 10px 10px;
    background: var(--ground);
    overflow-y: auto;
  }

  .compose {
    justify-content: flex-start;
    height: 40px;
    margin-bottom: 14px;
  }

  .on-accent {
    margin-left: auto;
    background: transparent;
    border-color: color-mix(in srgb, var(--accent-ink) 40%, transparent);
    color: inherit;
  }

  .nav {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink-2);
    text-align: left;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .nav:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .nav.active {
    background: var(--paper);
    color: var(--ink);
    font-weight: 500;
    box-shadow: 0 1px 2px rgba(20, 24, 40, 0.08);
  }

  :global(:root[data-theme='dark']) .nav.active {
    background: var(--raised);
    box-shadow: none;
  }

  .count {
    margin-left: auto;
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-2);
  }

  /* Etiket rengi kullanıcı verisidir; ikon genişliğine (18 px) hizalanır. */
  .tdot {
    flex: none;
    width: 10px;
    height: 10px;
    margin: 0 4px;
    border-radius: 50%;
  }

  .group {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 4px 6px 10px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .group .btn-icon {
    color: var(--ink-3);
  }

  .group .btn-icon:hover {
    color: var(--ink);
  }

  .acc,
  .folder {
    display: flex;
    align-items: center;
    border-radius: var(--r-icon);
    color: var(--ink-2);
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .acc:hover,
  .folder:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .acc.active,
  .folder.active {
    background: var(--paper);
    color: var(--ink);
    font-weight: 500;
    box-shadow: 0 1px 2px rgba(20, 24, 40, 0.08);
  }

  :global(:root[data-theme='dark']) .acc.active,
  :global(:root[data-theme='dark']) .folder.active {
    background: var(--raised);
    box-shadow: none;
  }

  .disclose {
    flex: none;
    display: grid;
    place-items: center;
    width: 22px;
    height: 34px;
    margin-left: 2px;
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--ink-3);
  }

  .disclose.small {
    width: 18px;
    height: 30px;
    margin-left: 0;
  }

  .disclose:hover {
    color: var(--ink);
  }

  .disclose :global(.icon) {
    transition: transform var(--t-fast) var(--ease);
  }

  .disclose :global(.turned) {
    transform: rotate(90deg);
  }

  .acc-main,
  .folder-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px 0 4px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: inherit;
    font-weight: inherit;
    text-align: left;
  }

  .folder-main {
    height: 30px;
    gap: 8px;
  }

  .tree {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 2px 0 8px;
  }

  .folder {
    /* Alt klasörler 14 px içeri; hesap okunun hizasından başlar. */
    padding-left: calc(14px + var(--depth) * 14px);
  }

  .grow {
    flex: 1;
  }

  .manage {
    align-self: flex-start;
    margin: 4px 0 0 46px;
    font-size: var(--fs-meta);
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    margin: 0 1px;
    border-radius: 50%;
  }

  .acc-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: 8px;
    height: 22px;
    overflow: hidden;
  }

  .domain {
    flex: 1 1 64px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .update {
    margin-top: auto;
    margin-bottom: 4px;
    padding: 12px;
    border-radius: var(--r-card);
    background: var(--paper);
    border: 1px solid var(--line);
    animation: fade-up var(--t-panel) var(--ease);
  }

  .u-head {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent-text);
  }

  .u-head strong {
    flex: 1;
    font-weight: 600;
    color: var(--ink);
  }

  .u-head .btn-icon {
    margin: -6px -6px -6px 0;
  }

  .u-notes {
    margin-top: 4px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .u-notes.err {
    color: var(--danger);
  }

  .u-install {
    width: 100%;
    margin-top: 10px;
  }

  .update + .foot {
    margin-top: 0;
  }

  .foot {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 0 0 10px;
  }

  .foot-text {
    flex: 1;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .foot-text.warn {
    padding: 0;
    text-align: left;
    color: var(--amber);
  }

  .state {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--ok);
  }

  .state.busy {
    background: var(--accent);
    animation: pulse 1.4s var(--ease) infinite;
  }

  .state.warn {
    background: var(--amber);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
