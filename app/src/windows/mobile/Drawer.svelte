<script lang="ts">
  // Sol çekmece (300 px, --ground): kenar çubuğunun birebir karşılığı, aynı
  // bilgi mimarisi. Hesap kapsamı (yalnız çok hesapta), kapsama göre daralan
  // görünümler, her hesabın istenmeyen + özel klasörleri, etiketler, kayıtlı
  // aramalar; altta eşitleme durumu, Şimdi eşitle, tema ve Ayarlar.
  // Menü düğmesiyle açılır (kenardan kaydırma sistemin geri hareketine
  // bırakılır); karartma örtüsü, Esc ya da sola kaydırma kapatır. Açıkken odak
  // içeride döner (odak tuzağı).
  import type { Account, Mailbox, Tag } from '../../lib/api';
  import { call } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
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
  import Logo from '../../lib/ui/Logo.svelte';
  import { quietScroll } from '../../lib/ui/scroll';
  import { mail, openSettings, searchAll, setView, syncLabel } from '../../lib/stores/mail.svelte';
  import { resolvedTheme, settings, toggleTheme } from '../../lib/stores/settings.svelte';
  import { tagStore } from '../../lib/stores/tags.svelte';

  interface Props {
    open: boolean;
    onclose: () => void;
    /** Çekmeceden bir görünüm seçilince (çekmece kapanır). */
    onnavigate: () => void;
    onsettings: () => void;
    /** Planlananlar (yalnız bekleyen gönderim varsa listelenir). */
    onscheduled: () => void;
    onaddaccount: () => void;
  }

  let { open, onclose, onnavigate, onsettings, onscheduled, onaddaccount }: Props = $props();

  const multi = $derived(mail.accounts.length > 1);
  const scope = $derived(mail.accountId);
  const folderList = $derived(folderAccounts(mail.accounts, scope));
  const collapsible = $derived(multi && scope == null);

  // Açık hesaplar ve kapalı klasörler: masaüstü kenar çubuğuyla aynı anahtar.
  let expanded = $state<Set<string>>(initialOpenAccounts(mail.accounts));
  let shut = $state<Set<string>>(loadSet(COLLAPSED_FOLDERS_KEY) ?? new Set());

  function toggleAccount(id: string) {
    expanded = toggled(expanded, id);
    saveSet(OPEN_ACCOUNTS_KEY, expanded);
  }

  function toggleFolder(id: string) {
    shut = toggled(shut, id);
    saveSet(COLLAPSED_FOLDERS_KEY, shut);
  }

  const trees = $derived(Object.fromEntries(mail.accounts.map((a) => [a.id, flatten(sideTree(mail.mailboxes, a.id), shut)])));
  const sync = $derived(syncLabel());

  function go(fn: () => void) {
    fn();
    onnavigate();
  }

  function pickScope(id: string | null) {
    go(() => setView(scopeView(mail), id));
  }

  function openFolder(account: Account, b: Mailbox) {
    const target = folderTarget(account, b);
    go(() => setView(target.view, target.accountId));
  }

  function openTag(tg: Tag) {
    go(() => setView({ kind: 'tag', id: tg.id }, scope));
  }

  /** Hesabın klasör ayarları: ekran isteğini kabuk ayar ekranı olarak iter, çekmece kapanır. */
  function manage(account: Account) {
    openSettings(`account:${account.id}`);
  }

  // ----------------------------------------------------------- odak ve kaydırma

  let panel = $state<HTMLElement>();
  let returnTo: HTMLElement | null = null;

  $effect(() => {
    if (!open || !panel) return;
    returnTo = document.activeElement as HTMLElement | null;
    // Odak hemen içeri (inert kalktı); açılış animasyonu kaydırmasın.
    panel.querySelector<HTMLElement>('button:not(:disabled)')?.focus({ preventScroll: true });
    return () => {
      returnTo?.focus?.({ preventScroll: true });
    };
  });

  /** Esc, odak nerede olursa olsun çekmeceyi kapatır. */
  function onWindowKey(e: KeyboardEvent) {
    if (!open || e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Tab' || !panel) return;
    const list = [...panel.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input')];
    if (!list.length) return;
    const first = list[0];
    const last = list[list.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  // Sola kaydırarak kapatma: yalnız transform; bırakınca ya kapanır ya yerine döner.
  let dx = $state(0);
  let dragging = $state(false);
  let start: { x: number; y: number } | null = null;

  function down(e: PointerEvent) {
    start = { x: e.clientX, y: e.clientY };
    dragging = false;
  }

  function move(e: PointerEvent) {
    if (!start) return;
    const mx = e.clientX - start.x;
    const my = e.clientY - start.y;
    if (!dragging) {
      if (Math.abs(mx) < 12 || Math.abs(mx) < Math.abs(my)) {
        if (Math.abs(my) > 12) start = null;
        return;
      }
      dragging = true;
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    }
    dx = Math.min(0, mx);
  }

  function up(e: PointerEvent) {
    if (dragging && e.type !== 'pointercancel' && panel && -dx > panel.clientWidth / 3) {
      dx = 0;
      dragging = false;
      start = null;
      onclose();
      return;
    }
    dx = 0;
    dragging = false;
    start = null;
  }
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="scrim" class:open role="presentation" onclick={onclose}></div>
<div
  class="drawer"
  class:open
  class:dragging
  role="dialog"
  aria-modal="true"
  aria-label={t('nav.label')}
  aria-hidden={!open}
  inert={!open}
  bind:this={panel}
  style:transform={open ? `translateX(${dx}px)` : undefined}
  onkeydown={onKey}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
>
  <!-- Tek hesapta başlık kimliktir (ad + adres) ve hesap ayarlarına götürür; çok hesapta marka. -->
  <div class="head">
    {#if !multi && mail.accounts[0]}
      {@const a = mail.accounts[0]}
      <button class="who" title={t('side.accountSettings')} onclick={() => manage(a)}>
        <span class="badge" style:--c={a.color} aria-hidden="true">{a.name.slice(0, 1).toLocaleUpperCase('tr')}</span>
        <span class="who-text">
          <span class="trunc who-name">{a.name}</span>
          <span class="trunc who-mail">{a.email}</span>
        </span>
      </button>
    {:else}
      <span class="brand">
        <Logo size={28} />
        <span class="name">{t('app.name')}</span>
      </span>
    {/if}
    <button class="btn-icon" aria-label={t('mob.closeDrawer')} title={t('mob.closeDrawer')} onclick={onclose}>
      <Icon name="x" size={22} />
    </button>
  </div>

  <nav class="scroll scroll-quiet" aria-label={t('nav.label')} use:quietScroll>
    {#if multi}
      <div class="m-section group">
        <span class="group-title">{t('nav.accounts')}</span>
        <button class="btn-icon small" aria-label={t('act.addAccount')} title={t('act.addAccount')} onclick={onaddaccount}>
          <Icon name="plus" size={18} />
        </button>
      </div>
      <div class="scopes" role="group" aria-label={t('nav.accounts')}>
        {#snippet scopeRow(id: string | null, name: string, hint: string, color: string | null, n: number)}
          {@const on = scopeActive(id, mail)}
          <button class="item acc" class:active={on} aria-pressed={on} onclick={() => pickScope(id)}>
            <span class="dot" class:all={!color} style:background={color ?? undefined}></span>
            <span class="acc-text">
              <span class="trunc">{name}</span>
              <span class="trunc hint">{hint}</span>
            </span>
            {#if n > 0}<span class="count num">{n}</span>{/if}
          </button>
        {/snippet}
        {@render scopeRow(null, t('nav.allAccounts'), t('nav.accountsCount', { n: mail.accounts.length }), null, 0)}
        {#each mail.accounts as account (account.id)}
          {@render scopeRow(account.id, account.name, accountHint(account), account.color, mail.counts.perAccountUnread[account.id] ?? 0)}
        {/each}
      </div>
    {/if}

    <div class="views">
      {#each VIEW_ITEMS as item (item.kind)}
        {@const active = viewActive(item.kind, mail)}
        {@const n = viewCount(item, mail.counts, scope)}
        {#if viewVisible(item, mail)}
          <button class="item view" class:active aria-current={active ? 'page' : undefined} onclick={() => go(() => setView({ kind: item.kind }, scope))}>
            <Icon name={item.icon} size={22} />
            <span class="trunc grow">{t(item.label)}</span>
            {#if n > 0}<span class="count num">{n}</span>{/if}
          </button>
        {/if}
        {#if item.kind === 'drafts' && scheduledVisible(mail)}
          <button class="item view" onclick={onscheduled}>
            <Icon name="calendarClock" size={22} />
            <span class="trunc grow">{t('nav.scheduled')}</span>
            {#if mail.counts.scheduled > 0}<span class="count num">{mail.counts.scheduled}</span>{/if}
          </button>
        {/if}
      {/each}
    </div>

    <!-- Klasörler: hesap başlığı altında istenmeyen + özel klasörler. -->
    {#each folderList as account (account.id)}
      {@const isOpen = !collapsible || expanded.has(account.id)}
      <div class="m-section group">
        {#if collapsible}
          <button
            class="group-toggle"
            aria-expanded={isOpen}
            aria-label={t(isOpen ? 'side.collapseNamed' : 'side.expandNamed', { name: account.name })}
            onclick={() => toggleAccount(account.id)}
          >
            <span class="dot" style:background={account.color}></span>
            <span class="trunc grow">{account.name}</span>
            <Icon name="chevronRight" size={16} class="chev {isOpen ? 'turned' : ''}" />
          </button>
        {:else if multi}
          <span class="group-title">
            <span class="dot" style:background={account.color}></span>
            <span class="trunc grow">{account.name}</span>
          </span>
        {:else}
          <!-- Tek hesapta kimlik başlıkta; burada yalnız bölüm adı. -->
          <span class="group-title">{t('mob.folders')}</span>
        {/if}
        <button class="btn-icon small" aria-label={t('side.manageNamed', { name: account.name })} title={t('side.manage')} onclick={() => manage(account)}>
          <Icon name="settings" size={18} />
        </button>
      </div>
      {#if isOpen}
        <div class="tree" role="group" aria-label={t('side.folders', { name: account.name })}>
          {#each trees[account.id] ?? [] as node (node.mailbox.id)}
            {@const b = node.mailbox}
            {@const on = folderActive(account, b, mail)}
            <!-- Simge görünümlerle aynı sütunda; aç/kapa oku yalnız alt klasörü olanda, sonda. -->
            <div class="folder" class:active={on} style:--depth={node.depth}>
              <button class="item folder-main" aria-current={on ? 'page' : undefined} onclick={() => openFolder(account, b)}>
                <Icon name={folderIcon(b)} size={22} />
                <span class="trunc grow">{folderLabel(b)}</span>
                {#if folderCount(b) > 0}<span class="count num">{folderCount(b)}</span>{/if}
              </button>
              {#if node.children.length}
                <button
                  class="disclose"
                  aria-expanded={!shut.has(b.id)}
                  aria-label={t(shut.has(b.id) ? 'side.expandNamed' : 'side.collapseNamed', { name: folderLabel(b) })}
                  onclick={() => toggleFolder(b.id)}
                >
                  <Icon name="chevronRight" size={16} class="chev {shut.has(b.id) ? '' : 'turned'}" />
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/each}

    {#if tagStore.list.length}
      <div class="m-section group"><span class="group-title">{t('tag.title')}</span></div>
      {#each tagStore.list as tg (tg.id)}
        {@const on = tagActive(tg, mail)}
        <button class="item" class:active={on} aria-current={on ? 'page' : undefined} onclick={() => openTag(tg)}>
          <span class="tdot" style:background={tg.color}></span>
          <span class="trunc grow">{tg.name}</span>
        </button>
      {/each}
    {/if}

    {#if settings.value.savedSearches.length}
      <div class="m-section group"><span class="group-title">{t('saved.nav')}</span></div>
      {#each settings.value.savedSearches as s (s.id)}
        {@const on = savedSearchActive(s.query, mail)}
        <button class="item" class:active={on} aria-current={on ? 'page' : undefined} title={t('saved.query', { q: s.query })} onclick={() => go(() => searchAll(s.query))}>
          <Icon name="bookmark" size={22} />
          <span class="trunc grow">{s.name}</span>
        </button>
      {/each}
    {/if}

    {#if !multi}
      <button class="item quiet" onclick={onaddaccount}>
        <Icon name="plus" size={22} />
        <span class="trunc grow">{t('act.addAccount')}</span>
      </button>
    {/if}
  </nav>

  <div class="foot">
    <span class="state {sync.tone}"></span>
    <span class="trunc foot-text" class:warn={sync.tone === 'warn'}>{sync.text}</span>
    <button class="btn-icon" aria-label={t('side.syncNow')} title={t('side.syncNow')} onclick={() => void call('sync_now')}>
      <Icon name="refresh" size={20} />
    </button>
    <button class="btn-icon" aria-label={t('win.theme')} title={t('win.theme')} onclick={() => toggleTheme()}>
      <Icon name={resolvedTheme() === 'dark' ? 'sun' : 'moon'} size={20} />
    </button>
    <button class="btn-icon" aria-label={t('act.settings')} title={t('act.settings')} onclick={onsettings}>
      <Icon name="settings" size={20} />
    </button>
  </div>
</div>

<style>
  .scrim {
    position: absolute;
    inset: 0;
    z-index: 20;
    background: color-mix(in srgb, var(--ink) 42%, transparent);
    opacity: 0;
    visibility: hidden;
    transition:
      opacity var(--t-state) var(--ease),
      visibility 0s var(--t-state);
  }

  .scrim.open {
    opacity: 1;
    visibility: visible;
    transition: opacity var(--t-state) var(--ease);
  }

  .drawer {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 21;
    width: min(var(--m-drawer), 86vw);
    display: flex;
    flex-direction: column;
    padding-top: var(--sat);
    padding-bottom: var(--sab);
    padding-left: var(--sal);
    background: var(--ground);
    box-shadow: var(--shadow-lg);
    transform: translateX(-104%);
    visibility: hidden;
    touch-action: pan-y;
    transition:
      transform var(--t-panel) var(--ease),
      visibility 0s var(--t-panel);
  }

  .drawer.open {
    transform: none;
    visibility: visible;
    transition: transform var(--t-panel) var(--ease);
  }

  .drawer.dragging {
    transition: none;
  }

  /* ------------------------------------------------------------------ başlık */

  .head {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 64px;
    padding: 6px 6px 6px 8px;
  }

  .brand {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding-left: 8px;
  }

  .name {
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 4px 8px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--ink);
    text-align: left;
    transition: background var(--t-fast);
  }

  .who:active {
    background: var(--hover);
  }

  /* Hesap rozeti: hesap rengi kullanıcı verisidir; zemin aynı tonun %18'i,
     harf aynı ton mürekkeple koyulaşır (--ink temayla döner, iki temada ≥ 4.5:1). */
  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--c) 18%, transparent);
    color: color-mix(in srgb, var(--c) 60%, var(--ink));
    font-weight: 600;
  }

  .who-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }

  .who-name {
    font-weight: 600;
  }

  .who-mail {
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  /* --------------------------------------------------------------- kaydırma */

  .scroll {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 6px 12px 8px;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
  }

  /* Taşan sütunda satırlar büzülmesin: klasörü çok hesapta görünümler yazı
     yüksekliğine iniyor, klasör bloğu taşıp son satır kaydırılamıyordu. */
  .scroll > :global(*) {
    flex-shrink: 0;
  }

  .views,
  .scopes,
  .tree {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .scopes {
    margin-bottom: 8px;
  }

  /* ---------------------------------------------------------------- satırlar */

  .item {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    min-height: var(--m-nav-row);
    padding: 0 10px 0 12px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--ink-2);
    text-align: left;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .item:active {
    background: var(--hover);
  }

  /* Görünümler birincil gezinmedir: 500; klasörler 400 ve simgeleri daha sessiz. */
  .item.view {
    font-weight: 500;
  }

  .folder-main :global(.icon) {
    color: var(--ink-3);
  }

  .item.active {
    background: var(--paper);
    color: var(--ink);
    font-weight: 600;
    box-shadow: var(--seg-on-shadow);
  }

  :global(:root[data-theme='dark']) .item.active {
    background: var(--raised);
    box-shadow: none;
  }

  .grow {
    flex: 1;
  }

  .count {
    flex: none;
    margin-left: auto;
    min-width: 2ch;
    font-size: var(--fs-meta);
    font-weight: 500;
    text-align: right;
    color: var(--ink-3);
  }

  .item.active .count,
  .folder.active .count {
    color: var(--ink);
    font-weight: 600;
  }

  .quiet {
    margin-top: 12px;
  }

  .tdot {
    flex: none;
    width: 12px;
    height: 12px;
    margin: 0 5px;
    border-radius: 50%;
  }

  /* -------------------------------------------------------------- kapsam */

  .acc {
    min-height: 56px;
  }

  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    margin: 0 6px;
    border-radius: 50%;
  }

  .dot.all {
    box-sizing: border-box;
    border: 1.6px solid currentColor;
  }

  .acc-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }

  .hint {
    font-size: var(--fs-meta);
    font-weight: 400;
    color: var(--ink-2);
  }

  /* ----------------------------------------------------------- bölümler */

  .group {
    padding-left: 4px;
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

  .group + .group {
    padding-top: 0;
    min-height: 32px;
  }

  .group .btn-icon {
    color: var(--ink-3);
  }

  .group-toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 40px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
  }

  .group-toggle:active {
    background: var(--hover);
  }

  :global(.drawer .chev) {
    flex: none;
    color: var(--ink-3);
    transition: transform var(--t-fast) var(--ease);
  }

  :global(.drawer .chev.turned) {
    transform: rotate(90deg);
  }

  /* ------------------------------------------------------------ klasörler */

  .folder {
    display: flex;
    align-items: center;
    padding-left: calc(var(--depth) * 20px);
    border-radius: var(--r-row);
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
    font-weight: 600;
  }

  .folder.active .folder-main :global(.icon) {
    color: var(--ink);
  }

  .folder-main {
    flex: 1;
    min-width: 0;
  }

  .folder.active .folder-main,
  .folder-main:active {
    background: transparent;
    box-shadow: none;
  }

  .disclose {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--m-touch);
    height: var(--m-nav-row);
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--ink-3);
  }

  /* ---------------------------------------------------------------- alt */

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 6px 6px 16px;
    border-top: 1px solid var(--line);
  }

  .foot .btn-icon {
    color: var(--ink-3);
  }

  .foot-text {
    flex: 1;
    margin-right: 6px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .foot-text.warn {
    color: var(--amber-text);
  }

  .state {
    flex: none;
    width: 7px;
    height: 7px;
    margin-right: 8px;
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
    .drawer,
    .drawer.open,
    .scrim,
    .scrim.open,
    :global(.drawer .chev) {
      transition: none;
    }
  }
</style>
