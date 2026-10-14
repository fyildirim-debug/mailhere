<script lang="ts">
  import { onMount } from 'svelte';
  import type { Category, ListOnly } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { folderLabel } from '../../lib/mail/folders';
  import { dateGroup, type DateGroup } from '../../lib/util/format';
  import { followUpDue } from '../../lib/util/followup';
  import { loadMore, mail, markAllRead, rowsOf, setCategory, setOnly, setView } from '../../lib/stores/mail.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import MessageRow from './MessageRow.svelte';

  const GROUP_H = 34;
  const OVERSCAN = 480;

  let scroller: HTMLDivElement;
  let scrollTop = $state(0);
  let viewport = $state(800);
  let now = $state(Date.now());

  onMount(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

  const compact = $derived(settings.value.density === 'compact');
  const isInbox = $derived(mail.view.kind === 'inbox' && mail.search == null);
  const rowH = $derived(compact ? 58 : 82);

  type Entry =
    | { kind: 'group'; key: string; label: string; top: number; h: number }
    | { kind: 'row'; key: string; index: number; top: number; h: number };

  const groupKey: Record<DateGroup, MessageKey> = {
    today: 'group.today',
    yesterday: 'group.yesterday',
    week: 'group.week',
    older: 'group.older',
  };

  const rows = $derived(rowsOf(mail.items, mail.search == null));

  const layout = $derived.by(() => {
    const entries: Entry[] = [];
    let top = 0;
    let last: DateGroup | 'due' | null = null;
    rows.forEach((m, index) => {
      // Süresi dolan hatırlatmalar gelen kutusunun başında kendi başlığıyla durur;
      // liste tarihe göre sıralı olmayabilir (arama, hatırlatma), başlık anahtarı
      // bu yüzden konuma bağlıdır.
      const g = isInbox && followUpDue(m.followUp, now) ? 'due' : dateGroup(m.date, now);
      if (g !== last) {
        const label = g === 'due' ? t('nav.followups') : t(groupKey[g]);
        entries.push({ kind: 'group', key: `g-${index}`, label, top, h: GROUP_H });
        top += GROUP_H;
        last = g;
      }
      entries.push({ kind: 'row', key: `m-${m.id}`, index, top, h: rowH });
      top += rowH;
    });
    return { entries, total: top + 16 };
  });

  function firstVisible(entries: Entry[], y: number): number {
    let lo = 0;
    let hi = entries.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (entries[mid].top + entries[mid].h < y) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  const visible = $derived.by(() => {
    const { entries } = layout;
    if (!entries.length) return [];
    const start = firstVisible(entries, scrollTop - OVERSCAN);
    const end = scrollTop + viewport + OVERSCAN;
    const out: Entry[] = [];
    for (let i = start; i < entries.length && entries[i].top < end; i++) out.push(entries[i]);
    return out;
  });

  function onScroll() {
    scrollTop = scroller.scrollTop;
    if (scrollTop + viewport > layout.total - 600) void loadMore();
  }

  // Klavyeyle seçim değişince satırı görünür tut.
  $effect(() => {
    const id = mail.selectedId;
    if (id == null || !scroller) return;
    const i = rows.findIndex((r) => r.ids.includes(id));
    const e = layout.entries.find((x) => x.kind === 'row' && x.index === i);
    if (!e) return;
    const groupAbove = layout.entries[layout.entries.indexOf(e) - 1];
    const top = groupAbove?.kind === 'group' ? groupAbove.top : e.top;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (e.top + e.h > scroller.scrollTop + viewport) scroller.scrollTop = e.top + e.h - viewport;
  });

  const tabs: { key: Category | null; label: MessageKey }[] = [
    { key: null, label: 'cat.all' },
    { key: 'people', label: 'cat.people' },
    { key: 'notifications', label: 'cat.notifications' },
    { key: 'newsletters', label: 'cat.newsletters' },
  ];

  const titleKey: Record<string, MessageKey> = {
    inbox: 'nav.inbox',
    snoozed: 'nav.snoozed',
    starred: 'nav.starred',
    drafts: 'nav.drafts',
    sent: 'nav.sent',
    archive: 'nav.archive',
    trash: 'nav.trash',
    followups: 'nav.followups',
    screener: 'nav.screener',
  };

  /** Boş durum metni: görünüme özel, yoksa genel. */
  const empty = $derived.by((): { title: MessageKey; body: MessageKey } => {
    if (mail.search == null && !mail.category && !mail.only) {
      if (mail.view.kind === 'followups') return { title: 'follow.emptyTitle', body: 'follow.emptyBody' };
      if (mail.view.kind === 'screener') return { title: 'screen.emptyTitle', body: 'screen.emptyBody' };
    }
    return { title: 'list.emptyTitle', body: mail.category || mail.only ? 'list.emptyFilteredBody' : 'list.emptyBody' };
  });

  const title = $derived.by(() => {
    if (mail.search != null) return t('list.searchTitle', { q: mail.search });
    const box = mail.view.kind === 'mailbox' ? mail.mailboxes.find((b) => b.id === (mail.view as { id: string }).id) : undefined;
    const base = mail.view.kind === 'mailbox' ? (box ? folderLabel(box) : '') : t(titleKey[mail.view.kind]);
    const acc = mail.accounts.find((a) => a.id === mail.accountId);
    return acc ? `${base} · ${acc.name}` : base;
  });

  const unread = $derived(rows.filter((m) => m.unread).length);

  const onlyKey: Record<ListOnly, MessageKey> = { unread: 'list.only.unread', starred: 'list.only.starred', attachments: 'list.only.attachments' };
  let filterMenu = $state<{ x: number; y: number } | null>(null);
  const filterItems = $derived<MenuItem[]>([
    { label: t('list.only.all'), checked: mail.only == null, onclick: () => mail.only && setOnly(mail.only) },
    ...(['unread', 'starred', 'attachments'] as ListOnly[]).map((k) => ({
      label: t(onlyKey[k]),
      checked: mail.only === k,
      onclick: () => mail.only !== k && setOnly(k),
    })),
  ]);

  function openFilter(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    filterMenu = { x: r.right - 220, y: r.bottom + 4 };
  }
</script>

<section class="list" aria-label={t('list.label')}>
  <div class="head">
    <div class="title-row">
      <h1 class="trunc">{title}</h1>
      <span class="meta num">{unread ? t('list.unread', { n: unread }) : t('list.allRead')}</span>
      {#if mail.search != null}
        <button class="btn-icon mark" aria-label={t('list.clearSearch')} title={t('list.clearSearch')} onclick={() => setView(mail.view, mail.accountId)}>
          <Icon name="x" />
        </button>
      {/if}
      {#if mail.search == null}
        <button
          class="btn-icon mark"
          class:on={mail.only != null}
          aria-label={t('list.filterBy')}
          title={t('list.filterBy')}
          aria-haspopup="menu"
          onclick={openFilter}
        >
          <Icon name="filter" />
        </button>
      {/if}
      <button class="btn-icon" aria-label={t('act.markAllRead')} title={t('act.markAllRead')} disabled={!unread} onclick={() => markAllRead()}>
        <Icon name="checkAll" />
      </button>
    </div>
    {#if mail.only && mail.search == null}
      <div class="pill-row">
        <span class="pill">
          <Icon name="filter" size={14} />{t(onlyKey[mail.only])}
          <button aria-label={t('list.clearFilter')} title={t('list.clearFilter')} onclick={() => mail.only && setOnly(mail.only)}><Icon name="x" size={12} /></button>
        </span>
      </div>
    {/if}
    {#if isInbox}
      <div class="seg-track" role="group" aria-label={t('list.filter')}>
        {#each tabs as tab (tab.label)}
          <button class="seg" aria-pressed={mail.category === tab.key} onclick={() => setCategory(tab.key)}>{t(tab.label)}</button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="scroller" bind:this={scroller} bind:clientHeight={viewport} onscroll={onScroll}>
    {#if rows.length}
      <div class="inner" style:height="{layout.total}px">
        {#each visible as entry (entry.key)}
          <div class="slot" style:transform="translateY({entry.top}px)" style:height="{entry.h}px">
            {#if entry.kind === 'group'}
              <div class="group">{entry.label}</div>
            {:else}
              <MessageRow message={rows[entry.index]} {now} {compact} />
            {/if}
          </div>
        {/each}
      </div>
    {:else if !mail.loading}
      <div class="empty">
        <Icon name="check" size={28} />
        <div class="empty-title">{t(empty.title)}</div>
        <div class="empty-body">{t(empty.body)}</div>
      </div>
    {/if}
  </div>
</section>

{#if filterMenu}<Menu at={filterMenu} title={t('list.filterBy')} items={filterItems} onclose={() => (filterMenu = null)} />{/if}

<style>
  .list {
    flex: none;
    width: 404px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border-left: 1px solid var(--line);
    border-right: 1px solid var(--line);
  }

  .head {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px 18px 12px;
  }

  .title-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }

  h1 {
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
  }

  .meta {
    flex: none;
  }

  .mark {
    margin-left: auto;
    align-self: center;
  }

  .pill-row {
    display: flex;
    margin-top: -4px;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 4px 0 10px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: var(--fs-meta);
    font-weight: 500;
  }

  .pill button {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: inherit;
  }

  .pill button:hover {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .mark:disabled {
    opacity: 0.4;
    background: transparent;
  }

  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    contain: strict;
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

  .group {
    padding: 14px 20px 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 72px 32px;
    color: var(--accent);
    text-align: center;
  }

  .empty-title {
    color: var(--ink);
    font-weight: 600;
  }

  .empty-body {
    max-width: 30ch;
    color: var(--ink-3);
  }
</style>
