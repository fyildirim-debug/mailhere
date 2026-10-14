<script lang="ts">
  // Tam ekran arama: yazdıkça yerel dizinde arar (searchAll ve arama işleçleri
  // masaüstüyle aynı). Enter tüm sonuçları listede açar; sonuca dokunmak postayı açar.
  import { call, type MessageSummary } from '../../lib/api';
  import { t, when } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { displayName } from '../../lib/util/format';
  import { searchAll } from '../../lib/stores/mail.svelte';
  import AppBar from './AppBar.svelte';

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

  const needle = $derived(q.trim());

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

  function addOp(op: string) {
    q = `${needle ? `${needle} ` : ''}${op}`;
    input?.focus();
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!needle) return;
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
  <AppBar leading="back" onleading={onback}>
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
    {#snippet actions()}
      {#if q}
        <button type="button" class="btn-icon" aria-label={t('act.clear')} title={t('act.clear')} onclick={() => ((q = ''), input?.focus())}>
          <Icon name="x" size={20} />
        </button>
      {/if}
    {/snippet}
  </AppBar>

  <div class="body">
    {#if !needle}
      <div class="sec-title">{t('palette.ops')}</div>
      <div class="ops">
        {#each ops as [op, hint] (op)}
          <button type="button" class="op" onclick={() => addOp(op)}>
            <span class="op-code">{op}</span>
            <span class="meta trunc">{hint}</span>
          </button>
        {/each}
      </div>
      <p class="hint meta">{t('mob.searchHint')}</p>
    {:else}
      <button type="submit" class="all">
        <span class="all-icon"><Icon name="search" size={18} /></span>
        <span class="trunc grow">{t('palette.searchAll', { q: needle })}</span>
        <Icon name="chevronRight" size={16} class="quiet" />
      </button>
      {#if found.length}
        <div class="sec-title">{t('palette.mails')}</div>
        {#each found as m (m.id)}
          <button type="button" class="hit" onclick={() => onopen(m)}>
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
    {/if}
  </div>
</form>

<style>
  .search {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  input {
    flex: 1;
    min-width: 0;
    height: var(--m-touch);
    padding: 0 4px;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--ink);
    font-size: var(--fs-section);
    letter-spacing: -0.01em;
  }

  input::-webkit-search-cancel-button {
    display: none;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 6px 8px calc(16px + var(--sab));
    display: flex;
    flex-direction: column;
  }

  .sec-title {
    padding: 14px 12px 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  .ops {
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

  .op-code {
    flex: none;
    min-width: 128px;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--accent-text);
  }

  .hint {
    padding: 12px;
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
    line-height: 20px;
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
</style>
