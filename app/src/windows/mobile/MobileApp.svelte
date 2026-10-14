<script lang="ts">
  // Mobil kabuğun kökü: gezinme yığını (nav.svelte.ts), çekmece, liste ve
  // üstüne itilen ekranlar (okuma, yazma, arama, ayarlar). Açılış akışı
  // masaüstüyle aynı (initMail); hesap yoksa mevcut hesap ekleme bileşeni gelir.
  //
  // Ayarlar (MobileSettings) ve planlananlar (ScheduledView `embedded`) yığına
  // itilen ekranlardır. Alt sayfalar (menüler) ve yazma ekranı açılışta gezinme
  // yığınına bağlanır: Sheet katmanı `nav.open('sheet')`, yazma istekleri
  // (`openCompose`, Android'in `mh:navigate` olayı) yığına itilen MobileCompose.
  import '../../styles/mobile.css';
  import { onMount } from 'svelte';
  import { call, EVENTS, on, type ComposePrefill, type ListOnly, type MessageSummary } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import { listTitle } from '../../lib/mail/listing';
  import { bindComposeOpener } from '../../lib/mail/open';
  import { watchWake } from '../../lib/mail/wake';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { bindSheetLayer } from '../../lib/ui/sheet-layer';
  import ToastHost from '../../lib/ui/ToastHost.svelte';
  import { initMail, mail, markAllRead, rowsOf, select, setOnly, setView, type Row } from '../../lib/stores/mail.svelte';
  import { parseRoute } from '../../lib/stores/route.svelte';
  import { checkAll, clearChecked } from '../../lib/stores/selection.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import { tagById } from '../../lib/stores/tags.svelte';
  import Onboarding from '../main/Onboarding.svelte';
  import ScheduledView from '../main/ScheduledView.svelte';
  import AppBar from './AppBar.svelte';
  import Drawer from './Drawer.svelte';
  import MobileBulkBar from './MobileBulkBar.svelte';
  import MobileCompose from './MobileCompose.svelte';
  import MobileList from './MobileList.svelte';
  import MobileReader from './MobileReader.svelte';
  import MobileSearch from './MobileSearch.svelte';
  import MobileSettings from './MobileSettings.svelte';
  import { nav, type MobileScreen } from './nav.svelte';

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
  /** Listenin büyük başlığı kaydırılıp gitti mi: uygulama çubuğu başlığı o zaman gösterir. */
  let listScrolled = $state(false);

  onMount(() => {
    void initMail();
    const onPop = (e: PopStateEvent) => nav.onPopState(e.state);
    window.addEventListener('popstate', onPop);
    // Alt sayfalar yığının katmanı olur: geri tuşu önce onları kapatır.
    bindSheetLayer({
      get active() {
        return nav.layer === 'sheet';
      },
      show: () => nav.open('sheet'),
      hide: () => nav.close(),
    });
    // Yazma istekleri (okuma ekranı, kişi kartı) yeni pencere yerine yığına.
    bindComposeOpener((p) => openCompose(composeScreen(p)));
    // Bildirimden açılan posta okuma ekranına iter.
    // Yazma ekranının üstüne itilmez: sökülen yazma taslağa kaydedilir ve geri
    // dönünce boş bir yazma ekranı açılırdı. Yerine geçer; taslak Taslaklar'da.
    const offOpen = on(EVENTS.open, (p) => {
      if (nav.current.name === 'read' || nav.current.name === 'compose') nav.replace({ name: 'read', id: p.id });
      else nav.push({ name: 'read', id: p.id });
    });
    // Android kabuğu (Platform::open_compose) yazma rotasını olayla verir.
    const offNav = on(EVENTS.navigate, (p) => {
      const r = parseRoute(p.route);
      if (r.name === 'compose') openCompose({ name: 'compose', mode: r.mode, id: r.id, to: r.to, from: r.from });
    });
    // Arka plandan dönüşte ve ağ değişince ölü bağlantılar yenilenir.
    const offWake = watchWake({ background: true });
    return () => {
      window.removeEventListener('popstate', onPop);
      bindSheetLayer(null);
      bindComposeOpener(null);
      offOpen();
      offNav();
      offWake();
    };
  });

  type ComposeScreen = Extract<MobileScreen, { name: 'compose' }>;

  function composeScreen(p: ComposePrefill): ComposeScreen {
    return { name: 'compose', mode: p.mode, id: p.messageId, to: p.to ?? [], from: p.accountId ?? null };
  }

  /** Yazma ekranını iter; zaten yazılıyorsa yerine geçer (eskisi sökülürken taslağa yazılır). */
  function openCompose(screen: ComposeScreen) {
    if (nav.current.name === 'compose') nav.replace(screen);
    else nav.push(screen);
  }

  // Android sistem çubuklarının simgeleri uygulamanın temasını izler (telefon
  // açık kipteyken uygulama koyu temada olabilir).
  $effect(() => {
    const dark = settings.resolved === 'dark';
    call('system_bars', { dark }).catch(() => {
      /* masaüstü tarayıcıda ve eski kabukta komut yok; çubuklar sistem temasında kalır */
    });
  });

  // Görev çubuğu / sekme başlığında okunmamış sayısı.
  $effect(() => {
    const n = mail.counts.inboxUnread;
    document.title = n ? `(${n}) ${t('app.name')}` : t('app.name');
  });

  // Masaüstü ekran isteklerini yığına çevir: okuma menüsünden "Etiketleri düzenle"
  // ya da ayar bölümlerinden `openSettings('account:…')` gibi çağrılar
  // `mail.screen = 'settings'` yapar; mobilde istenen bölüm ekran olarak itilir.
  // Aynı bölüm zaten üstteyse yeniden itilmez.
  $effect(() => {
    if (mail.screen !== 'settings' && mail.screen !== 'scheduled') return;
    const target: MobileScreen = mail.screen === 'settings' ? { name: 'settings', section: mail.settingsSection } : { name: 'scheduled' };
    mail.screen = 'mail';
    const cur = nav.current;
    if (cur.name === 'scheduled' && target.name === 'scheduled') return;
    if (cur.name === 'settings' && target.name === 'settings' && cur.section === target.section) return;
    nav.push(target);
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
        <AppBar leading="menu" {title} {subtitle} heading={false} titleVisible={listScrolled} tone="surface" divided={listScrolled} onleading={() => nav.open('drawer')}>
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

      <MobileList {title} {subtitle} onopen={openRow} onscrolled={(v) => (listScrolled = v)} />

      {#if selecting}
        <MobileBulkBar />
      {:else}
        <button class="btn-primary m-fab" aria-label={t('act.compose')} title={t('act.compose')} onclick={() => openCompose({ name: 'compose', mode: 'new', id: null, to: [], from: null })}>
          <Icon name="pen" size={22} />
          <span>{t('act.compose')}</span>
        </button>
      {/if}
      <!-- Sol kenardan sağa kaydırma çekmeceyi açar. -->
      <div class="edge" aria-hidden="true" onpointerdown={edgeDown} onpointermove={edgeMove} onpointerup={edgeUp} onpointercancel={edgeUp}></div>
    </div>

    {#if screen.name === 'read'}
      <div class="m-screen pushed">
        <MobileReader id={screen.id} />
      </div>
    {:else if screen.name === 'search'}
      <div class="m-screen pushed">
        <MobileSearch bind:q={searchQ} onback={() => nav.back()} onsubmit={() => nav.back()} onopen={openFromSearch} />
      </div>
    {:else if screen.name === 'compose'}
      <!-- Her yazma isteği yeni ekran: yerine geçen istek eskisini söker (taslağa yazar). -->
      {#key screen}
        <div class="m-screen pushed">
          <MobileCompose mode={screen.mode} id={screen.id} to={screen.to} from={screen.from} />
        </div>
      {/key}
    {:else if screen.name === 'settings'}
      <!-- Her bölüm ayrı itilen ekran: `screen` nesnesi değişince yeniden kurulur ve giriş hareketi oynar. -->
      {#key screen}
        <div class="m-screen pushed">
          <MobileSettings
            section={screen.section}
            onback={() => nav.back()}
            onopen={(section) => nav.push({ name: 'settings', section })}
            onaddaccount={() => (mail.screen = 'onboarding')}
          />
        </div>
      {/key}
    {:else if screen.name === 'scheduled'}
      <div class="m-screen pushed">
        <AppBar leading="back" title={t('nav.scheduled')} onleading={() => nav.back()} />
        <div class="m-sched"><ScheduledView embedded /></div>
      </div>
    {/if}

    <Drawer
      open={drawerOpen}
      onclose={() => nav.close()}
      onnavigate={() => nav.close()}
      onsettings={() => nav.push({ name: 'settings', section: null })}
      onscheduled={() => nav.push({ name: 'scheduled' })}
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
  /* Hesap ekleme tam ekran: güvenli alan kenarlıkları, Onboarding'in mobil
     kapsayıcı sorgularının kökü (`container: onboarding`). */
  .setup {
    overflow-y: auto;
    padding: var(--sat) var(--sar) var(--sab) var(--sal);
    background: var(--paper);
    container: onboarding / inline-size;
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

  .m-sched {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--paper);
  }

  /* Toast yüzen düğmenin ve alt çubuğun üstünde durur. */
  :global(.m-shell .toast[role='status']) {
    bottom: calc(88px + var(--sab));
  }
</style>
