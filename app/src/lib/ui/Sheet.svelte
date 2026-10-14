<script lang="ts">
  // Alt sayfa (mobil): menüler, kartlar ve seçiciler ekranın altından gelir.
  // 48 px satırlar, üstte 14 px köşe, --raised + --shadow-lg, tutamaç, güvenli
  // alan. Kapanış: örtüye dokunma, Esc, tutamaçtan (ya da en üstteyken
  // içerikten) aşağı kaydırma, geri tuşu (sheet-layer.ts üzerinden gezinme
  // yığınına bağlıdır). Odak içeride döner; kapanınca açan öğeye döner.
  import { onMount, type Snippet } from 'svelte';
  import { t } from '../i18n/index.svelte';
  import { sheetClosed, sheetLayerActive, sheetOpened } from './sheet-layer';

  interface Props {
    title?: string;
    /** Erişilebilir ad; yoksa başlık. */
    label?: string;
    onclose: () => void;
    children: Snippet;
  }

  let { title, label, onclose, children }: Props = $props();

  let panel = $state<HTMLDivElement>();
  let content = $state<HTMLDivElement>();
  let dy = $state(0);
  let dragging = $state(false);
  let closing = $state(false);
  let drag: { y: number; id: number; fromHead: boolean } | null = null;
  let swallowClick = false;

  // Katman girdisi bileşen kurulurken alınır: geri tuşu etkisi (aşağıdaki
  // effect) ilk çalıştığında katman çoktan açıktır.
  sheetOpened();

  onMount(() => {
    const returnTo = document.activeElement as HTMLElement | null;
    const first = panel?.querySelector<HTMLElement>('button:not(:disabled), input, textarea, [tabindex="0"]');
    (first ?? panel)?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      onclose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      sheetClosed();
      returnTo?.focus?.({ preventScroll: true });
    };
  });

  // Geri tuşu (ya da başka bir ekranın itilmesi) katmanı kapatınca alt sayfa da kapanır.
  $effect(() => {
    if (!sheetLayerActive()) onclose();
  });

  /** Sekme tuşu alt sayfanın içinde döner. */
  function onTab(e: KeyboardEvent) {
    if (e.key !== 'Tab' || !panel) return;
    const list = [...panel.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input, textarea, select, [tabindex="0"]')];
    if (!list.length) return;
    const first = list[0];
    const last = list[list.length - 1];
    if (e.shiftKey && (document.activeElement === first || document.activeElement === panel)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  // ------------------------------------------------- aşağı kaydırarak kapatma
  // Yalnız transform; tutamaçtan her zaman, içerikten yalnız en üstteyken.

  function down(e: PointerEvent) {
    if (closing || (e.pointerType === 'mouse' && e.button !== 0)) return;
    drag = { y: e.clientY, id: e.pointerId, fromHead: (e.target as HTMLElement).closest('.head') != null };
    dragging = false;
  }

  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const my = e.clientY - drag.y;
    if (!dragging) {
      if (Math.abs(my) < 10) return;
      if (my < 0 || (!drag.fromHead && content && content.scrollTop > 0)) {
        drag = null;
        return;
      }
      dragging = true;
      panel?.setPointerCapture(e.pointerId);
    }
    dy = Math.max(0, my);
  }

  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    drag = null;
    if (!dragging) return;
    dragging = false;
    swallowClick = true;
    if (e.type !== 'pointercancel' && panel && dy > Math.min(120, panel.clientHeight / 3)) dismiss();
    else dy = 0;
  }

  /** Alt sayfa aşağı kayar, sonra kapanır. */
  function dismiss() {
    if (closing) return;
    closing = true;
    dy = (panel?.clientHeight ?? 0) + 40;
    const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
    setTimeout(onclose, reduced ? 0 : 200);
  }

  /** Sürükleme sonrası gelen tık bir satırı seçmesin. */
  function clickCapture(e: MouseEvent) {
    if (!swallowClick) return;
    swallowClick = false;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<div class="scrim" class:closing role="presentation" onclick={onclose}></div>
<div
  class="sheet"
  class:dragging
  class:closing
  role="dialog"
  aria-modal="true"
  aria-label={label ?? title ?? t('mob.sheet.label')}
  tabindex="-1"
  bind:this={panel}
  style:transform={dy ? `translateY(${dy}px)` : undefined}
  onkeydown={onTab}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onclickcapture={clickCapture}
>
  <div class="head">
    <span class="handle" aria-hidden="true"></span>
    {#if title}<div class="title">{title}</div>{/if}
  </div>
  <div class="content" bind:this={content}>
    {@render children()}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: color-mix(in srgb, var(--ink) 42%, transparent);
    animation: sheet-scrim var(--t-state) var(--ease);
  }

  .scrim.closing {
    opacity: 0;
    transition: opacity var(--t-state) var(--ease);
  }

  .sheet {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 61;
    display: flex;
    flex-direction: column;
    max-height: calc(100% - 56px);
    padding-left: env(safe-area-inset-left, 0px);
    padding-right: env(safe-area-inset-right, 0px);
    border-radius: var(--r-palette) var(--r-palette) 0 0;
    background: var(--raised);
    box-shadow: var(--shadow-lg);
    outline: none;
    animation: sheet-in var(--t-panel) var(--ease);
    transition: transform var(--t-state) var(--ease);
  }

  .sheet.dragging {
    transition: none;
  }

  .head {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    touch-action: none;
    cursor: grab;
  }

  .handle {
    width: 32px;
    height: 4px;
    margin: 10px auto 10px;
    border-radius: 999px;
    background: var(--line-strong);
  }

  .title {
    padding: 0 20px 10px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    touch-action: pan-y;
    padding: 0 8px calc(8px + env(safe-area-inset-bottom, 0px));
  }

  @keyframes sheet-in {
    from {
      transform: translateY(100%);
    }
  }

  @keyframes sheet-scrim {
    from {
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .sheet,
    .scrim {
      animation: none;
      transition: none;
    }
  }
</style>
