<script lang="ts" module>
  import type { SwipeAction } from '../../lib/api';
  import type { IconName } from '../../lib/ui/icons';

  /** Kaydırma eyleminin görünümü: ikon, ad ve eşik geçilince dolan zemin jetonu. */
  export interface SwipeLook {
    icon: IconName;
    label: string;
    /** Arşivle ve okundu vurgu (okunmamış işareti), ertele kehribar, sil ve istenmeyen tehlike, taşı ve sabitle sessiz mürekkep. */
    tone: 'accent' | 'amber' | 'danger' | 'neutral';
  }

  /**
   * Bu eylem satırı listeden çıkarır mı (çıkarmıyorsa satır yerine döner).
   * Klasöre taşı önce seçici açar: satır yerine döner, taşıma seçimden sonra.
   */
  export function removesRow(a: SwipeAction): boolean {
    return a === 'archive' || a === 'snooze' || a === 'trash' || a === 'spam';
  }
</script>

<script lang="ts">
  // Kaydırmalı satır: yön başına eylem ayarlardan gelir (sola varsayılan
  // Arşivle, sağa Ertele; `none` o yönü kapatır). Eşik genişliğin %40'ı;
  // eşiğe kadar zemin nötr (`--paper`, ikon `--ink-3`), eşik geçilince zemin
  // eylemin jetonuyla dolar, ikon 120 ms'de büyür ve bir kez titreşir
  // (`haptic('tick')`). Bırakınca: satırı çıkaran eylemde satır kenardan
  // çıkar, çıkarmayanda (okundu, sabitle, klasör seçici, İstenmeyen kutusu
  // olmayan hesap) yerine döner; eylem ardından çalışır.
  // Yalnız transform canlandırılır (200 ms), azaltılmış harekette anlık.
  // Uzun basış (450 ms, kıpırdamadan) seçim için ayrı olay; dikey kaydırma
  // tarayıcıya bırakılır (touch-action: pan-y).
  import type { Snippet } from 'svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { haptic } from './haptic';

  interface Props {
    /** Sola (parmak sola) ve sağa kaydırma eylemleri. */
    left: SwipeAction;
    right: SwipeAction;
    /** Eylemin görünümü (satıra göre: okundu/okunmadı, sabitle/kaldır). */
    look: (a: SwipeAction) => SwipeLook | null;
    /** Satırı çıkarır mı (ör. İstenmeyen kutusu olmayan hesapta hayır); yoksa `removesRow`. */
    removes?: (a: SwipeAction) => boolean;
    onaction: (a: SwipeAction) => void;
    onlongpress?: () => void;
    /** Seçim kipinde kaydırma kapalıdır. */
    disabled?: boolean;
    children: Snippet;
  }

  let { left, right, look, removes = removesRow, onaction, onlongpress, disabled = false, children }: Props = $props();

  const THRESHOLD = 0.4;
  const SLOP = 10;
  const LONG_PRESS = 450;
  /** Kapalı yönde parmağı izlemez, yalnız hafifçe direnir. */
  const RESIST = 24;

  let el = $state<HTMLDivElement>();
  let width = $state(360);
  let dx = $state(0);
  let dragging = $state(false);
  let leaving = $state<'left' | 'right' | null>(null);
  let start: { x: number; y: number; id: number } | null = null;
  let pressTimer: ReturnType<typeof setTimeout> | null = null;
  let swallowClick = false;

  const enabled = $derived(!disabled && (left !== 'none' || right !== 'none'));

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
      if (!enabled || Math.abs(mx) < Math.abs(my)) {
        // Dikey niyet, seçim kipi ya da kaydırma kapalı: tarayıcı kaydırır, biz çekiliriz.
        start = null;
        return;
      }
      dragging = true;
      el?.setPointerCapture(e.pointerId);
    }
    const action = mx < 0 ? left : right;
    const next = action === 'none' ? Math.sign(mx) * Math.min(RESIST, Math.abs(mx) / 4) : Math.max(-width, Math.min(width, mx));
    const wasArmed = armed;
    dx = next;
    // Eşik geçildiği an bir kez titreşir (geri dönüp yeniden geçince yine).
    if (!wasArmed && armedAt(next)) haptic('tick');
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
    if (e.type !== 'pointercancel' && armedAt(dx)) commit(dx < 0 ? 'left' : 'right');
    else dx = 0;
  }

  function armedAt(x: number): boolean {
    const action = x < 0 ? left : x > 0 ? right : 'none';
    return action !== 'none' && Math.abs(x) >= width * THRESHOLD;
  }

  /** Satırı çıkaran eylemde satır kenardan çıkar, sonra eylem çalışır; diğerinde yerine döner. */
  function commit(dir: 'left' | 'right') {
    const action = dir === 'left' ? left : right;
    const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (!removes(action)) {
      dx = 0;
      onaction(action);
      return;
    }
    leaving = dir;
    dx = dir === 'left' ? -width : width;
    setTimeout(
      () => {
        onaction(action);
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
  const armed = $derived(armedAt(dx));
  const shown = $derived(side ? look(side === 'left' ? left : right) : null);
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
  {#if side && shown}
    <div class="under {side} tone-{shown.tone}" class:armed={armed || leaving != null} aria-hidden="true" data-action={side === 'left' ? left : right}>
      <Icon name={shown.icon} size={22} />
      <span>{shown.label}</span>
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

  /* Eşiğe kadar nötr: kâğıt zemin, sessiz ikon; niyet belli olunca renk gelir. */
  .under {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 20px;
    background: var(--paper);
    color: var(--ink-3);
    font-size: var(--fs-meta);
    font-weight: 600;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .under.left {
    justify-content: flex-end;
  }

  .under.right {
    justify-content: flex-start;
  }

  .under.armed {
    color: var(--accent-ink);
  }

  .under.armed.tone-accent {
    background: var(--accent);
  }

  .under.armed.tone-amber {
    background: var(--amber);
  }

  .under.armed.tone-danger {
    background: var(--danger);
  }

  .under.armed.tone-neutral {
    background: var(--ink-2);
    color: var(--paper);
  }

  .under :global(.icon) {
    transition: transform var(--t-fast) var(--ease);
  }

  /* Eşik geçilince işaret büyür: bırakınca ne olacağı belli. */
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
    .under,
    .under :global(.icon) {
      transition: none;
    }
  }
</style>
