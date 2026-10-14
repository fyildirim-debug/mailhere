<script lang="ts">
  // Uygulama çubuğu (56 px + güvenli alan): soldaki düğme, başlık ve sağ eylemler.
  // Başlık yerine özel içerik (arama girdisi) için `children` verilir.
  import type { Snippet } from 'svelte';
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';

  interface Props {
    title?: string;
    subtitle?: string;
    /** Soldaki düğme: çekmece, geri ya da kapat (seçim kipi). */
    leading: 'menu' | 'back' | 'close';
    leadingLabel?: string;
    onleading: () => void;
    /** Seçim kipi: vurgulu zemin. */
    select?: boolean;
    actions?: Snippet;
    children?: Snippet;
  }

  let { title = '', subtitle, leading, leadingLabel, onleading, select = false, actions, children }: Props = $props();

  const icon = $derived(leading === 'menu' ? 'menu' : leading === 'back' ? 'arrowLeft' : 'x');
  const label = $derived(leadingLabel ?? t(leading === 'menu' ? 'mob.menu' : leading === 'back' ? 'mob.back' : 'bulk.clear'));
</script>

<header class="appbar" class:select>
  <!-- type="button": arama formunun içinde Enter bu düğmeyi "varsayılan düğme" saymasın. -->
  <button type="button" class="btn-icon lead" aria-label={label} title={label} onclick={onleading}>
    <Icon name={icon} size={22} />
  </button>
  {#if children}
    <div class="center">{@render children()}</div>
  {:else}
    <div class="titles">
      <h1 class="trunc">{title}</h1>
      {#if subtitle}<span class="sub trunc num">{subtitle}</span>{/if}
    </div>
  {/if}
  {#if actions}
    <div class="acts">{@render actions()}</div>
  {/if}
</header>

<style>
  .appbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: calc(var(--m-appbar) + var(--sat));
    padding: var(--sat) calc(6px + var(--sar)) 0 calc(6px + var(--sal));
    background: var(--ground);
    border-bottom: 1px solid var(--line);
    transition:
      background var(--t-state),
      color var(--t-state);
  }

  /* Seçim kipi: vurgu zemin, üstünde vurgu metni (gri değil). */
  .appbar.select {
    background: var(--accent);
    color: var(--accent-ink);
    border-bottom-color: transparent;
  }

  .select :global(.btn-icon) {
    color: inherit;
  }

  .select :global(.btn-icon:hover) {
    background: color-mix(in srgb, var(--accent-ink) 14%, transparent);
    color: inherit;
  }

  .lead {
    flex: none;
  }

  .titles {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 0 4px;
  }

  h1 {
    font-size: var(--fs-section);
    font-weight: 600;
    line-height: 1.3;
    letter-spacing: -0.015em;
  }

  .sub {
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .select .sub {
    color: inherit;
    opacity: 0.85;
  }

  .center {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
  }

  .acts {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
  }
</style>
