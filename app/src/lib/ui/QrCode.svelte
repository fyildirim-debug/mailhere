<script lang="ts">
  // QR kodu: modül başına bir `rect`, 4 modül sessiz bölge. Zemin ve modül
  // rengi her temada sabittir (--qr-paper / --qr-ink): telefon kamerası ters
  // kodu güvenilir okumaz, okunabilirlik temadan önce gelir (TASARIM.md §2).
  import type { QrMatrix } from '../api/types';
  import { qrCells } from './qr';

  interface Props {
    qr: QrMatrix;
    /** Piksel; sessiz bölge dahil. */
    size?: number;
    label: string;
  }

  let { qr, size = 240, label }: Props = $props();

  const drawing = $derived(qrCells(qr));
</script>

<svg class="qr" role="img" aria-label={label} viewBox="0 0 {drawing.side} {drawing.side}" width={size} height={size} shape-rendering="crispEdges">
  <rect class="bg" width={drawing.side} height={drawing.side} />
  {#each drawing.cells as c (c.y * drawing.side + c.x)}
    <rect class="cell" x={c.x} y={c.y} width="1" height="1" />
  {/each}
</svg>

<style>
  .qr {
    display: block;
    flex: none;
  }

  .bg {
    fill: var(--qr-paper);
  }

  .cell {
    fill: var(--qr-ink);
  }
</style>
