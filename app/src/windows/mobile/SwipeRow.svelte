<script lang="ts">
  // Kaydırmalı satır: sola Arşivle (--accent), sağa Ertele (--amber). Eşik
  // genişliğin %40'ı; yalnız transform canlandırılır (200 ms), azaltılmış
  // harekette anlık. Uzun basış (450 ms, kıpırdamadan) seçim için ayrı olay.
  // Dikey kaydırma tarayıcıya bırakılır (touch-action: pan-y).
  import type { Snippet } from 'svelte';
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';

  interface Props {
    onarchive: () => void;
    onsnooze: () => void;
    onlongpress?: () => void;
    /** Seçim kipinde kaydırma kapalıdır. */
    disabled?: boolean;
    children: Snippet;
  }

  let { onarchive, onsnooze, onlongpress, disabled = false, children }: Props = $props();

  const THRESHOLD = 0.4;
  const SLOP = 10;
  const LONG_PRESS = 450;

  let el = $state<HTMLDivElement>();
  let width = $state(360);
  let dx = $state(0);
  let dragging = $state(false);
  let leaving = $state<'left' | 'right' | null>(null);
  let start: { x: number; y: number; id: number } | null = null;
  let pressTimer: ReturnType<typeof setTimeout> | null = null;
  let swallowClick = false;

  function clearPress() {
    if (pressTimer) clearTimeout(pressTimer);
    pressTimer = null;
  }

  function down(e: PointerEvent) {
    if (leaving || (e.pointerType === 'mouse' && e.button !== 0)) return;
    // Android uzun basıştan sonra click üretmez; asılı bayrak sonraki dokunuşu yutmasın.
    swallowClick = false;
    start = { x: e.clientX, y: e.clientY, id: e.pointerId };
    dragging = false;
    clearPress();
    if (onlongpress) {
      pressTimer = setTimeout(() => {
        pressTimer = null;
        start = null;
        swallowClick = true;
        onlongpress();
      }, LONG_PRESS);
    }
  }

  function move(e: PointerEvent) {
    if (!start || e.pointerId !== start.id) return;
    const mx = e.clientX - start.x;
    const my = e.clientY - start.y;
    if (!dragging) {
      if (Math.abs(mx) < SLOP && Math.abs(my) < SLOP) return;
      clearPress();
      if (disabled || Math.abs(mx) < Math.abs(my)) {
        // Dikey niyet ya da seçim kipi: tarayıcı kaydırır, biz çekiliriz.
        start = null;
        return;
      }
      dragging = true;
      el?.setPointerCapture(e.pointerId);
    }
    dx = Math.max(-width, Math.min(width, mx));
  }

  function up(e: PointerEvent) {
    clearPress();
    if (!start || e.pointerId !== start.id) {
      start = null;
      return;
    }
    start = null;
    if (!dragging) return;
    dragging = false;
    swallowClick = true;
    if (e.type !== 'pointercancel' && Math.abs(dx) >= width * THRESHOLD) commit(dx < 0 ? 'left' : 'right');
    else dx = 0;
  }

  /** Satır kenardan çıkar, sonra eylem çalışır (liste satırı kaldırır). */
  function commit(dir: 'left' | 'right') {
    leaving = dir;
    dx = dir === 'left' ? -width : width;
    const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
    setTimeout(
      () => {
        if (dir === 'left') onarchive();
        else onsnooze();
        // Satır listede kaldıysa (hata) yerine döner.
        leaving = null;
        dx = 0;
      },
      reduced ? 0 : 200,
    );
  }

  /** Sürükleme ya da uzun basış sonrası gelen tık satırı açmasın. */
  function clickCapture(e: MouseEvent) {
    if (!swallowClick) return;
    swallowClick = false;
    e.preventDefault();
    e.stopPropagation();
  }

  const side = $derived(dx < 0 ? 'left' : dx > 0 ? 'right' : null);
  const armed = $derived(Math.abs(dx) >= width * THRESHOLD);
</script>

<div
  class="swipe"
  role="presentation"
  class:dragging
  class:leaving={leaving != null}
  bind:this={el}
  bind:clientWidth={width}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onclickcapture={clickCapture}
  oncontextmenu={(e) => e.preventDefault()}
>
  {#if side === 'left'}
    <div class="under left" class:armed aria-hidden="true">
      <Icon name="archive" size={22} />
      <span>{t('act.archive')}</span>
    </div>
  {:else if side === 'right'}
    <div class="under right" class:armed aria-hidden="true">
      <Icon name="clock" size={22} />
      <span>{t('snooze.tomorrow')}</span>
    </div>
  {/if}
  <div class="front" style:transform={dx ? `translateX(${dx}px)` : undefined}>
    {@render children()}
  </div>
</div>

<style>
  .swipe {
    position: relative;
    height: 100%;
    overflow: hidden;
    touch-action: pan-y;
    user-select: none;
    -webkit-user-select: none;
  }

  .under {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 20px;
    color: var(--accent-ink);
    font-size: var(--fs-meta);
    font-weight: 600;
  }

  .under.left {
    justify-content: flex-end;
    background: var(--accent);
  }

  .under.right {
    justify-content: flex-start;
    background: var(--amber);
  }

  .under :global(.icon),
  .under span {
    transition: transform var(--t-fast) var(--ease);
  }

  /* Eşik geçilince işaret hafifçe büyür: bırakınca ne olacağı belli. */
  .under.armed :global(.icon) {
    transform: scale(1.15);
  }

  .front {
    position: relative;
    height: 100%;
    background: var(--surface);
    transition: transform var(--t-state) var(--ease);
  }

  .dragging .front {
    transition: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .front,
    .under :global(.icon),
    .under span {
      transition: none;
    }
  }
</style>
