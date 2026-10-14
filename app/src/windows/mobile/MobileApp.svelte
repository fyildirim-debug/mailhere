<script lang="ts">
  // Mobil kabuğun kökü: gezinme yığını (nav.svelte.ts), çekmece, liste ve
  // üstüne itilen ekranlar. Açılış akışı masaüstüyle aynı (initMail); hesap
  // yoksa mevcut hesap ekleme bileşeni gelir.
  //
  // Bu pakette okuma ekranı mevcut Reader'ı tam ekran gösterir; yazma ve
  // ayarlar ekranları sonraki paketlerde (MobileCompose, MobileSettings) gelir.
  import '../../styles/mobile.css';
  import { onMount } from 'svelte';
  import { call, EVENTS, on, type ListOnly, type MessageSummary } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import { listTitle } from '../../lib/mail/listing';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import ToastHost from '../../lib/ui/ToastHost.svelte';
  import { initMail, mail, markAllRead, rowsOf, select, setOnly, setView, type Row } from '../../lib/stores/mail.svelte';
  import { checkAll, clearChecked } from '../../lib/stores/selection.svelte';
  import { resolvedTheme, toggleTheme } from '../../lib/stores/settings.svelte';
  import { tagById } from '../../lib/stores/tags.svelte';
  import Onboarding from '../main/Onboarding.svelte';
  import Reader from '../main/Reader.svelte';
  import AppBar from './AppBar.svelte';
  import Drawer from './Drawer.svelte';
  import MobileBulkBar from './MobileBulkBar.svelte';
  import MobileList from './MobileList.svelte';
  import MobileSearch from './MobileSearch.svelte';
  import { nav } from './nav.svelte';

  // Hesap ekleme sürerken (ilk hesap dahil) uygulamanın geri kalanı açılmaz.
  const setup = $derived(!mail.accounts.length || mail.screen === 'onboarding');
  const screen = $derived(nav.current);
  const drawerOpen = $derived(nav.layer === 'drawer');
  // Mobilde tek işaret bile seçim kipidir (masaüstünde iki ve üstü panel açar).
  const selecting = $derived(mail.checked.length > 0);
  const title = $derived(
    listTitle({ view: mail.view, search: mail.search, accountId: mail.accountId, mailboxes: mail.mailboxes, accounts: mail.accounts, tagById }),
  );
  const unread = $derived(rowsOf(mail.items, mail.search == null).filter((r) => r.unread).length);
  const subtitle = $derived(unread ? t('list.unread', { n: unread }) : t('list.allRead'));

  onMount(() => {
    void initMail();
    const onPop = (e: PopStateEvent) => nav.onPopState(e.state);
    window.addEventListener('popstate', onPop);
    // Bildirimden açılan posta okuma ekranına iter.
    const off = on(EVENTS.open, (p) => {
      if (nav.current.name === 'read') nav.replace({ name: 'read', id: p.id });
      else nav.push({ name: 'read', id: p.id });
    });
    return () => {
      window.removeEventListener('popstate', onPop);
      off();
    };
  });

  // Görev çubuğu / sekme başlığında okunmamış sayısı.
  $effect(() => {
    const n = mail.counts.inboxUnread;
    document.title = n ? `(${n}) ${t('app.name')}` : t('app.name');
  });

  // Masaüstü ekran isteklerini yığına çevir: okuma menüsünden "Etiketleri düzenle"
  // gibi çağrılar `mail.screen = 'settings'` yapar; mobilde ayar ekranı itilir.
  $effect(() => {
    if (mail.screen === 'settings' || mail.screen === 'scheduled') {
      const section = mail.screen === 'settings' ? mail.settingsSection : null;
      mail.screen = 'mail';
      if (nav.current.name !== 'settings') nav.push({ name: 'settings', section });
    }
  });

  // Hesap kalmayınca (ya da ekleme akışı açılınca) yığın köke döner.
  $effect(() => {
    if (setup && nav.depth() > 0) nav.reset();
  });

  // Seçim kipi yalnız listede anlamlı.
  $effect(() => {
    if (screen.name !== 'list' && mail.checked.length) clearChecked();
  });

  function openRow(row: Row) {
    nav.push({ name: 'read', id: row.id });
  }

  /** Arama sorgusu: sonuç açılıp geri dönülünce yazılan kalır; yeni arama boş başlar. */
  let searchQ = $state('');

  function openSearch() {
    searchQ = '';
    nav.push({ name: 'search' });
  }

  function openFromSearch(m: MessageSummary) {
    void select(m.id);
    nav.push({ name: 'read', id: m.id });
  }

  // ------------------------------------------------------------- diğer menüsü

  let more = $state<{ x: number; y: number } | null>(null);
  const onlyKey: Record<ListOnly, MessageKey> = { unread: 'list.only.unread', starred: 'list.only.starred', attachments: 'list.only.attachments' };
  const moreItems = $derived.by((): MenuItem[] => {
    const items: MenuItem[] = [
      { label: t('act.markAllRead'), icon: 'checkAll', disabled: !unread, onclick: () => void markAllRead() },
      { label: t('side.syncNow'), icon: 'refresh', onclick: () => void call('sync_now') },
    ];
    if (mail.search == null) {
      items.push(
        { key: 'only-all', label: t('list.only.all'), checked: mail.only == null, onclick: () => mail.only && setOnly(mail.only) },
        ...(['unread', 'starred', 'attachments'] as ListOnly[]).map((k) => ({
          key: `only-${k}`,
          label: t(onlyKey[k]),
          checked: mail.only === k,
          onclick: () => mail.only !== k && setOnly(k),
        })),
      );
    }
    return items;
  });

  function openMore(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    more = { x: r.right - 260, y: r.bottom + 4 };
  }

  // ------------------------------------------------ kenardan kaydırarak çekmece

  let edgeStart: { x: number; y: number } | null = null;

  function edgeDown(e: PointerEvent) {
    edgeStart = { x: e.clientX, y: e.clientY };
  }

  function edgeMove(e: PointerEvent) {
    if (!edgeStart) return;
    const dx = e.clientX - edgeStart.x;
    const dy = Math.abs(e.clientY - edgeStart.y);
    if (dx > 28 && dy < dx) {
      edgeStart = null;
      nav.open('drawer');
    } else if (dy > 20) edgeStart = null;
  }

  function edgeUp() {
    edgeStart = null;
  }
