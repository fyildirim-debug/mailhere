<script lang="ts">
  // Sol çekmece (300 px, --ground): kenar çubuğunun birebir karşılığı.
  // Görünümler, etiketler, kayıtlı aramalar, hesaplar ve klasör ağacı, Ayarlar.
  // Dokunmayla ve kenardan kaydırmayla açılır; karartma örtüsü, Esc ya da
  // sola kaydırma kapatır. Açıkken odak içeride döner (odak tuzağı).
  import type { Account, Mailbox, Tag } from '../../lib/api';
  import { call } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import { flatten, folderIcon, folderLabel, folderTree } from '../../lib/mail/folders';
  import {
    accountActive,
    accountHint,
    COLLAPSED_FOLDERS_KEY,
    folderActive,
    folderCount,
    folderTarget,
    initialOpenAccounts,
    loadSet,
    OPEN_ACCOUNTS_KEY,
    savedSearchActive,
    saveSet,
    scheduledVisible,
    tagActive,
    toggled,
    VIEW_ITEMS,
    viewActive,
    viewCount,
    viewVisible,
  } from '../../lib/mail/nav';
  import Icon from '../../lib/ui/Icon.svelte';
  import Logo from '../../lib/ui/Logo.svelte';
  import { mail, searchAll, setView, syncLabel } from '../../lib/stores/mail.svelte';
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

  const trees = $derived(Object.fromEntries(mail.accounts.map((a) => [a.id, flatten(folderTree(mail.mailboxes, a.id), shut)])));
  const sync = $derived(syncLabel());

  function go(fn: () => void) {
    fn();
    onnavigate();
  }

  function openFolder(account: Account, b: Mailbox) {
    const target = folderTarget(account, b);
    go(() => setView(target.view, target.accountId));
  }

  function openTag(tg: Tag) {
    go(() => setView({ kind: 'tag', id: tg.id }));
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
  <div class="brand">
    <Logo size={28} />
    <span class="name">{t('app.name')}</span>
    <button class="btn-icon" aria-label={t('mob.closeDrawer')} title={t('mob.closeDrawer')} onclick={onclose}>
      <Icon name="x" size={22} />
    </button>
  </div>

  <nav class="scroll" aria-label={t('nav.label')}>
    {#each VIEW_ITEMS as item (item.kind)}
      {@const active = viewActive(item.kind, mail)}
      {@const n = viewCount(item, mail.counts)}
      {#if viewVisible(item, mail)}
        <button class="item" class:active aria-current={active ? 'page' : undefined} onclick={() => go(() => setView({ kind: item.kind }))}>
          <Icon name={item.icon} size={22} />
          <span class="trunc grow">{t(item.label)}</span>
          {#if n > 0}<span class="count num">{n}</span>{/if}
        </button>
      {/if}
      {#if item.kind === 'drafts' && scheduledVisible(mail)}
        <button class="item" onclick={onscheduled}>
          <Icon name="calendarClock" size={22} />
          <span class="trunc grow">{t('nav.scheduled')}</span>
          {#if mail.counts.scheduled > 0}<span class="count num">{mail.counts.scheduled}</span>{/if}
        </button>
      {/if}
    {/each}

    {#if tagStore.list.length}
      <div class="group"><span>{t('tag.title')}</span></div>
      {#each tagStore.list as tg (tg.id)}
        {@const on = tagActive(tg, mail)}
        <button class="item" class:active={on} aria-current={on ? 'page' : undefined} onclick={() => openTag(tg)}>
          <span class="tdot" style:background={tg.color}></span>
          <span class="trunc grow">{tg.name}</span>
        </button>
      {/each}
    {/if}

    {#if settings.value.savedSearches.length}
      <div class="group"><span>{t('saved.nav')}</span></div>
      {#each settings.value.savedSearches as s (s.id)}
        {@const on = savedSearchActive(s.query, mail)}
        <button class="item" class:active={on} aria-current={on ? 'page' : undefined} title={t('saved.query', { q: s.query })} onclick={() => go(() => searchAll(s.query))}>
          <Icon name="bookmark" size={22} />
          <span class="trunc grow">{s.name}</span>
        </button>
      {/each}
    {/if}

    <div class="group">
      <span>{t('nav.accounts')}</span>
      <button class="btn-icon small" aria-label={t('act.addAccount')} title={t('act.addAccount')} onclick={onaddaccount}>
        <Icon name="plus" size={18} />
      </button>
    </div>
    {#each mail.accounts as account (account.id)}
      {@const isOpen = expanded.has(account.id)}
      {@const n = mail.counts.perAccountUnread[account.id] ?? 0}
      {@const active = accountActive(account, isOpen, mail)}
      <div class="acc" class:active>
        <button
          class="disclose"
          aria-expanded={isOpen}
          aria-label={t(isOpen ? 'side.collapseNamed' : 'side.expandNamed', { name: account.name })}
          onclick={() => toggleAccount(account.id)}
        >
          <Icon name="chevronRight" size={16} class={isOpen ? 'turned' : ''} />
        </button>
        <button class="acc-main" aria-current={active ? 'page' : undefined} onclick={() => go(() => setView({ kind: 'inbox' }, account.id))}>
          <span class="dot" style:background={account.color}></span>
          <span class="acc-text">
            <span class="trunc acc-name">{account.name}</span>
            <span class="trunc domain">{accountHint(account)}</span>
          </span>
          {#if n > 0 && !isOpen}<span class="count num">{n}</span>{/if}
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
                  <Icon name="chevronRight" size={14} class={shut.has(b.id) ? '' : 'turned'} />
                </button>
              {:else}
                <span class="disclose" aria-hidden="true"></span>
              {/if}
              <button class="folder-main" aria-current={on ? 'page' : undefined} onclick={() => openFolder(account, b)}>
                <Icon name={folderIcon(b)} size={18} />
                <span class="trunc grow">{folderLabel(b)}</span>
                {#if folderCount(b) > 0}<span class="count num">{folderCount(b)}</span>{/if}
              </button>
            </div>
          {/each}
        </div>
      {/if}
    {/each}
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

  .brand {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--m-appbar);
    padding: 0 6px 0 16px;
  }

  .name {
    flex: 1;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overscroll-behavior: contain;
  }

  .item,
  .acc-main,
  .folder-main {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    height: var(--m-touch-lg);
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--ink-2);
    text-align: left;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .item:active,
  .acc-main:active,
  .folder-main:active {
    background: var(--hover);
  }

  .item.active,
  .acc.active,
  .folder.active {
    background: var(--paper);
    color: var(--ink);
    font-weight: 500;
    box-shadow: 0 1px 2px rgba(20, 24, 40, 0.08);
  }

  :global(:root[data-theme='dark']) .item.active,
  :global(:root[data-theme='dark']) .acc.active,
  :global(:root[data-theme='dark']) .folder.active {
    background: var(--raised);
    box-shadow: none;
  }

  .grow {
    flex: 1;
  }

  .count {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-2);
  }

  .tdot {
    flex: none;
    width: 12px;
    height: 12px;
    margin: 0 5px;
    border-radius: 50%;
  }

  .group {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 40px;
    padding: 16px 2px 0 12px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .acc,
  .folder {
    display: flex;
    align-items: center;
    border-radius: var(--r-row);
    color: var(--ink-2);
  }

  .acc-main,
  .folder-main {
    flex: 1;
    min-width: 0;
    padding-left: 4px;
    color: inherit;
    font-weight: inherit;
  }

  .disclose {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: var(--m-touch-lg);
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--ink-3);
  }

  .disclose :global(.icon) {
    transition: transform var(--t-fast) var(--ease);
  }

  .disclose :global(.turned) {
    transform: rotate(90deg);
  }

  .tree {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 2px 0 6px;
  }

  .folder {
    padding-left: calc(16px + var(--depth) * 16px);
  }

  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    margin: 0 6px;
    border-radius: 50%;
  }

  .acc-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }

  .domain {
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 6px 6px 16px;
    border-top: 1px solid var(--line);
  }

  .foot-text {
    flex: 1;
    margin-right: 6px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .foot-text.warn {
    color: var(--amber);
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
    .scrim.open {
      transition: none;
    }
  }
</style>
