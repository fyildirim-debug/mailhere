<script lang="ts">
  // Kısayol sayfası (?): tüm kısayollar üç grupta. Esc ya da dışarı tıklama kapatır.
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { KEY_GROUPS } from '../../lib/util/keys';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let sheet = $state<HTMLDivElement>();

  $effect(() => {
    // Odak pencerenin kendisine: Tab kapat düğmesine geçer, halka ilk açılışta görünmez.
    sheet?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || e.key === '?') {
        e.preventDefault();
        e.stopPropagation();
        onclose();
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  });
</script>

<div class="scrim" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="keys-title" tabindex="-1" bind:this={sheet}>
    <div class="head">
      <span class="mark"><Icon name="keyboard" /></span>
      <div class="titles">
        <h2 id="keys-title">{t('keys.title')}</h2>
        <p class="meta">{t('keys.dialogHint')}</p>
      </div>
      <button class="btn-icon" aria-label={t('act.close')} title="{t('act.close')} · Esc" onclick={onclose}>
        <Icon name="x" />
      </button>
    </div>
    <div class="groups">
      {#each KEY_GROUPS as g (g.title)}
        <section class="group" aria-labelledby="kg-{g.title}">
          <h3 id="kg-{g.title}">{t(g.title)}</h3>
          <dl>
            {#each g.rows as r (r.label)}
              <dt>
                {#each r.keys.split(' / ') as k, i (k)}
                  {#if i}<span class="or">/</span>{/if}
                  {#each k.split(' ') as part, j (j)}<span class="kbd">{part}</span>{/each}
                {/each}
              </dt>
              <dd>{t(r.label)}</dd>
            {/each}
          </dl>
        </section>
      {/each}
    </div>
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
    padding: 56px 16px 16px;
    background: color-mix(in srgb, var(--ground) 55%, transparent);
    animation: fade var(--t-fast) var(--ease);
  }

  .sheet {
    outline: none;
    width: 920px;
    max-width: 100%;
    max-height: calc(100vh - 88px);
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
    align-items: flex-start;
    gap: 14px;
    padding: 20px 16px 16px 24px;
    border-bottom: 1px solid var(--line);
  }

  .mark {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    flex: none;
    border-radius: var(--r-row);
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .titles {
    flex: 1;
    min-width: 0;
  }

  h2 {
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
  }

  .groups {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 8px 32px;
    padding: 12px 24px 24px;
  }

  h3 {
    margin: 12px 0 8px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  dl {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 8px 14px;
    margin: 0;
  }

  dt {
    display: flex;
    align-items: center;
    gap: 4px;
    justify-content: flex-end;
  }

  .or {
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  dd {
    margin: 0;
    color: var(--ink-2);
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
