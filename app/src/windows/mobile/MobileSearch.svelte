<script lang="ts">
  // Tam ekran arama: alan altta, başparmak bölgesinde ve klavyenin hemen
  // üstünde (görünüm alanı klavyeyle küçülür: `interactive-widget=resizes-content`).
  // Yazdıkça yerel dizinde arar (searchAll ve arama işleçleri masaüstüyle
  // aynı); Enter tüm sonuçları listede açar; sonuca dokunmak postayı açar.
  // Boşken son aramalar (yalnız bu cihazda, en çok 6) ve işleç ipuçları;
  // içerik alana yakın, alttan dizilir.
  import { onMount } from 'svelte';
  import { call, type MessageSummary } from '../../lib/api';
  import { t, when } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { displayName } from '../../lib/util/format';
  import { searchAll } from '../../lib/stores/mail.svelte';
  import { loadRecent, pushRecent, saveRecent } from './recent';

  interface Props {
    /** Sorgu üst bileşende tutulur: sonuç açılıp geri dönülünce yazılan kalır. */
    q: string;
    onback: () => void;
    /** Enter: liste sorguyla açılır. */
    onsubmit: () => void;
    onopen: (m: MessageSummary) => void;
  }

  let { q = $bindable(''), onback, onsubmit, onopen }: Props = $props();

  let found = $state<MessageSummary[]>([]);
  let searching = $state(false);
  let input = $state<HTMLInputElement>();
  let recent = $state<string[]>([]);

  const needle = $derived(q.trim());

  onMount(() => {
    recent = loadRecent();
  });

  $effect(() => {
    input?.focus();
  });

  // Yazarken yerel dizinde ara (kısa gecikmeyle).
  $effect(() => {
    const query = needle;
    if (!query) {
      found = [];
      searching = false;
      return;
    }
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const res = await call('search', { q: query, limit: 30 });
        if (q.trim() === query) found = res;
      } catch {
        found = [];
      } finally {
        if (q.trim() === query) searching = false;
      }
    }, 80);
    return () => clearTimeout(timer);
  });

  const ops: [string, string][] = [
    ['from:', t('palette.op.from')],
    ['has:ek', t('palette.op.attachment')],
    ['is:okunmamış', t('palette.op.unread')],
    ['in:arşiv', t('palette.op.in')],
    ['tag:', t('tag.op')],
    ['after:2026-01-01', t('palette.op.after')],
  ];

  function remember() {
    recent = pushRecent(recent, needle);
    saveRecent(recent);
  }

  function forget() {
    recent = [];
    saveRecent(recent);
    input?.focus();
  }

  function addOp(op: string) {
    q = `${needle ? `${needle} ` : ''}${op}`;
    input?.focus();
  }

  function useRecent(query: string) {
    q = query;
    input?.focus();
  }

  function open(m: MessageSummary) {
    remember();
    onopen(m);
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!needle) return;
    remember();
    searchAll(needle);
    onsubmit();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onback();
    }
  }
</script>

