<script lang="ts">
  // Mobil posta listesi: sanal liste (lib/ui/virtual.ts), grup başlıkları ve
  // sıra masaüstüyle aynı (lib/mail/listing.ts). Satırda kaydırma eylemleri,
  // uzun basışla seçim, aşağı çekip yenileme.
  import { onMount } from 'svelte';
  import { call, type Category } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import { emptyState, groupLabel, groupOf, isInboxView, orderRows } from '../../lib/mail/listing';
  import Icon from '../../lib/ui/Icon.svelte';
  import { buildLayout, visibleEntries } from '../../lib/ui/virtual';
  import { snoozeOptions } from '../../lib/util/format';
  import { loadMore, mail, moveTo, refreshCounts, refreshList, rowsOf, setCategory, snooze, type Row } from '../../lib/stores/mail.svelte';
  import { isChecked, openRow, toggleCheck } from '../../lib/stores/selection.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import MobileRow from './MobileRow.svelte';
  import SwipeRow from './SwipeRow.svelte';

  interface Props {
    onopen: (row: Row) => void;
  }

  let { onopen }: Props = $props();

  const GROUP_H = 34;
  const OVERSCAN = 600;
  const PULL_MAX = 96;
  const PULL_THRESHOLD = 64;

  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let viewport = $state(800);
  let now = $state(Date.now());

  onMount(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

  const compact = $derived(settings.value.density === 'compact');
  const rowH = $derived(compact ? 60 : 72);
  const isInbox = $derived(isInboxView(mail));
  const ctx = $derived({ view: mail.view, search: mail.search, now });
  const rows = $derived(orderRows(rowsOf(mail.items, mail.search == null), ctx));
  const selecting = $derived(mail.checked.length > 0);
  const empty = $derived(emptyState(mail));

  const layout = $derived(
    buildLayout(rows, { rowH, groupH: GROUP_H, tail: 96, groupOf: (m) => groupOf(m, ctx), groupLabel, rowKey: (m) => `m-${m.id}` }),
  );
  const visible = $derived(visibleEntries(layout.entries, scrollTop, viewport, OVERSCAN));

  function onScroll() {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
    if (scrollTop + viewport > layout.total - 800) void loadMore();
  }

  // Görünüm değişince başa dön.
  $effect(() => {
    void mail.view;
    void mail.accountId;
    void mail.search;
    void mail.category;
    if (scroller) scroller.scrollTop = 0;
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

  function archive(row: Row) {
    void moveTo(row.ids, 'archive');
  }

  function snoozeRow(row: Row) {
    const tomorrow = snoozeOptions(Date.now()).find((o) => o.key === 'tomorrow')!.at;
    void snooze(row.ids, tomorrow);
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
      await call('sync_now');
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
  {#if isInbox}
    <div class="filters">
      <div class="seg-track" role="group" aria-label={t('list.filter')}>
        {#each tabs as tab (tab.label)}
          <button class="seg" aria-pressed={mail.category === tab.key} onclick={() => setCategory(tab.key)}>{t(tab.label)}</button>
        {/each}
      </div>
    </div>
  {/if}

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
    {#if rows.length}
      <div class="inner" style:height="{layout.total}px">
        {#each visible as entry (entry.key)}
          <div class="slot" class:group-slot={entry.kind === 'group'} style:transform="translateY({entry.top}px)" style:height="{entry.h}px">
            {#if entry.kind === 'group'}
              <div class="group">{entry.label}</div>
            {:else}
              {@const row = rows[entry.index]}
              <SwipeRow disabled={selecting} onarchive={() => archive(row)} onsnooze={() => snoozeRow(row)} onlongpress={() => toggleCheck(row.id)}>
                <MobileRow message={row} {now} {compact} checked={isChecked(row.id)} {selecting} onopen={() => open(row)} ontoggle={() => toggleCheck(row.id)} />
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
        <span class="m-empty-icon"><Icon name="check" size={28} /></span>
        <div class="m-empty-title">{t(empty.title)}</div>
        <div class="m-empty-body">{t(empty.body)}</div>
      </div>
    {/if}
  </div>
</section>

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

  .filters {
    flex: none;
    padding: 8px 12px 6px;
  }

  .filters .seg {
    height: 34px;
    padding: 0 6px;
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

  /* Süzgeç satırı varken gösterge onun altında başlar. */
  .filters + .pull {
    top: 48px;
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
    overflow-y: auto;
    overscroll-behavior: contain;
    contain: strict;
    transition: transform var(--t-state) var(--ease);
  }

  .scroller.pulling {
    transition: none;
  }

  .inner {
    position: relative;
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
    padding: 14px 16px 6px;
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
