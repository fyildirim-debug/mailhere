<script lang="ts">
  // "Yanıt gelmezse hatırlat" menüsü: yarın / 3 gün / 1 hafta / özel tarih;
  // kurulu hatırlatma varsa kaldırma. Okuma panelinde açılır menü, yazma
  // penceresinde gönder menüsünün içine gömülü (`embedded`).
  import { untrack } from 'svelte';
  import { t, when, type MessageKey } from '../i18n/index.svelte';
  import { isMobileShell } from '../shell';
  import Icon from '../ui/Icon.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { followUpOptions, parseLocalDateTime, toLocalDateTime, type FollowUpKey } from '../util/followup';

  interface Props {
    /** Kurulu hatırlatmanın anı; yoksa null. */
    current?: number | null;
    onpick: (at: number) => void;
    /** Kurulu hatırlatmayı kaldır; verilmezse seçenek görünmez. */
    onclear?: () => void;
    onclose: () => void;
    /** Menünün konumu (kapsayıcıya göre). */
    style?: string;
    /** Ekran koordinatı (okuma paneli); verilirse sabit konumlanır ve pencereye sığdırılır. */
    at?: { x: number; y: number } | null;
    /** Başka bir menünün içinde, kendi yüzeyi ve dışarı tıklama dinleyicisi olmadan. */
    embedded?: boolean;
  }

  let { current = null, onpick, onclear, onclose, style = 'top: 44px; right: 0', at = null, embedded = false }: Props = $props();

  const now = Date.now();
  const opts = followUpOptions(now);
  const labels: Record<FollowUpKey, MessageKey> = {
    tomorrow: 'follow.tomorrow',
    days3: 'follow.days3',
    week: 'follow.week',
  };

  let custom = $state(false);
  // Özel tarihin başlangıcı: kurulu hatırlatma, yoksa ilk seçenek (yalnız ilk değer).
  let customValue = $state(toLocalDateTime(untrack(() => current) ?? opts[0].at));
  const customAt = $derived(parseLocalDateTime(customValue, now));
  let menu = $state<HTMLDivElement>();
  let pos = $state<{ left: number; top: number } | null>(null);

  $effect(() => {
    // Gömülü ve alt sayfadaki menünün kendi konumu ve dışarı tıklaması yoktur.
    if (embedded || isMobileShell) return;
    if (menu && at) {
      // Pencereden taşmasın.
      const r = menu.getBoundingClientRect();
      pos = {
        left: Math.max(8, Math.min(at.x, window.innerWidth - r.width - 8)),
        top: Math.max(8, Math.min(at.y, window.innerHeight - r.height - 8)),
      };
    }
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

  function pickCustom() {
    if (customAt != null) onpick(customAt);
  }
</script>

{#snippet rows(iconSize: number)}
  {#each opts as o (o.key)}
    <button class="menu-item" role="menuitemcheckbox" aria-checked={current === o.at} onclick={() => onpick(o.at)}>
      <Icon name="followUp" size={iconSize} />
      <span class="grow">{t(labels[o.key])}</span>
      {#if current === o.at}<Icon name="check" size={iconSize} class="tick" />{:else}<span class="meta num">{when(o.at, now)}</span>{/if}
    </button>
  {/each}
  {#if custom}
    <div class="custom">
      <input class="input small num" type="datetime-local" aria-label={t('follow.customDate')} bind:value={customValue} min={toLocalDateTime(now)} />
      <button class="btn-primary small" disabled={customAt == null} onclick={pickCustom}>{t('follow.set')}</button>
    </div>
  {:else}
    <button class="menu-item" role="menuitem" onclick={() => (custom = true)}>
      <Icon name="calendar" size={iconSize} />
      <span class="grow">{t('follow.custom')}</span>
    </button>
  {/if}
  {#if current != null && onclear}
    <button class="menu-item clear" role="menuitem" onclick={() => onclear?.()}>
      <Icon name="x" size={iconSize} />
      <span class="grow">{t('follow.clear')}</span>
    </button>
  {/if}
{/snippet}

{#if !embedded && isMobileShell}
  <Sheet title={t('follow.menu')} {onclose}>
    <div class="sheet-menu" role="menu" aria-label={t('follow.menu')}>{@render rows(20)}</div>
  </Sheet>
{:else}
  <div
    class={embedded ? 'embedded' : 'popover'}
    class:fixed={!embedded && !!at}
    role={embedded ? 'group' : 'menu'}
    aria-label={t('follow.menu')}
    bind:this={menu}
    style={embedded ? '' : at ? `left:${pos?.left ?? at.x}px;top:${pos?.top ?? at.y}px` : style}
  >
    <div class="menu-title">{t('follow.menu')}</div>
    {@render rows(16)}
  </div>
{/if}

<style>
  .popover {
    width: 300px;
  }

  .fixed {
    position: fixed;
    z-index: 60;
  }

  .embedded {
    padding-top: 4px;
    border-top: 1px solid var(--line);
    margin-top: 4px;
  }

  .menu-item :global(.icon) {
    color: var(--ink-3);
  }

  .menu-item :global(.tick) {
    color: var(--accent-text);
  }

  .sheet-menu .menu-item {
    padding: 0 12px;
    gap: 14px;
  }

  .sheet-menu .custom {
    padding: 8px 12px 10px;
  }

  .grow {
    flex: 1;
  }

  .custom {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px 8px;
  }

  .custom .input {
    flex: 1;
    min-width: 0;
  }

  .clear {
    color: var(--ink-2);
  }
</style>