<form class="search" role="search" onsubmit={submit}>
  <div class="body">
    <div class="stack">
      {#if !needle}
        {#if recent.length}
          <div class="sec-title">
            <span class="grow">{t('mob.recentSearches')}</span>
            <button type="button" class="link clear-recent" onclick={forget}>{t('mob.recentClear')}</button>
          </div>
          <div class="list" role="group" aria-label={t('mob.recentSearches')}>
            {#each recent as r (r)}
              <button type="button" class="op recent" onclick={() => useRecent(r)}>
                <span class="op-icon"><Icon name="clock" size={18} /></span>
                <span class="trunc grow">{r}</span>
              </button>
            {/each}
          </div>
        {/if}
        <div class="sec-title">{t('palette.ops')}</div>
        <div class="list">
          {#each ops as [op, hint] (op)}
            <button type="button" class="op" onclick={() => addOp(op)}>
              <span class="op-code">{op}</span>
              <span class="meta trunc">{hint}</span>
            </button>
          {/each}
        </div>
        <p class="hint meta">{t('mob.searchHint')}</p>
      {:else}
        {#if found.length}
          <div class="sec-title">{t('palette.mails')}</div>
          {#each found as m (m.id)}
            <button type="button" class="hit" onclick={() => open(m)}>
              <Avatar address={m.from} size={36} />
              <span class="hit-text">
                <span class="hit-line"><span class="trunc hit-who" class:unread={m.unread}>{displayName(m.from)}</span><span class="meta num">{when(m.date)}</span></span>
                <span class="trunc hit-subject">{m.subject || '—'}</span>
              </span>
            </button>
          {/each}
        {:else if !searching}
          <div class="m-empty">
            <span class="m-empty-icon"><Icon name="search" size={28} /></span>
            <div class="m-empty-title">{t('mob.searchEmpty')}</div>
            <div class="m-empty-body">{t('mob.searchEmptyBody')}</div>
          </div>
        {/if}
        <button type="submit" class="all">
          <span class="all-icon"><Icon name="search" size={18} /></span>
          <span class="trunc grow">{t('palette.searchAll', { q: needle })}</span>
          <Icon name="chevronRight" size={16} class="quiet" />
        </button>
      {/if}
    </div>
  </div>

  <!-- Alan altta: klavye açıkken hemen üstünde, başparmağın altında. -->
  <div class="bar">
    <!-- type="button": formun "varsayılan düğmesi" sayılmasın (Enter aramayı açar). -->
    <button type="button" class="btn-icon" aria-label={t('mob.back')} title={t('mob.back')} onclick={onback}>
      <Icon name="arrowLeft" size={22} />
    </button>
    <label class="field">
      <Icon name="search" size={18} />
      <input
        bind:this={input}
        bind:value={q}
        type="search"
        enterkeyhint="search"
        autocomplete="off"
        placeholder={t('mob.searchPlaceholder')}
        aria-label={t('mob.searchPlaceholder')}
        onkeydown={onKey}
      />
      {#if q}
        <button type="button" class="btn-icon small" aria-label={t('act.clear')} title={t('act.clear')} onclick={() => ((q = ''), input?.focus())}>
          <Icon name="x" size={18} />
        </button>
      {/if}
    </label>
  </div>
</form>

<style>
  .search {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
  }

  /* İçerik alttan dizilir (alana yakın); taşınca normal kayar. */
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    display: flex;
    flex-direction: column;
    padding: calc(8px + var(--sat)) calc(8px + var(--sar)) 8px calc(8px + var(--sal));
  }

  .stack {
    margin-top: auto;
    display: flex;
    flex-direction: column;
  }

  .sec-title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 36px;
    padding: 10px 12px 2px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  .clear-recent {
    font-size: var(--fs-meta);
    font-weight: 500;
    min-height: var(--m-touch);
  }

  .list {
    display: flex;
    flex-direction: column;
  }

  .op,
  .all,
  .hit {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: var(--m-touch-lg);
    padding: 6px 12px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--ink);
    text-align: left;
    transition: background var(--t-fast);
  }

  .op:active,
  .all:active,
  .hit:active {
    background: var(--hover);
  }

  .op-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    color: var(--ink-3);
  }

  .op-code {
    flex: none;
    min-width: 128px;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--accent-text);
  }

  .hint {
    padding: 8px 12px 4px;
    max-width: 40ch;
  }

  .all {
    margin-top: 6px;
    font-weight: 500;
  }

  .all-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .grow {
    flex: 1;
  }

  .all :global(.quiet) {
    color: var(--ink-3);
  }

  .hit-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: var(--m-line);
  }

  .hit-line {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .hit-who {
    flex: 1;
    font-weight: 500;
  }

  .hit-who.unread {
    font-weight: 600;
  }

  .hit-subject {
    color: var(--ink-2);
  }

  /* ------------------------------------------------------------ alt alan */

  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: calc(64px + var(--sab));
    padding: 8px calc(12px + var(--sar)) calc(8px + var(--sab)) calc(4px + var(--sal));
    background: var(--raised);
    border-top: 1px solid var(--line);
  }

  .field {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: var(--m-touch-lg);
    padding: 0 4px 0 14px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-palette);
    background: var(--paper);
    color: var(--ink-3);
    transition:
      border-color var(--t-state),
      box-shadow var(--t-state);
  }

  .field:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  input {
    flex: 1;
    min-width: 0;
    min-height: var(--m-touch);
    padding: 0 4px 0 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--ink);
    font-size: var(--fs-body);
  }

  input::-webkit-search-cancel-button {
    display: none;
  }
</style>
