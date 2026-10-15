<script lang="ts">
  // Sütun ayırıcısı: iki sütun arasındaki 1 px çizgi, 9 px tutma alanı.
  // Fareyle sürüklenir (sınırlar içinde), odaktayken ←/→ 8 px adım, Home/End
  // en dar / en geniş, çift tık varsayılan genişliğe döner. Pencere bölücü
  // deseni: role="separator" + aria-valuenow; değer soldaki sütunun genişliği.
  import { STEP } from './layout.svelte';

  interface Props {
    label: string;
    value: number;
    min: number;
    max: number;
    def: number;
    /** `persist` false iken sürükleme sürüyor; bırakınca true ile bir kez daha çağrılır. */
    onchange: (v: number, persist: boolean) => void;
  }

  let { label, value, min, max, def, onchange }: Props = $props();

  let drag = $state<{ x: number; w: number } | null>(null);

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    (e.currentTarget as HTMLElement).focus();
    drag = { x: e.clientX, w: value };
  }

  function moveTo(e: PointerEvent) {
    if (!drag) return;
    onchange(clampV(drag.w + (e.clientX - drag.x)), false);
  }

  function up(e: PointerEvent) {
    if (!drag) return;
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    drag = null;
    onchange(value, true);
  }

  function clampV(v: number) {
    return Math.round(Math.min(Math.max(v, min), max));
  }

  function key(e: KeyboardEvent) {
    let next: number | null = null;
    if (e.key === 'ArrowLeft') next = value - STEP;
    else if (e.key === 'ArrowRight') next = value + STEP;
    else if (e.key === 'Home') next = min;
    else if (e.key === 'End') next = max;
    else if (e.key === 'Enter') next = def;
    if (next == null) return;
    e.preventDefault();
    e.stopPropagation();
    onchange(clampV(next), true);
  }
</script>

<!-- Odaklanabilir ayırıcı (WAI-ARIA pencere bölücü): değeri değişen bir denetimdir. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="split"
  class:drag={drag != null}
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={value}
  aria-valuemin={min}
  aria-valuemax={max}
  tabindex="0"
  title={label}
  onpointerdown={down}
  onpointermove={moveTo}
  onpointerup={up}
  onpointercancel={up}
  ondblclick={() => onchange(clampV(def), true)}
  onkeydown={key}
></div>

<style>
  .split {
    position: relative;
    z-index: 5;
    flex: none;
    width: 1px;
    background: var(--line);
    cursor: col-resize;
    touch-action: none;
    transition: background var(--t-fast);
  }

  /* Tutma alanı çizginin iki yanına taşar; görünmez. */
  .split::before {
    content: '';
    position: absolute;
    inset: 0 -4px;
  }

  .split:hover,
  .split.drag {
    background: var(--line-strong);
  }

  /* Sürüklerken ve klavye odağında çizgi vurgulanır (odak halkası yerine). */
  .split.drag,
  .split:focus-visible {
    outline: none;
    background: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
</style>
