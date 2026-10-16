<script module lang="ts">
  import type { IconName } from './icons';

  export interface MenuItem {
    /** Listeleme anahtarı; aynı adlı öğeler (eş adlı klasörler) için. Yoksa `label`. */
    key?: string;
    label: string;
    icon?: IconName;
    /** Sağda soluk ek bilgi (sayı, adres, tarih). */
    hint?: string;
    /** Sağda kısayol çipi (palet ve odak kipindeki gibi). */
    kbd?: string;
    danger?: boolean;
    disabled?: boolean;
    /** İşaretli seçenek (süzgeç gibi). */
    checked?: boolean;
    onclick: () => void;
  }
</script>

<script lang="ts">
  // Açılır menü: sağ tık menüsü ve düğme menüleri. Ok tuşlarıyla gezilir,
  // Esc ya da dışarı tıklama kapatır; seçimden sonra kendiliğinden kapanır.
  // Mobil kabukta aynı öğeler alt sayfada (Sheet) listelenir; konum yok sayılır.
  import { isMobileShell } from '../shell';
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';

  interface Props {
    items: MenuItem[];
    title?: string;
    /** Ekran koordinatı (sağ tık); verilmezse `style` ile kapsayıcıya göre konumlanır. */
    at?: { x: number; y: number } | null;
    style?: string;
    label?: string;
    onclose: () => void;
  }

  let { items, title, at = null, style = '', label, onclose }: Props = $props();

  let menu = $state<HTMLDivElement>();
  let pos = $state<{ left: number; top: number } | null>(null);

  $effect(() => {
    if (!menu || isMobileShell) return;
    if (at) {
      // Pencereden taşmasın. Hesaplanmış boyut giriş animasyonunun ölçeğinden
      // etkilenmez (getBoundingClientRect menüyü %2 dar ölçerdi); kesirli
      // genişlik yukarı yuvarlanır.
      const cs = getComputedStyle(menu);
      const w = Math.ceil(parseFloat(cs.width));
      const h = Math.ceil(parseFloat(cs.height));
      pos = {
        left: Math.max(8, Math.min(at.x, window.innerWidth - w - 8)),
        top: Math.max(8, Math.min(at.y, window.innerHeight - h - 8)),
      };
    }
    menu.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    const onDoc = (e: MouseEvent) => {
      if (menu && !menu.contains(e.target as Node)) onclose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onclose();
      }
    };
    const timer = setTimeout(() => document.addEventListener('mousedown', onDoc));
    document.addEventListener('keydown', onKey, true);
    window.addEventListener('blur', onclose);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey, true);
      window.removeEventListener('blur', onclose);
    };
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Home' && e.key !== 'End') return;
    e.preventDefault();
    const list = [...(menu?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
    if (!list.length) return;
    const i = list.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      e.key === 'Home' ? 0 : e.key === 'End' ? list.length - 1 : (i + (e.key === 'ArrowDown' ? 1 : -1) + list.length) % list.length;
    list[next].focus();
  }

  function pick(item: MenuItem) {
    onclose();
    item.onclick();
  }
</script>

{#snippet rows(iconSize: number)}
  {#each items as item (item.key ?? item.label)}
    <button
      class="menu-item"
      class:danger={item.danger}
      role={item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
      aria-checked={item.checked === undefined ? undefined : item.checked}
      disabled={item.disabled}
      onclick={() => pick(item)}
    >
      {#if item.icon}<Icon name={item.icon} size={iconSize} />{/if}
      <span class="grow trunc">{item.label}</span>
      {#if item.checked}<Icon name="check" size={iconSize} class="tick" />{:else if item.kbd}<kbd class="kbd">{item.kbd}</kbd>{:else if item.hint}<span class="meta num">{item.hint}</span>{/if}
    </button>
  {/each}
{/snippet}

{#if isMobileShell}
  <Sheet {title} label={label ?? title} {onclose}>
    <div class="sheet-menu" role="menu" tabindex="-1" aria-label={label ?? title} bind:this={menu} onkeydown={onKeydown}>
      {@render rows(20)}
    </div>
  </Sheet>
{:else}
  <div
    class="popover menu"
    class:fixed={!!at}
    role="menu"
    tabindex="-1"
    aria-label={label ?? title}
    bind:this={menu}
    style={at ? `left:${pos?.left ?? at.x}px;top:${pos?.top ?? at.y}px` : style}
    onkeydown={onKeydown}
  >
    {#if title}<div class="menu-title">{title}</div>{/if}
    {@render rows(16)}
  </div>
{/if}

<style>
  /* Menü etiketine göre genişler (kırpılmaz); konum zaten görünür alana
     sıkıştırıldığı için pencereden taşmaz. */
  .menu {
    width: max-content;
    min-width: 220px;
    max-width: min(440px, calc(100vw - 16px));
  }

  .fixed {
    position: fixed;
    z-index: 60;
  }

  .grow {
    flex: 1;
  }

  .danger {
    color: var(--danger);
  }

  .menu-item:disabled {
    color: var(--ink-3);
    background: transparent;
  }

  .menu :global(.tick),
  .sheet-menu :global(.tick) {
    color: var(--accent-text);
  }

  /* Alt sayfada satırlar 48 px, ikonlar soluk ve biraz daha geniş aralıklı. */
  .sheet-menu .menu-item {
    padding: 0 12px;
    gap: 14px;
  }

  .sheet-menu .menu-item :global(.icon:not(.tick)) {
    color: var(--ink-3);
  }

  .sheet-menu .menu-item.danger :global(.icon) {
    color: var(--danger);
  }
</style>
