<script module lang="ts">
  import type { IconName } from '../../lib/ui/icons';

  export interface PickItem {
    key: string;
    label: string;
    /** Klasör ağacında derinlik (süzülmezken girinti). */
    depth?: number;
    icon?: IconName;
    /** Etiket rengi (kullanıcı verisi). */
    color?: string;
    /** Etiket postada var mı. */
    checked?: boolean;
  }
</script>

<script lang="ts">
  // Yazarak süzülen seçici (V: klasöre taşı, L: etiketle): ↑/↓ gezinir,
  // Enter seçer, Esc kapatır. Mevcut taşıma / etiket komutlarını çağıran
  // ana pencere `onpick` ile karar verir.
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { matchRange, score } from './fuzzy';

  interface Props {
    title: string;
    placeholder: string;
    items: PickItem[];
    /** Hiç öğe yokken (süzgeçten bağımsız) gösterilen durum. */
    empty?: { title: string; body: string; action?: { label: string; run: () => void } };
    onpick: (item: PickItem) => void;
    onclose: () => void;
  }

  let { title, placeholder, items, empty, onpick, onclose }: Props = $props();

  let q = $state('');
  let active = $state(0);
  let input = $state<HTMLInputElement>();
  let emptyBtn = $state<HTMLButtonElement>();
  let listEl = $state<HTMLDivElement>();

  const shown = $derived.by(() => {
    const n = q.trim();
    if (!n) return items;
    return items
      .map((it, i) => ({ it, s: score([it.label], n), i }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s || a.i - b.i)
      .map((x) => x.it);
  });

  $effect(() => {
    (input ?? emptyBtn)?.focus();
  });

  $effect(() => {
    if (active >= shown.length) active = Math.max(0, shown.length - 1);
  });

  $effect(() => {
    listEl?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  function pick(it: PickItem | undefined) {
    if (!it) return;
    onclose();
    onpick(it);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(active + 1, shown.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      pick(shown[active]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }

  function parts(label: string) {
    const r = matchRange(label, q);
    return r ? { pre: label.slice(0, r[0]), mid: label.slice(r[0], r[1]), post: label.slice(r[1]) } : { pre: label, mid: '', post: '' };
  }
</script>

<div class="scrim" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="picker" role="dialog" aria-modal="true" aria-label={title}>
    <div class="head">
      <span class="title">{title}</span>
      <span class="kbd">Esc</span>
    </div>
    {#if !items.length && empty}
      <div class="empty">
        <p class="empty-title">{empty.title}</p>
        <p class="empty-body">{empty.body}</p>
        {#if empty.action}
          {@const act = empty.action}
          <button
            class="btn-soft small"
            bind:this={emptyBtn}
            onclick={() => {
              onclose();
              act.run();
            }}
            onkeydown={(e) => e.key === 'Escape' && (e.preventDefault(), e.stopPropagation(), onclose())}>{act.label}</button
          >
        {/if}
      </div>
    {:else}
      <div class="search">
        <Icon name="search" size={16} />
        <input
          bind:this={input}
          bind:value={q}
          oninput={() => (active = 0)}
          onkeydown={onKey}
          {placeholder}
          aria-label={placeholder}
          role="combobox"
          aria-expanded="true"
          aria-controls="picker-list"
          aria-activedescendant={shown.length ? `picker-${active}` : undefined}
        />
      </div>
      <div class="list" id="picker-list" role="listbox" aria-label={title} bind:this={listEl}>
        {#each shown as it, i (it.key)}
          {@const p = parts(it.label)}
          <button
            class="opt"
            class:active={i === active}
            id="picker-{i}"
            role="option"
            aria-selected={i === active}
            aria-checked={it.checked}
            data-index={i}
            style:--depth={q.trim() ? 0 : (it.depth ?? 0)}
            onmouseenter={() => (active = i)}
            onclick={() => pick(it)}
          >
            {#if it.color}
              <span class="tdot" style:background={it.color}></span>
            {:else if it.icon}
              <Icon name={it.icon} size={16} />
            {/if}
            <span class="trunc grow">{p.pre}<mark>{p.mid}</mark>{p.post}</span>
            {#if it.checked}<Icon name="check" size={16} />{/if}
          </button>
        {/each}
        {#if !shown.length}
          <p class="nothing">{t('picker.nothing')}</p>
        {/if}
      </div>
      <div class="foot">
        <span class="hint"><span class="kbd"><Icon name="arrowUp" size={12} /></span><span class="kbd"><Icon name="arrowDown" size={12} /></span>{t('palette.navigate')}</span>
        <span class="hint"><span class="kbd">Enter</span>{t('palette.select')}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 96px 16px 16px;
    background: color-mix(in srgb, var(--ground) 55%, transparent);
    animation: fade var(--t-fast) ease-out;
  }

  .picker {
    width: 440px;
    max-width: 100%;
    max-height: min(460px, calc(100vh - 128px));
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: var(--r-palette);
    background: var(--paper);
    border: 1px solid var(--line);
    box-shadow: var(--shadow-lg);
    animation: drop var(--t-state) var(--ease);
  }

  .head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px 4px;
  }

  .title {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  .search {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 6px 12px 4px;
    padding: 0 12px;
    height: 40px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-row);
    color: var(--ink-3);
  }

  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 36px;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--ink);
  }

  input::placeholder {
    color: var(--ink-3);
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 6px 8px 8px;
  }

  .opt {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 38px;
    padding: 0 10px 0 calc(10px + var(--depth) * 16px);
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink);
    text-align: left;
  }

  .opt :global(svg) {
    color: var(--ink-2);
  }

  .opt.active {
    background: var(--sel);
  }

  .tdot {
    flex: none;
    width: 10px;
    height: 10px;
    margin: 0 3px;
    border-radius: 50%;
  }

  .grow {
    flex: 1;
  }

  /* Eşleşen parça renkle değil ağırlık ve alt çizgiyle. */
  mark {
    background: transparent;
    color: inherit;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .nothing {
    padding: 24px 12px;
    color: var(--ink-3);
    text-align: center;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 8px 16px 18px;
  }

  .empty-title {
    font-weight: 600;
  }

  .empty-body {
    color: var(--ink-3);
    margin-bottom: 6px;
  }

  .foot {
    flex: none;
    display: flex;
    gap: 16px;
    height: 38px;
    align-items: center;
    padding: 0 16px;
    border-top: 1px solid var(--line);
    background: var(--surface);
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-6px) scale(0.985);
    }
  }
</style>
