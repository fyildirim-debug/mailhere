<script lang="ts">
  // Mobil posta listesi: sanal liste (lib/ui/virtual.ts), grup başlıkları ve
  // sıra masaüstüyle aynı (lib/mail/listing.ts). Satırda kaydırma eylemleri,
  // uzun basışla seçim, aşağı çekip yenileme.
  //
  // Büyük başlık (24 px) ve kategori çipleri listenin içinde, kaydırılan
  // alanın başında durur: içerik akınca başlık uygulama çubuğuna çekilir
  // (`onscrolled`), çipler sayfanın üstüne yapışır. Sanal yerleşim başlığın
  // yüksekliği kadar kaydırılarak okunur.
  import { onMount, untrack } from 'svelte';
  import { fade } from 'svelte/transition';
  import { call, type Category, type SwipeAction } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import { folderLabel } from '../../lib/mail/folders';
  import { emptyState, groupLabel, groupOf, isInboxView, orderRows } from '../../lib/mail/listing';
  import Icon from '../../lib/ui/Icon.svelte';
  import { buildLayout, visibleEntries } from '../../lib/ui/virtual';
  import { snoozeOptions } from '../../lib/util/format';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { bulkMoveToFolder, bulkSetUnread, setPinned } from '../../lib/stores/bulk.svelte';
  import { loadMore, mail, moveTo, refreshCounts, refreshList, rowsOf, setCategory, snooze, type Row } from '../../lib/stores/mail.svelte';
  import { isChecked, openRow, toggleCheck } from '../../lib/stores/selection.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import { categoryUnread } from './counts.svelte';
  import { junkOf, moveItems } from './folders';
  import { haptic } from './haptic';
  import MobileRow from './MobileRow.svelte';
  import { connection } from './offline.svelte';
  import SwipeRow, { removesRow, type SwipeLook } from './SwipeRow.svelte';

  interface Props {
    title: string;
    subtitle: string;
    onopen: (row: Row) => void;
    /** Büyük başlık görünür alandan çıkınca true: uygulama çubuğu başlığı gösterir. */
    onscrolled?: (scrolled: boolean) => void;
    /** Kaydırma yönü: aşağı inerken true (yüzen düğme daralır), yukarı ya da baştayken false. */
    ondown?: (down: boolean) => void;
  }

  let { title, subtitle, onopen, onscrolled, ondown }: Props = $props();

  const OVERSCAN = 600;
  const PULL_MAX = 96;
  const PULL_THRESHOLD = 64;
  /** Yön değişimi bu kadar kaydırmadan sonra sayılır (titremesin). */
  const DIR_SLOP = 12;

  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let viewport = $state(800);
  /** Yalnız büyük başlığın (çipler hariç) yüksekliği: çubuk başlığının eşiği. */
  let titleH = $state(0);
  /** Çip şeridinin yüksekliği (gelen kutusu dışında 0). */
  let chipsH = $state(0);
  let now = $state(Date.now());

  onMount(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

  /**
   * Sistem yazı ölçeği: Android WebView kullanıcının yazı büyütmesini px
   * yazılara da uygular. Bir satır yüksekliğindeki ölçü metninin gerçek boyu
   * 20 px'e bölünür; sanal liste satırları bu oranla büyür, metin kırpılmaz.
   */
  let probeH = $state(20);
  const textScale = $derived(Math.max(1, probeH / 20));
  const compact = $derived(settings.value.density === 'compact');
  const rowH = $derived(Math.round((compact ? 60 : 72) * textScale));
  const groupH = $derived(Math.round(34 * textScale));
  const isInbox = $derived(isInboxView(mail));
  /** Kaydırılan alanın başındaki başlık bloğu; sanal yerleşim bunun altından başlar. */
  const headH = $derived(titleH + (isInbox ? chipsH : 0));
  const ctx = $derived({ view: mail.view, search: mail.search, now });
  const rows = $derived(orderRows(rowsOf(mail.items, mail.search == null), ctx));
  const selecting = $derived(mail.checked.length > 0);
  const empty = $derived(emptyState(mail));

  const layout = $derived(
    buildLayout(rows, { rowH, groupH, tail: 104, groupOf: (m) => groupOf(m, ctx), groupLabel, rowKey: (m) => `m-${m.id}` }),
  );
  const visible = $derived(visibleEntries(layout.entries, scrollTop - headH, viewport, OVERSCAN));
  /** Birleşik görünüm (birden çok hesap, hesap seçili değil): satırda hesap rengi karesi. */
  const unified = $derived(mail.accountId == null && mail.accounts.length > 1);
  const accountById = $derived(new Map(mail.accounts.map((a) => [a.id, a])));

  let scrolled = $state(false);
  let down = false;
  /** Geçerli yöndeki en uç konum: ters yöne DIR_SLOP kadar gidilince yön değişir. */
  let anchor = 0;

  function setDown(v: boolean) {
    anchor = scrollTop;
    if (v === down) return;
    down = v;
    ondown?.(v);
  }

  function onScroll() {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
    const past = scrollTop > Math.max(24, titleH - 8);
    if (past !== scrolled) {
      scrolled = past;
      onscrolled?.(past);
    }
    // Aşağı inerken yüzen düğme daralır; yukarı çıkınca ya da başa varınca açılır.
    if (scrollTop <= 8) setDown(false);
    else if (down ? scrollTop > anchor : scrollTop < anchor) anchor = scrollTop;
    else if (Math.abs(scrollTop - anchor) > DIR_SLOP) setDown(!down);
    if (scrollTop + viewport > headH + layout.total - 800) void loadMore();
  }

  // Görünüm değişince başa dön. Yerleşim ölçüleri (başlık yüksekliği, satır
  // sayısı) izlenmez: onlar değişince kaydırma konumu korunur.
  $effect(() => {
    void mail.view;
    void mail.accountId;
    void mail.search;
    void mail.category;
    untrack(() => {
      if (!scroller) return;
      scroller.scrollTop = 0;
      onScroll();
    });
  });

  const tabs: { key: Category | null; label: MessageKey }[] = [
    { key: null, label: 'cat.all' },
    { key: 'people', label: 'cat.people' },
    { key: 'notifications', label: 'cat.notifications' },
    { key: 'newsletters', label: 'cat.newsletters' },
  ];

  function open(row: Row) {
    openRow(row.id);
    onopen(row);
  }

  /** Kaydırma eyleminin görünümü; okundu ve sabitle satırın durumuna göre adlanır. */
  function lookOf(row: Row, a: SwipeAction): SwipeLook | null {
    switch (a) {
      case 'archive':
        return { icon: 'archive', label: t('act.archive'), tone: 'accent' };
      case 'snooze':
        return { icon: 'clock', label: t('snooze.tomorrow'), tone: 'amber' };
      case 'trash':
        return { icon: 'trash', label: t('act.delete'), tone: 'danger' };
      case 'spam':
        return { icon: 'spam', label: t('mob.swipe.spam'), tone: 'danger' };
      case 'move':
        return { icon: 'folder', label: t('mob.swipe.move'), tone: 'neutral' };
      case 'toggleRead':
        return { icon: 'mail', label: row.unread ? t('mob.read') : t('mob.unread'), tone: 'accent' };
      case 'pin':
        return { icon: 'pin', label: row.pinned ? t('pin.unpin') : t('pin.pin'), tone: 'neutral' };
      default:
        return null;
    }
  }

  /** İstenmeyen kutusu olmayan hesapta satır yerinde kalır. */
  function swipeRemoves(row: Row, a: SwipeAction): boolean {
    return a === 'spam' ? junkOf(row.accountId) != null : removesRow(a);
  }

  /** "Klasöre taşı" kaydırmasının açtığı seçici (alt sayfa). */
  let moving = $state<{ items: MenuItem[] } | null>(null);

  /** Kaydırma eylemi; her biri mevcut geri alma toast'ıyla gelir. */
  function swipeAction(row: Row, a: SwipeAction) {
    switch (a) {
      case 'archive':
      case 'trash':
        void moveTo(row.ids, a);
        break;
      case 'spam': {
        const junk = junkOf(row.accountId);
        if (junk) void bulkMoveToFolder(row.ids, junk, folderLabel(junk));
        else {
          haptic('reject');
          showToast({ text: t('mob.swipe.noJunk') });
        }
        break;
      }
      case 'move':
        moving = { items: moveItems(row.accountId, row.mailboxId, (box, label) => void bulkMoveToFolder(row.ids, box, label)) };
        break;
      case 'snooze': {
        const tomorrow = snoozeOptions(Date.now()).find((o) => o.key === 'tomorrow')!.at;
        void snooze(row.ids, tomorrow);
        break;
      }
      case 'toggleRead':
        void bulkSetUnread(row.ids, !row.unread);
        break;
      case 'pin':
        void setPinned(row.ids, !row.pinned);
        break;
    }
  }

  function longPress(row: Row) {
    haptic('confirm');
    toggleCheck(row.id);
  }

  // Kategori çiplerinin okunmamış sayıları: sayaçlar ve hesap değişince yenilenir.
  $effect(() => {
    if (!isInbox) return;
    void mail.counts;
    void mail.accountId;
    untrack(() => void categoryUnread.refresh(mail.accountId));
  });

  function chipLabel(label: string, n: number): string | undefined {
    return n ? t('mob.chipUnread', { name: label, n }) : undefined;
  }

  // ------------------------------------------------------- aşağı çekip yenileme
  // Dokunmada tarayıcı dikey kaydırmayı üstlendiği için touch olayları
  // (pasif olmayan) kullanılır; fare için pointer olayları yeter.

  let pull = $state(0);
  let pulling = $state(false);
  let refreshing = $state(false);
  let pullStart: number | null = null;

  async function refresh() {
    if (refreshing) return;
    refreshing = true;
    pull = 56;
    try {
      // Elle yenileme: yarı ölü bağlantı beklenmeden baştan kurulur.
      await call('sync_reconnect');
      await refreshList();
      await refreshCounts();
    } finally {
      refreshing = false;
      pull = 0;
    }
  }

  function pullBegin(y: number) {
    if (refreshing || !scroller || scroller.scrollTop > 0) return;
    pullStart = y;
    pulling = false;
  }

  /** Çekme sürerken true döner (tarayıcı kaydırmasın). */
  function pullMove(y: number): boolean {
    if (pullStart == null || !scroller) return false;
    const dy = y - pullStart;
    if (scroller.scrollTop > 0 || dy <= 0) {
      if (!pulling) pullStart = null;
      pull = 0;
      return false;
    }
    pulling = true;
    pull = Math.min(PULL_MAX, dy * 0.5);
    return true;
  }

  function pullEnd() {
    if (pullStart == null) return;
    pullStart = null;
    const go = pulling && pull >= PULL_THRESHOLD;
    pulling = false;
    if (go) void refresh();
    else pull = 0;
  }

  function touchable(node: HTMLElement) {
    const onStart = (e: TouchEvent) => pullBegin(e.touches[0].clientY);
    const onMove = (e: TouchEvent) => {
      if (pullMove(e.touches[0].clientY) && e.cancelable) e.preventDefault();
    };
    const onEnd = () => pullEnd();
    node.addEventListener('touchstart', onStart, { passive: true });
    node.addEventListener('touchmove', onMove, { passive: false });
    node.addEventListener('touchend', onEnd);
    node.addEventListener('touchcancel', onEnd);
    return {
      destroy() {
        node.removeEventListener('touchstart', onStart);
        node.removeEventListener('touchmove', onMove);
        node.removeEventListener('touchend', onEnd);
        node.removeEventListener('touchcancel', onEnd);
      },
    };
  }

  function mouseDown(e: PointerEvent) {
    if (e.pointerType === 'mouse' && e.button === 0) pullBegin(e.clientY);
  }

  function mouseMove(e: PointerEvent) {
    if (e.pointerType === 'mouse') pullMove(e.clientY);
  }

  function mouseUp(e: PointerEvent) {
    if (e.pointerType === 'mouse') pullEnd();
  }

  const pullText = $derived(refreshing ? t('mob.refreshing') : pull >= PULL_THRESHOLD ? t('mob.release') : t('mob.pull'));
</script>

<section class="list" aria-label={t('list.label')}>
  <div class="pull" class:on={pull > 0} aria-live="polite" style:transform="translateY({pull - 48}px)">
    <span class="pull-icon" class:spin={refreshing} style:transform="rotate({refreshing ? 0 : Math.min(180, pull * 2.8)}deg)">
      <Icon name="refresh" size={18} />
    </span>
    <span>{pull > 0 ? pullText : ''}</span>
  </div>

  <div
    class="scroller"
    role="presentation"
    class:pulling
    bind:this={scroller}
    bind:clientHeight={viewport}
    use:touchable
    onscroll={onScroll}
    onpointerdown={mouseDown}
    onpointermove={mouseMove}
    onpointerup={mouseUp}
    onpointercancel={mouseUp}
    style:transform={pull ? `translateY(${pull}px)` : undefined}
  >
    <!-- Büyük başlık: seçim kipinde h1 uygulama çubuğundadır ("2 seçili"); burada yalnız metin kalır. -->
    <!-- Başlık ve çipler kaydırıcının doğrudan çocukları: çipler içerik boyunca üste yapışır. -->
    <div class="titles" bind:clientHeight={titleH}>
        <svelte:element this={selecting ? 'div' : 'h1'} class="big">{title}</svelte:element>
        <span class="sub num">{subtitle}</span>
        <!-- Çevrimdışı durum satırı: bağlantı gelince söner (200 ms). -->
        {#if connection.offline}
          <span class="status num" role="status" transition:fade={{ duration: connection.reduced ? 0 : 200 }}>
            {connection.pending ? t('mob.offlinePending', { n: connection.pending }) : t('mob.offline')}
          </span>
        {/if}
    </div>
    {#if isInbox}
      <div class="chips" role="group" aria-label={t('list.filter')} bind:clientHeight={chipsH}>
        {#each tabs as tab (tab.label)}
          {@const n = categoryUnread.of(tab.key)}
          <button class="m-chip" aria-pressed={mail.category === tab.key} aria-label={chipLabel(t(tab.label), n)} onclick={() => setCategory(tab.key)}>
            {t(tab.label)}{#if n}<span class="m-chip-n num" aria-hidden="true">{n > 99 ? '99+' : n}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}
    <!-- Yazı ölçeği ölçüsü (görünmez): satır yüksekliği bununla büyür. -->
    <span class="probe" aria-hidden="true" bind:clientHeight={probeH}>Ag</span>

    {#if rows.length}
      <div class="inner" style:height="{layout.total}px">
        {#each visible as entry (entry.key)}
          <div class="slot" class:group-slot={entry.kind === 'group'} style:transform="translateY({entry.top}px)" style:height="{entry.h}px">
            {#if entry.kind === 'group'}
              <div class="group">{entry.label}</div>
            {:else}
              {@const row = rows[entry.index]}
              <SwipeRow
                disabled={selecting}
                left={settings.value.swipeLeft}
                right={settings.value.swipeRight}
                look={(a) => lookOf(row, a)}
                removes={(a) => swipeRemoves(row, a)}
                onaction={(a) => swipeAction(row, a)}
                onlongpress={() => longPress(row)}
              >
                <MobileRow
                  message={row}
                  {now}
                  {compact}
                  checked={isChecked(row.id)}
                  {selecting}
                  account={unified ? accountById.get(row.accountId) : undefined}
                  onopen={() => open(row)}
                  ontoggle={() => toggleCheck(row.id)}
                />
              </SwipeRow>
            {/if}
          </div>
        {/each}
      </div>
    {:else if mail.loading}
      <div class="m-empty">
        <span class="meta">{t('mob.loading')}</span>
      </div>
    {:else}
      <div class="m-empty">
        <span class="m-empty-icon"><Icon name="check" size={26} /></span>
        <div class="m-empty-title">{t(empty.title)}</div>
        <div class="m-empty-body">{t(empty.body)}</div>
      </div>
    {/if}
  </div>
</section>

{#if moving}<Menu title={t('read.moveTo')} items={moving.items} onclose={() => (moving = null)} />{/if}

<style>
  .list {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    overflow: hidden;
  }

  .pull {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 48px;
    color: var(--ink-2);
    font-size: var(--fs-meta);
    font-weight: 500;
    opacity: 0;
    pointer-events: none;
    transition:
      transform var(--t-state) var(--ease),
      opacity var(--t-fast);
  }

  .pull.on {
    opacity: 1;
  }

  .pull-icon {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--paper);
    color: var(--accent);
    box-shadow: var(--shadow-sm);
  }

  .pull-icon.spin {
    animation: spin 0.9s linear infinite;
  }

  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overscroll-behavior: contain;
    contain: strict;
    transition: transform var(--t-state) var(--ease);
  }

  .scroller.pulling {
    transition: none;
  }

  /* ------------------------------------------------------------ büyük başlık */

  .titles {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px var(--m-gutter) 12px;
  }

  .big {
    font-size: var(--fs-title);
    font-weight: 600;
    line-height: 1.25;
    letter-spacing: -0.02em;
    overflow-wrap: anywhere;
    text-wrap: balance;
  }

  .sub {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-3);
  }

  /* Çevrimdışı: tek satır, sessiz; ne olduğu + ne bekleneceği. */
  .status {
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .probe {
    position: absolute;
    top: 0;
    left: 0;
    visibility: hidden;
    pointer-events: none;
    font-size: var(--fs-body);
    line-height: var(--m-line);
    white-space: nowrap;
  }

  /* Çipler yatay kayar; içerik akınca sayfanın üstüne yapışır. */
  .chips {
    flex: none;
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    gap: 8px;
    padding: 2px var(--m-gutter) 10px;
    background: var(--surface);
    overflow-x: auto;
    scrollbar-width: none;
  }

  .chips::-webkit-scrollbar {
    display: none;
  }

  /* ------------------------------------------------------------------ liste */

  .inner {
    position: relative;
    flex: none;
  }

  .slot {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
  }

  .group-slot {
    z-index: 1;
  }

  .group {
    padding: 14px var(--m-gutter) 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pull,
    .scroller {
      transition: none;
    }

    .pull-icon.spin {
      animation: none;
    }
  }
</style>
