<script lang="ts">
  import { t, when, type MessageKey } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';
  import { snoozeOptions } from '../util/format';

  interface Props {
    onpick: (at: number) => void;
    onclose: () => void;
    /** Menünün konumu (kapsayıcıya göre). */
    style?: string;
  }

  let { onpick, onclose, style = 'top: 44px; left: 0' }: Props = $props();

  const now = Date.now();
  const opts = snoozeOptions(now);
  const labels: Record<string, MessageKey> = {
    evening: 'snooze.evening',
    tomorrow: 'snooze.tomorrow',
    weekend: 'snooze.weekend',
    nextWeek: 'snooze.nextWeek',
  };

  let menu = $state<HTMLDivElement>();

  $effect(() => {
    menu?.querySelector<HTMLButtonElement>('button')?.focus();
    const onDoc = (e: MouseEvent) => {
      if (menu && !menu.contains(e.target as Node)) onclose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onclose();
      }
    };
    setTimeout(() => document.addEventListener('mousedown', onDoc));
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey, true);
    };
  });
</script>

<div class="popover" role="menu" bind:this={menu} {style}>
  <div class="menu-title">{t('snooze.title')}</div>
  {#each opts as o (o.key)}
    <button class="menu-item" role="menuitem" onclick={() => onpick(o.at)}>
      <Icon name="clock" size={16} />
      <span class="grow">{t(labels[o.key])}</span>
      <span class="meta num">{when(o.at, now)}</span>
    </button>
  {/each}
</div>

<style>
  .popover {
    width: 300px;
  }

  .menu-item :global(.icon) {
    color: var(--ink-3);
  }

  .grow {
    flex: 1;
  }
</style>
