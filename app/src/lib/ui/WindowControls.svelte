<script lang="ts">
  import { appWindow } from '../api';
  import { t } from '../i18n/index.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    /** Büyüt düğmesi (küçük pencerelerde yok). */
    maximize?: boolean;
    minimize?: boolean;
    height?: number;
    onclose?: () => void;
  }

  let { maximize = true, minimize = true, height = 46, onclose }: Props = $props();
</script>

<div class="controls" style:--h="{height}px">
  {#if minimize}
    <button class="ctl" aria-label={t('win.minimize')} onclick={() => appWindow.minimize()}>
      <Icon name="minimize" size={16} />
    </button>
  {/if}
  {#if maximize}
    <button class="ctl" aria-label={t('win.maximize')} onclick={() => appWindow.toggleMaximize()}>
      <Icon name="maximize" size={14} />
    </button>
  {/if}
  <button class="ctl close" aria-label={t('win.close')} onclick={() => (onclose ? onclose() : appWindow.close())}>
    <Icon name="x" size={16} />
  </button>
</div>

<style>
  .controls {
    display: flex;
    align-self: stretch;
  }

  .ctl {
    width: 46px;
    height: var(--h);
    display: grid;
    place-items: center;
    border: 0;
    background: transparent;
    color: var(--ink-2);
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .ctl:hover {
    background: var(--hover);
    color: var(--ink);
  }

  /* Platform kapat kırmızısı (tasarım dilinde tanımlı tek istisna). */
  .close:hover {
    background: #c42b1c;
    color: #ffffff;
  }
</style>
