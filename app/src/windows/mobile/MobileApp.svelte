<script lang="ts">
  // Mobil kabuğun kökü: gezinme yığını (nav.svelte.ts), çekmece, liste ve
  // üstüne itilen ekranlar (okuma, yazma, arama, ayarlar). Açılış akışı
  // masaüstüyle aynı (initMail); hesap yoksa mevcut hesap ekleme bileşeni gelir.
  //
  // Listede başparmak bölgesi: arama ve yeni posta sağ altta yan yana (üst
  // çubukta arama yok); liste aşağı kaydırılırken yeni posta yalnız ikona
  // iner. Çekmece yalnız menü düğmesiyle açılır: sol kenar Android'in geri
  // hareketine aittir.
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
  import LockScreen from '../../lib/ui/LockScreen.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { bindSheetLayer } from '../../lib/ui/sheet-layer';
  import ToastHost from '../../lib/ui/ToastHost.svelte';
  import { initMail, mail, markAllRead, pullMail, rowsOf, select, setOnly, setView, type Row } from '../../lib/stores/mail.svelte';
  import { initLock, isLocked, isOpen, lock, watchRelock } from '../../lib/stores/lock.svelte';
  import { signalFirstScreen } from '../../lib/ready';
  import { refreshSettings } from '../../lib/stores/settings.svelte';
  import { parseRoute } from '../../lib/stores/route.svelte';
  import { checkAll, clearChecked } from '../../lib/stores/selection.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import { askNotificationsOnce } from '../../lib/notify-permission';
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
  import { connection } from './offline.svelte';

  // Hesap ekleme sürerken (ilk hesap dahil) uygulamanın geri kalanı açılmaz.
  const setup = $derived(!mail.accounts.length || mail.screen === 'onboarding');
  // Parola kilidi: kilit ekranı tüm kabuğun yerine geçer; posta DOM'a girmez.
  const locked = $derived(isLocked(lock.status));
  let mailStarted = false;
  let wasLocked = false;
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
  /** Liste aşağı kaydırılıyor: yüzen düğme yalnız ikon. */
  let fabSmall = $state(false);
  const allChecked = $derived(selecting && mail.checked.length >= rowsOf(mail.items, mail.search == null).length);

  onMount(() => {
    // Önce kilit durumu; posta kilit yokken (ya da açılınca) yüklenir.
    void initLock();
    // Arka planda 1 dk'dan uzun kalınca dönüşte yeniden kilitlenir (tercih açıksa).
    const offRelock = watchRelock();
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
    // Liste başlığının altındaki çevrimdışı satırı.
    const offConnection = connection.watch();
    return () => {
      offRelock();
      window.removeEventListener('popstate', onPop);
      bindSheetLayer(null);
      bindComposeOpener(null);
      offOpen();
      offNav();
      offWake();
      offConnection();
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

  // Kilit yokken ya da açılınca posta yüklenir; yeniden açılışta liste yenilenir.
  $effect(() => {
    const open = isOpen(lock.status);
    const l = locked;
    if (open && !mailStarted) {
      mailStarted = true;
      void refreshSettings();
      void initMail();
    } else if (open && wasLocked && mail.ready) {
      void pullMail();
    }
    wasLocked = l;
  });

  // İlk anlamlı ekran DOM'da (kilit ekranı ya da posta yüklendi): Android
  // açılış ekranı bu sinyale dek durur, sonra doğrudan arayüze solar.
  $effect(() => {
    if (locked || (isOpen(lock.status) && mail.ready)) signalFirstScreen();
  });

  // Android 13+: ilk hesap olunca bildirim izni bir kez istenir (hesap elle
  // eklense de bilgisayardan alınsa da); sonrası Ayarlar › Bildirimler'de.
  $effect(() => {
    if (mail.ready && mail.accounts.length > 0) void askNotificationsOnce();
  });

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
</script>

<div class="m-shell">
  {#if locked}
    <div class="m-screen"><LockScreen /></div>
  {:else if setup}
    {#if mail.ready}
      <div class="m-screen setup"><Onboarding /></div>
    {/if}
  {:else}
    <!-- Liste her zaman altta durur; itilen ekranlar üstünü örter. -->
    <div class="m-screen" inert={screen.name !== 'list' || drawerOpen}>
      {#if selecting}
        <AppBar leading="close" select title={t('mob.selected', { n: mail.checked.length })} onleading={clearChecked}>
          {#snippet actions()}
            <!-- Görünür adlı düğme: tek tek dokunmadan hepsini seçmenin yolu. -->
            <button class="select-all" disabled={allChecked} onclick={checkAll}>
              <Icon name="checkAll" size={20} />{t('mob.selectAll')}
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
            <button class="btn-icon" class:on={mail.only != null} aria-label={t('act.more')} title={t('act.more')} aria-haspopup="menu" onclick={openMore}>
              <Icon name="more" size={22} />
            </button>
          {/snippet}
        </AppBar>
      {/if}

      <MobileList {title} {subtitle} onopen={openRow} onscrolled={(v) => (listScrolled = v)} ondown={(v) => (fabSmall = v)} />

      {#if selecting}
        <MobileBulkBar />
      {:else}
        <!-- Başparmak bölgesi: arama ve yeni posta altta yan yana. Aşağı kaydırırken
             yeni posta yalnız ikona iner (200 ms), yukarı kaydırınca adı geri gelir. -->
        <div class="thumb">
          <button class="m-search-fab" aria-label={t('mob.search')} title={t('mob.search')} onclick={openSearch}>
            <Icon name="search" size={22} />
          </button>
          <button
            class="btn-primary m-fab"
            class:small={fabSmall}
            aria-label={t('act.compose')}
            title={t('act.compose')}
            onclick={() => openCompose({ name: 'compose', mode: 'new', id: null, to: [], from: null })}
          >
            <Icon name="pen" size={22} />
            <span class="m-fab-label">{t('act.compose')}</span>
          </button>
        </div>
      {/if}
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
          <MobileCompose mode={screen.mode} id={screen.id} to={screen.to} from={screen.from} body={screen.body} />
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

  /* Seçim kipinde "Tümünü seç": vurgu zeminde vurgu metni, 44 px hedef. */
  .select-all {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: var(--m-touch);
    margin-right: 4px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: inherit;
    font-weight: 600;
  }

  .select-all:active {
    background: color-mix(in srgb, var(--accent-ink) 14%, transparent);
  }

  .select-all:disabled {
    opacity: var(--dim-disabled);
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
