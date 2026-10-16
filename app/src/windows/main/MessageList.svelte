<script lang="ts">
  import { onMount } from 'svelte';
  import { toAppError, SAVED_SEARCHES_MAX, SAVED_SEARCH_NAME_CHARS, type Category, type ListOnly } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import NamePrompt from '../../lib/ui/NamePrompt.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { emptyState, groupLabel, groupOf, isInboxView, listTitle, orderRows } from '../../lib/mail/listing';
  import { buildLayout, scrollToReveal, visibleEntries } from '../../lib/ui/virtual';
  import { loadMore, mail, markAllRead, pullMail, rowsOf, setCategory, setOnly, setView, syncing } from '../../lib/stores/mail.svelte';
  import { settings, updateSettings } from '../../lib/stores/settings.svelte';
  import { tagById } from '../../lib/stores/tags.svelte';
  import MessageRow from './MessageRow.svelte';
  import SearchChips from './SearchChips.svelte';
  import { desk } from './layout.svelte';
  import { startTriage } from './triage-state.svelte';

  interface Props {
    /** Sütun genişliği (px); verilmezse kalan alanı doldurur (yığın kipi). */
    width?: number | null;
  }

  let { width = null }: Props = $props();

  /** Arama sorgusunu komut paletinde düzenlemeye açar. */
  function editSearch() {
    desk.paletteSeed = mail.search;
    mail.paletteOpen = true;
  }

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
  const isInbox = $derived(isInboxView(mail));
  const rowH = $derived(compact ? 58 : 82);
  const ctx = $derived({ view: mail.view, search: mail.search, now });

  // Sıra ve grup başlıkları mobil listeyle ortak (lib/mail/listing.ts).
  const rows = $derived(orderRows(rowsOf(mail.items, mail.search == null), ctx));

  const layout = $derived(
    buildLayout(rows, {
      rowH,
      groupH: GROUP_H,
      groupOf: (m) => groupOf(m, ctx),
      groupLabel,
      rowKey: (m) => `m-${m.id}`,
    }),
  );

  const visible = $derived(visibleEntries(layout.entries, scrollTop, viewport, OVERSCAN));

  function onScroll() {
    scrollTop = scroller.scrollTop;
    if (scrollTop + viewport > layout.total - 600) void loadMore();
  }

  // Klavyeyle seçim değişince satırı görünür tut.
  $effect(() => {
    const id = mail.selectedId;
    if (id == null || !scroller) return;
    const i = rows.findIndex((r) => r.ids.includes(id));
    const target = scrollToReveal(layout, i, scroller.scrollTop, viewport);
    if (target != null) scroller.scrollTop = target;
  });

  const tabs: { key: Category | null; label: MessageKey }[] = [
    { key: null, label: 'cat.all' },
    { key: 'people', label: 'cat.people' },
    { key: 'notifications', label: 'cat.notifications' },
    { key: 'newsletters', label: 'cat.newsletters' },
  ];

  const empty = $derived(emptyState(mail));
  const title = $derived(listTitle({ view: mail.view, search: mail.search, accountId: mail.accountId, mailboxes: mail.mailboxes, accounts: mail.accounts, tagById }));

  // Arama sonucunu kenar çubuğuna kaydet.
  let savePrompt = $state<{ x: number; y: number } | null>(null);

  async function saveSearch(name: string) {
    const q = mail.search;
    savePrompt = null;
    if (!q) return;
    const list = settings.value.savedSearches;
    if (list.length >= SAVED_SEARCHES_MAX) {
      showToast({ text: t('saved.full', { n: SAVED_SEARCHES_MAX }) });
      return;
    }
    try {
      await updateSettings({ savedSearches: [...list, { id: '', name, query: q }] });
      showToast({ text: t('saved.saved', { name }) });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

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

<section class="list" class:fill={width == null} style:width={width == null ? undefined : `${width}px`} aria-label={t('list.label')}>
  <div class="head">
    <div class="title-row">
      {#if mail.search != null}
        <!-- Arama başlığına basınca sorgu komut paletinde düzenlenir. -->
        <h1 class="trunc"><button class="title-btn trunc" title="{t('list.editSearch')} · /" onclick={editSearch}>{title}</button></h1>
      {:else}
        <h1 class="trunc">{title}</h1>
        <span class="meta num">{unread ? t('list.unread', { n: unread }) : t('list.allRead')}</span>
      {/if}
      <div class="tools">
        <button
          class="btn-icon pull"
          class:busy={syncing()}
          aria-label={t('list.pull')}
          aria-busy={syncing()}
          title={t('list.pullHint')}
          onclick={() => pullMail()}
        >
          <Icon name="refresh" />
        </button>
        {#if mail.search != null}
          <button
            class="btn-icon"
            aria-label={t('saved.save')}
            title={t('saved.save')}
            aria-haspopup="dialog"
            onclick={(e) => {
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              savePrompt = { x: r.right - 300, y: r.bottom + 4 };
            }}
          >
            <Icon name="bookmark" />
          </button>
          <button class="btn-icon" aria-label={t('list.clearSearch')} title={t('list.clearSearch')} onclick={() => setView(mail.view, mail.accountId)}>
            <Icon name="x" />
          </button>
        {:else}
          <button
            class="btn-icon"
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
        <button
          class="btn-icon"
          aria-label={t('triage.start')}
          title="{t('triage.start')} · {t('triage.startHint')} · Shift O"
          disabled={!rows.length}
          onclick={() => startTriage()}
        >
          <Icon name="focus" />
        </button>
      </div>
    </div>
    {#if mail.search != null}<SearchChips />{/if}
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
        <span class="empty-icon"><Icon name={empty.icon} size={26} /></span>
        <div class="empty-title">{t(empty.title)}</div>
        <div class="empty-body">{t(empty.body)}</div>
      </div>
    {/if}
  </div>
</section>

{#if filterMenu}<Menu at={filterMenu} title={t('list.filterBy')} items={filterItems} onclose={() => (filterMenu = null)} />{/if}
{#if savePrompt}
  <NamePrompt
    at={savePrompt}
    title={t('saved.save')}
    label={t('saved.name')}
    value={mail.search ?? ''}
    maxlength={SAVED_SEARCH_NAME_CHARS}
    onsave={saveSearch}
    onclose={() => (savePrompt = null)}
  />
{/if}

<style>
  /* Kenar çizgileri ana penceredeki sütun ayırıcılarından gelir. */
  .list {
    flex: none;
    width: 404px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
  }

  .list.fill {
    flex: 1;
    width: auto;
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

  .tools {
    flex: none;
    display: flex;
    margin: 0 -8px 0 auto;
    align-self: center;
  }

  .title-btn {
    max-width: 100%;
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-align: left;
  }

  .title-btn:hover {
    text-decoration: underline;
    text-decoration-color: var(--line-strong);
    text-underline-offset: 4px;
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

  .tools .btn-icon:disabled {
    opacity: var(--dim-disabled);
    background: transparent;
  }

  /* Eşitleme sürerken simge döner; azaltılmış harekette yalnız soluklaşır. */
  .pull.busy :global(svg) {
    animation: pull-spin calc(var(--t-panel) * 3) linear infinite;
  }

  @keyframes pull-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pull.busy :global(svg) {
      animation: none;
      opacity: 0.55;
    }
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

</style>
