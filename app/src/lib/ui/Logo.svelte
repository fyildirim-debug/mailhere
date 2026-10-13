<script lang="ts">
  interface Props {
    size?: number;
    /** Ters renk: vurgu zemin üstünde beyaz döşeme. */
    inverse?: boolean;
    dot?: boolean;
  }

  let { size = 24, inverse = false, dot = true }: Props = $props();
</script>

<svg class="logo" class:inverse viewBox="0 0 28 28" width={size} height={size} aria-hidden="true">
  <rect class="tile" x="0" y="0" width="28" height="28" rx="8" />
  <path class="mark" d="M8 10.5h12v8.5a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1z" />
  <path class="mark" d="M8.6 11.2l5.4 4.1 5.4-4.1" />
  {#if dot}<circle class="dot" cx="21.5" cy="6.5" r="3.2" />{/if}
</svg>

<style>
  .logo {
    flex: none;
  }

  .tile {
    fill: var(--accent);
  }

  .mark {
    fill: none;
    stroke: var(--accent-ink);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .dot {
    fill: var(--signal);
    stroke: var(--accent);
    stroke-width: 1.4;
  }

  .inverse .tile {
    fill: var(--brand-ink);
  }

  .inverse .mark {
    stroke: var(--brand);
  }

  .inverse .dot {
    stroke: var(--brand);
  }
</style>