</script>

<div class="m-shell">
  {#if setup}
    {#if mail.ready}
      <div class="m-screen setup"><Onboarding /></div>
    {/if}
  {:else}
    <!-- Liste her zaman altta durur; itilen ekranlar üstünü örter. -->
    <div class="m-screen" inert={screen.name !== 'list' || drawerOpen}>
      {#if selecting}
        <AppBar leading="close" select title={t('mob.selected', { n: mail.checked.length })} onleading={clearChecked}>
          {#snippet actions()}
            <button class="btn-icon" aria-label={t('bulk.selectAll')} title={t('bulk.selectAll')} onclick={checkAll}>
              <Icon name="checkAll" size={22} />
            </button>
          {/snippet}
        </AppBar>
      {:else}
        <AppBar leading="menu" {title} {subtitle} onleading={() => nav.open('drawer')}>
          {#snippet actions()}
            {#if mail.search != null}
              <button class="btn-icon" aria-label={t('list.clearSearch')} title={t('list.clearSearch')} onclick={() => setView(mail.view, mail.accountId)}>
                <Icon name="x" size={22} />
              </button>
            {/if}
            <button class="btn-icon" aria-label={t('mob.search')} title={t('mob.search')} onclick={openSearch}>
              <Icon name="search" size={22} />
            </button>
            <button class="btn-icon" class:on={mail.only != null} aria-label={t('act.more')} title={t('act.more')} aria-haspopup="menu" onclick={openMore}>
              <Icon name="more" size={22} />
            </button>
          {/snippet}
        </AppBar>
      {/if}

      <MobileList onopen={openRow} />

      {#if selecting}
        <MobileBulkBar />
      {:else}
        <button class="btn-primary m-fab" aria-label={t('act.compose')} title={t('act.compose')} onclick={() => nav.push({ name: 'compose', mode: 'new', id: null })}>
          <Icon name="pen" size={24} />
        </button>
      {/if}
      <!-- Sol kenardan sağa kaydırma çekmeceyi açar. -->
      <div class="edge" aria-hidden="true" onpointerdown={edgeDown} onpointermove={edgeMove} onpointerup={edgeUp} onpointercancel={edgeUp}></div>
    </div>

    {#if screen.name === 'read'}
      <div class="m-screen pushed">
        <AppBar leading="back" {title} onleading={() => nav.back()} />
        <div class="m-read"><Reader /></div>
      </div>
    {:else if screen.name === 'search'}
      <div class="m-screen pushed">
        <MobileSearch bind:q={searchQ} onback={() => nav.back()} onsubmit={() => nav.back()} onopen={openFromSearch} />
      </div>
    {:else if screen.name === 'compose'}
      <!-- Yer tutucu: 3c paketinde MobileCompose (ComposeWindow `embedded`) gelir. -->
      <div class="m-screen pushed">
        <AppBar leading="back" title={t('act.compose')} onleading={() => nav.back()} />
        <div class="m-empty">
          <span class="m-empty-icon"><Icon name="pen" size={28} /></span>
          <div class="m-empty-title">{t('mob.composeSoonTitle')}</div>
          <div class="m-empty-body">{t('mob.composeSoonBody')}</div>
        </div>
      </div>
    {:else if screen.name === 'settings'}
      <!-- Yer tutucu: 3d paketinde MobileSettings gelir. -->
      <div class="m-screen pushed">
        <AppBar leading="back" title={t('act.settings')} onleading={() => nav.back()} />
        <div class="m-empty">
          <span class="m-empty-icon"><Icon name="settings" size={28} /></span>
          <div class="m-empty-title">{t('mob.settingsSoonTitle')}</div>
          <div class="m-empty-body">{t('mob.settingsSoonBody')}</div>
          <button class="btn-soft" onclick={() => toggleTheme()}>
            <Icon name={resolvedTheme() === 'dark' ? 'sun' : 'moon'} size={18} />
            {t('win.theme')}
          </button>
        </div>
      </div>
    {/if}

    <Drawer
      open={drawerOpen}
      onclose={() => nav.close()}
      onnavigate={() => nav.close()}
      onsettings={() => nav.push({ name: 'settings', section: null })}
      onaddaccount={() => {
        nav.close();
        mail.screen = 'onboarding';
      }}
    />
  {/if}

  {#if more}<Menu at={more} items={moreItems} onclose={() => (more = null)} />{/if}
  <ToastHost />
</div>

<style>
  .setup {
    overflow-y: auto;
  }

  .edge {
    position: absolute;
    top: calc(var(--m-appbar) + var(--sat));
    bottom: 0;
    left: 0;
    width: 20px;
    z-index: 4;
    touch-action: pan-y;
  }

  .m-read {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--paper);
  }

  /* Mevcut okuma alanı dar ekrana uyarlanır; 3b paketi MobileReader'ı yazar. */
  .m-read :global(.reader .toolbar) {
    padding: 0 8px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .m-read :global(.reader .toolbar::-webkit-scrollbar) {
    display: none;
  }

  .m-read :global(.reader .article) {
    padding: 20px 16px 16px;
  }

  .m-read :global(.reader .reply-wrap) {
    padding: 8px 12px calc(12px + var(--sab));
  }

  /* Toast yüzen düğmenin ve alt çubuğun üstünde durur. */
  :global(.m-shell .toast[role='status']) {
    bottom: calc(88px + var(--sab));
  }
</style>
