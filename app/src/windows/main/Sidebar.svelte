<script lang="ts">
  // Kenar çubuğu (248 px, --ground). Yukarıdan aşağı: Yeni posta; hesap
  // kapsamı (yalnız çok hesapta: "Tüm hesaplar" ya da bir hesap); kapsama göre
  // daralan görünümler; her hesabın istenmeyen + özel klasörleri; etiketler;
  // kayıtlı aramalar. Rol klasörleri görünümlerle sunulduğu için ağaçta
  // yinelenmez. Altta güncelleme kartı ve eşitleme durumu + Ayarlar.
  import type { Account, Mailbox, SavedSearch, Tag } from '../../lib/api';
  import { call, toAppError, SAVED_SEARCH_NAME_CHARS } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import CertDialog, { type CertTarget } from '../../lib/mail/CertDialog.svelte';
  import { flatten, folderIcon, folderLabel } from '../../lib/mail/folders';
  import {
    accountHint,
    COLLAPSED_FOLDERS_KEY,
    folderAccounts,
    folderActive,
    folderCount,
    folderTarget,
    initialOpenAccounts,
    loadSet,
    OPEN_ACCOUNTS_KEY,
    savedSearchActive,
    saveSet,
    scheduledVisible,
    scopeActive,
    scopeView,
    sideTree,
    tagActive,
    toggled,
    VIEW_ITEMS,
    viewActive,
    viewCount,
    viewVisible,
  } from '../../lib/mail/nav';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import NamePrompt from '../../lib/ui/NamePrompt.svelte';
  import { quietScroll } from '../../lib/ui/scroll';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { mail, openSettings, reloadMailboxes, searchAll, setView, syncLabel } from '../../lib/stores/mail.svelte';
  import { settings, updateSettings } from '../../lib/stores/settings.svelte';
  import { tagStore } from '../../lib/stores/tags.svelte';
  import { installUpdate, update } from '../../lib/stores/update.svelte';
  import { toggleSidebar } from './layout.svelte';

  // Görünüm listesi ve etkinlik kuralları mobil çekmeceyle ortak (lib/mail/nav.ts).

  interface Props {
    /** Genişlik (px); sütun ayırıcısıyla değişir. */
    width?: number;
    /** Dar pencerede ray üstünde açılan katman (Ctrl B). */
    overlay?: boolean;
  }

  let { width = 248, overlay = false }: Props = $props();

  const multi = $derived(mail.accounts.length > 1);
  /** Hesap kapsamı: depodaki `accountId`; `null` tüm hesaplar. */
  const scope = $derived(mail.accountId);
  /** Klasör bölümünde listelenen hesaplar (kapsamdaki ya da hepsi). */
  const folderList = $derived(folderAccounts(mail.accounts, scope));
  /** Hesap grupları yalnız "Tüm hesaplar" kapsamında ve çok hesapta katlanır. */
  const collapsible = $derived(multi && scope == null);

  function pickScope(id: string | null) {
    setView(scopeView(mail), id);
  }

  // ------------------------------------------------ açık hesaplar ve klasörler
  // Kişisel kolaylık: yalnız bu bilgisayarda, tarayıcı deposunda tutulur.

  let open = $state<Set<string>>(initialOpenAccounts(mail.accounts));
  let shut = $state<Set<string>>(loadSet(COLLAPSED_FOLDERS_KEY) ?? new Set());

  function toggleAccount(id: string) {
    open = toggled(open, id);
    saveSet(OPEN_ACCOUNTS_KEY, open);
  }

  function toggleFolder(id: string) {
    shut = toggled(shut, id);
    saveSet(COLLAPSED_FOLDERS_KEY, shut);
  }

  const trees = $derived(Object.fromEntries(mail.accounts.map((a) => [a.id, flatten(sideTree(mail.mailboxes, a.id), shut)])));

  function openFolder(account: Account, b: Mailbox) {
    const target = folderTarget(account, b);
    setView(target.view, target.accountId);
  }

  // ------------------------------------------------------------ sağ tık menüsü

  let menu = $state<{ at: { x: number; y: number }; title: string; items: MenuItem[] } | null>(null);

  function pointOf(e: MouseEvent | KeyboardEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return e instanceof MouseEvent && e.clientX ? { x: e.clientX, y: e.clientY } : { x: r.left + 24, y: r.bottom };
  }

  /** Klavyeyle menü: Shift+F10 ya da menü tuşu. */
  function menuKey(e: KeyboardEvent, show: (e: KeyboardEvent) => void) {
    if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) show(e);
  }

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
    menu = {
      at: pointOf(e),
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
    menu = {
      at: pointOf(e),
      title: account.name,
      items: [
        { label: t('side.syncNow'), icon: 'refresh', onclick: () => void call('sync_now') },
        { label: t('side.accountSettings'), icon: 'settings', onclick: () => openSettings(`account:${account.id}`) },
      ],
    };
  }

  // ------------------------------------------------ etiketler ve kayıtlı aramalar

  function openTag(tg: Tag) {
    setView({ kind: 'tag', id: tg.id }, scope);
  }

  function tagMenu(e: MouseEvent | KeyboardEvent, tg: Tag) {
    e.preventDefault();
    menu = {
      at: pointOf(e),
      title: tg.name,
      items: [
        { label: t('tag.open'), icon: 'tag', onclick: () => openTag(tg) },
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

  // ------------------------------------------------------------------- durum

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

<nav class="side" class:overlay style:width="{width}px" aria-label={t('nav.label')}>
  <button class="btn-primary compose" title="{t('act.compose')} (C)" onclick={() => call('open_compose', { prefill: { mode: 'new', messageId: null } })}>
    <Icon name="pen" />
    <span class="trunc">{t('act.compose')}</span>
    <span class="key" aria-hidden="true">C</span>
  </button>

  <div class="scroll scroll-quiet" use:quietScroll>
    {#if multi}
      <div class="group">
        <span class="group-title">{t('nav.accounts')}</span>
        <button class="btn-icon small" aria-label={t('act.addAccount')} title={t('act.addAccount')} onclick={() => (mail.screen = 'onboarding')}>
          <Icon name="plus" size={14} />
        </button>
      </div>
      <div class="scopes" role="group" aria-label={t('nav.accounts')}>
        {#snippet scopeRow(id: string | null, name: string, hint: string, color: string | null, n: number, account: Account | null)}
          {@const on = scopeActive(id, mail)}
          <button
            class="nav acc"
            class:active={on}
            aria-pressed={on}
            title={account ? `${account.name} · ${account.email}` : undefined}
            onclick={() => pickScope(id)}
            oncontextmenu={account ? (e) => accountMenu(e, account) : undefined}
            onkeydown={account ? (e) => menuKey(e, (ev) => accountMenu(ev, account)) : undefined}
          >
            <span class="dot" class:all={!color} style:background={color ?? undefined}></span>
            <span class="acc-text">
              <span class="trunc">{name}</span>
              <span class="trunc hint">{hint}</span>
            </span>
            {#if n > 0}<span class="count num">{n}</span>{/if}
          </button>
        {/snippet}
        {@render scopeRow(null, t('nav.allAccounts'), t('nav.accountsCount', { n: mail.accounts.length }), null, 0, null)}
        {#each mail.accounts as account (account.id)}
          {@render scopeRow(account.id, account.name, accountHint(account), account.color, mail.counts.perAccountUnread[account.id] ?? 0, account)}
        {/each}
      </div>
    {/if}

    <div class="views">
      {#each VIEW_ITEMS as item (item.kind)}
        {@const active = viewActive(item.kind, mail)}
        {@const n = viewCount(item, mail.counts, scope)}
        {#if viewVisible(item, mail)}
          <button class="nav" class:active aria-current={active ? 'page' : undefined} onclick={() => setView({ kind: item.kind }, scope)}>
            <Icon name={item.icon} size={16} />
            <span class="trunc">{t(item.label)}</span>
            {#if n > 0}<span class="count num">{n}</span>{/if}
          </button>
        {/if}
        {#if item.kind === 'drafts' && scheduledVisible(mail)}
          {@const on = mail.screen === 'scheduled'}
          <button class="nav" class:active={on} aria-current={on ? 'page' : undefined} onclick={() => (mail.screen = 'scheduled')}>
            <Icon name="calendarClock" size={16} />
            <span class="trunc">{t('nav.scheduled')}</span>
            {#if mail.counts.scheduled > 0}<span class="count num">{mail.counts.scheduled}</span>{/if}
          </button>
        {/if}
      {/each}
    </div>

    <!-- Klasörler: hesap başlığı altında istenmeyen + özel klasörler. -->
    {#each folderList as account (account.id)}
      {@const isOpen = !collapsible || open.has(account.id)}
      <div class="group">
        {#if collapsible}
          <button
            class="group-toggle"
            aria-expanded={isOpen}
            aria-label={t(isOpen ? 'side.collapseNamed' : 'side.expandNamed', { name: account.name })}
            title={`${account.name} · ${account.email}`}
            onclick={() => toggleAccount(account.id)}
            oncontextmenu={(e) => accountMenu(e, account)}
            onkeydown={(e) => menuKey(e, (ev) => accountMenu(ev, account))}
          >
            <span class="dot" style:background={account.color}></span>
            <span class="trunc grow">{account.name}</span>
            <Icon name="chevronRight" size={14} class="chev {isOpen ? 'turned' : ''}" />
          </button>
        {:else}
          <!-- Tek hesapta kimlik yalnız burada görünür; çok hesapta kapsam satırları zaten söyler. -->
          <span class="group-title" title={`${account.name} · ${account.email}`}>
            <span class="dot" style:background={account.color}></span>
            <span class="trunc grow">{account.name}</span>
            {#if !multi}<span class="hint">{accountHint(account)}</span>{/if}
          </span>
        {/if}
        <button
          class="btn-icon small"
          aria-label={t('side.manageNamed', { name: account.name })}
          title={t('side.manage')}
          onclick={() => openSettings(`account:${account.id}`)}
        >
          <Icon name="settings" size={14} />
        </button>
      </div>
      {#if isOpen}
        <div class="tree" role="group" aria-label={t('side.folders', { name: account.name })}>
          {#each trees[account.id] ?? [] as node (node.mailbox.id)}
            {@const b = node.mailbox}
            {@const on = folderActive(account, b, mail)}
            <div class="folder" class:active={on} style:--depth={node.depth}>
              {#if node.children.length}
                <button
                  class="disclose"
                  aria-expanded={!shut.has(b.id)}
                  aria-label={t(shut.has(b.id) ? 'side.expandNamed' : 'side.collapseNamed', { name: folderLabel(b) })}
                  onclick={() => toggleFolder(b.id)}
                >
                  <Icon name="chevronRight" size={14} class="chev {shut.has(b.id) ? '' : 'turned'}" />
                </button>
              {:else}
                <span class="disclose" aria-hidden="true"></span>
              {/if}
              <button
                class="nav folder-main"
                aria-current={on ? 'page' : undefined}
                onclick={() => openFolder(account, b)}
                oncontextmenu={(e) => folderMenu(e, account, b)}
                onkeydown={(e) => menuKey(e, (ev) => folderMenu(ev, account, b))}
              >
                <Icon name={folderIcon(b)} size={16} />
                <span class="trunc">{folderLabel(b)}</span>
                {#if folderCount(b) > 0}<span class="count num">{folderCount(b)}</span>{/if}
              </button>
            </div>
          {/each}
        </div>
      {/if}
    {/each}

    {#if tagStore.list.length}
      <div class="group">
        <span class="group-title">{t('tag.title')}</span>
        <button class="btn-icon small" aria-label={t('tag.manage')} title={t('tag.manage')} onclick={() => openSettings('tags')}>
          <Icon name="settings" size={14} />
        </button>
      </div>
      {#each tagStore.list as tg (tg.id)}
        {@const on = tagActive(tg, mail)}
        <button
          class="nav"
          class:active={on}
          aria-current={on ? 'page' : undefined}
          onclick={() => openTag(tg)}
          oncontextmenu={(e) => tagMenu(e, tg)}
          onkeydown={(e) => menuKey(e, (ev) => tagMenu(ev, tg))}
        >
          <span class="tdot" style:background={tg.color}></span>
          <span class="trunc">{tg.name}</span>
        </button>
      {/each}
    {/if}

    {#if settings.value.savedSearches.length}
      <div class="group"><span class="group-title">{t('saved.nav')}</span></div>
      {#each settings.value.savedSearches as s (s.id)}
        {@const on = savedSearchActive(s.query, mail)}
        <button
          class="nav"
          class:active={on}
          aria-current={on ? 'page' : undefined}
          title={t('saved.query', { q: s.query })}
          onclick={() => searchAll(s.query)}
          oncontextmenu={(e) => searchMenu(e, s)}
          onkeydown={(e) => menuKey(e, (ev) => searchMenu(ev, s))}
        >
          <Icon name="bookmark" size={16} />
          <span class="trunc">{s.name}</span>
        </button>
      {/each}
    {/if}

    {#if !multi}
      <button class="nav quiet" onclick={() => (mail.screen = 'onboarding')}>
        <Icon name="plus" size={16} />
        <span class="trunc">{t('act.addAccount')}</span>
      </button>
    {/if}
  </div>

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
      title="{t('act.settings')} · Ctrl ,"
      onclick={() => (mail.screen === 'settings' ? (mail.screen = 'mail') : openSettings())}
    >
      <Icon name="settings" />
    </button>
    <button class="btn-icon" aria-label={t('desk.collapse')} title="{t('desk.collapse')} · Ctrl B" onclick={toggleSidebar}>
      <Icon name="sidebar" />
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
    width: 248px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 12px 0 8px;
    background: var(--ground);
  }

  /* Dar pencerede ray üstünde açılan tam kenar çubuğu: kenarsız, gölgeyle. */
  .side.overlay {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 25;
    box-shadow: var(--shadow-lg);
    animation: side-in var(--t-state) var(--ease);
  }

  @keyframes side-in {
    from {
      opacity: 0;
      transform: translateX(-16px);
    }
  }

  /* Yeni posta: kenar çubuğunun tek birincil düğmesi; kısayol harfi sakin, çipsiz. */
  .compose {
    flex: none;
    justify-content: flex-start;
    margin: 0 10px 10px;
    padding: 0 12px;
  }

  .key {
    margin-left: auto;
    font-size: var(--fs-meta);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    color: color-mix(in srgb, var(--accent-ink) 72%, transparent);
  }

  /* Kaydırılan bölüm: sağda ince çubuk için yer ayrılır (scrollbar-gutter). */
  .scroll {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0 2px 8px 10px;
    overflow-y: auto;
    scrollbar-gutter: stable;
  }

  /* Taşan sütunda satırlar büzülmesin (çok klasörlü hesapta görünümler
     sıkışıyor, son klasör kaydırılamıyordu). */
  .scroll > :global(*) {
    flex-shrink: 0;
  }

  .views,
  .scopes,
  .tree {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .scopes {
    margin-bottom: 8px;
  }

  /* ---------------------------------------------------------------- satırlar */

  .nav {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 34px;
    padding: 0 8px 0 10px;
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
    box-shadow: var(--seg-on-shadow);
  }

  :global(:root[data-theme='dark']) .nav.active {
    background: var(--raised);
    box-shadow: none;
  }

  /* Sayaçlar sağda tek sütunda, eşit genişlikli rakamlarla. */
  .count {
    flex: none;
    margin-left: auto;
    min-width: 2ch;
    font-size: var(--fs-meta);
    font-weight: 500;
    text-align: right;
    color: var(--ink-2);
  }

  .nav.active .count {
    color: var(--ink);
  }

  .quiet {
    margin-top: 12px;
  }

  /* Etiket rengi kullanıcı verisidir; ikon genişliğine (16 px) hizalanır. */
  .tdot {
    flex: none;
    width: 10px;
    height: 10px;
    margin: 0 3px;
    border-radius: 50%;
  }

  /* -------------------------------------------------------------- kapsam */

  /* Hesap satırı iki satırlı: ad ve alan adı ayrı ayrı kısalır, hiçbiri kaybolmaz. */
  .acc {
    height: 40px;
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    margin: 0 5px;
    border-radius: 50%;
  }

  /* "Tüm hesaplar": içi boş halka, hesap noktalarıyla aynı ölçü. */
  .dot.all {
    box-sizing: border-box;
    border: 1.5px solid currentColor;
  }

  .acc-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }

  .hint {
    flex: none;
    font-size: var(--fs-meta);
    font-weight: 400;
    color: var(--ink-2);
  }

  /* ----------------------------------------------------------- bölümler */

  .group {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    margin-top: 12px;
    padding: 0 0 0 2px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .group-title {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 8px;
  }

  .group-title .dot,
  .group-toggle .dot {
    margin: 0 4px 0 0;
  }

  .grow {
    flex: 1;
  }

  /* Ardışık katlı başlıklar sıkı durur. */
  .group + .group {
    margin-top: 4px;
  }

  /* Bölüm başlığındaki yönetim düğmeleri kenar çubuğu kullanılırken belirir
     (üzerine gelme ya da klavye odağı); dinlenirken başlıklar yalnız metindir. */
  .group .btn-icon {
    color: var(--ink-2);
    opacity: 0;
    transition:
      opacity var(--t-fast),
      background var(--t-fast),
      color var(--t-fast);
  }

  .side:hover .group .btn-icon,
  .side:focus-within .group .btn-icon {
    opacity: 1;
  }

  .group .btn-icon:hover {
    color: var(--ink);
  }

  /* Katlanır hesap başlığı: satırın tamamı aç/kapa düğmesi. */
  .group-toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 6px 0 8px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .group-toggle:hover {
    background: var(--hover);
    color: var(--ink);
  }

  :global(.side .chev) {
    flex: none;
    color: var(--ink-2);
    transition: transform var(--t-fast) var(--ease);
  }

  :global(.side .chev.turned) {
    transform: rotate(90deg);
  }

  /* ------------------------------------------------------------ klasörler */

  .folder {
    display: flex;
    align-items: center;
    /* Alt klasörler 14 px içeri. */
    padding-left: calc(var(--depth) * 14px);
    border-radius: var(--r-icon);
    transition: background var(--t-fast);
  }

  .folder:hover {
    background: var(--hover);
  }

  .folder.active {
    background: var(--paper);
    box-shadow: var(--seg-on-shadow);
  }

  :global(:root[data-theme='dark']) .folder.active {
    background: var(--raised);
    box-shadow: none;
  }

  .folder.active .folder-main {
    color: var(--ink);
    font-weight: 500;
  }

  .folder.active .count {
    color: var(--ink);
  }

  .folder-main {
    flex: 1;
    min-width: 0;
    padding-left: 2px;
  }

  .folder:hover .folder-main {
    color: var(--ink);
  }

  .folder.active .folder-main,
  .folder-main:hover,
  .folder:hover .folder-main {
    background: transparent;
    box-shadow: none;
  }

  .disclose {
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 34px;
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--ink-2);
  }

  button.disclose:hover {
    color: var(--ink);
  }

  /* ---------------------------------------------------------- güncelleme */

  .update {
    flex: none;
    margin: 8px 10px 0;
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

  /* ---------------------------------------------------------------- alt */

  /* Alt satır ayırıcısız: boşluk yeter. */
  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    margin: 6px 6px 0 12px;
  }

  .foot .state {
    margin-right: 6px;
  }

  .foot-text {
    flex: 1;
    padding-left: 2px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .foot-text.warn {
    padding: 0 0 0 2px;
    text-align: left;
    color: var(--amber-text);
  }

  .state {
    width: 7px;
    height: 7px;
    flex: none;
    margin-left: 2px;
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

  @media (prefers-reduced-motion: reduce) {
    :global(.side .chev) {
      transition: none;
    }
  }
</style>
