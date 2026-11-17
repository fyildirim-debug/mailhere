<script lang="ts">
  // Erteleme menüsü: akşam / yarın / hafta sonu / haftaya ve altında kendi
  // tarih-saatini seçme. Masaüstünde açılır menü (ok tuşlarıyla gezilir), mobil
  // kabukta alt sayfa (Sheet). Bileşen menü her açıldığında yeniden kurulur:
  // hazır seçenekler o anın saatine göre hesaplanır.
  import { t, when, type MessageKey } from '../i18n/index.svelte';
  import { isMobileShell } from '../shell';
  import Icon from '../ui/Icon.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { snoozeOptions } from '../util/format';
  import { menuKeys } from './menukeys';
  import PickTime from './PickTime.svelte';

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
    if (isMobileShell) return;
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
    const timer = setTimeout(() => document.addEventListener('mousedown', onDoc));
    document.addEventListener('keydown', onKey, true);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey, true);
    };
  });
</script>

{#snippet rows(iconSize: number)}
  {#each opts as o (o.key)}
    <button class="menu-item" role="menuitem" onclick={() => onpick(o.at)}>
      <Icon name="clock" size={iconSize} />
      <span class="grow">{t(labels[o.key])}</span>
      <span class="meta num">{when(o.at, now)}</span>
    </button>
  {/each}
  <PickTime row={t('compose.pickTime')} label={t('time.pickSnooze')} action={t('act.snooze')} initial={opts[0].at} {iconSize} {onpick} />
{/snippet}

{#if isMobileShell}
  <Sheet title={t('snooze.title')} {onclose}>
    <div class="sheet-menu" role="menu" tabindex="-1" aria-label={t('snooze.title')} bind:this={menu} onkeydown={(e) => menuKeys(e, menu)}>
      {@render rows(20)}
    </div>
  </Sheet>
{:else}
  <div class="popover" role="menu" tabindex="-1" aria-label={t('snooze.title')} bind:this={menu} {style} onkeydown={(e) => menuKeys(e, menu)}>
    <div class="menu-title">{t('snooze.title')}</div>
    {@render rows(16)}
  </div>
{/if}

<style>
  .popover {
    width: 300px;
  }

  .menu-item :global(.icon) {
    color: var(--ink-3);
  }

  .sheet-menu .menu-item {
    padding: 0 12px;
    gap: 14px;
  }

  .grow {
    flex: 1;
  }
</style>
