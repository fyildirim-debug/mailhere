<script lang="ts">
  import { icons, type IconName } from './icons';

  interface Props {
    name: IconName;
    size?: number;
    /** Dolu yıldız gibi dolgulu kullanım. */
    filled?: boolean;
    label?: string;
    class?: string;
  }

  let { name, size = 18, filled = false, label, class: cls = '' }: Props = $props();
</script>

<svg
  class="icon {cls}"
  class:filled
  viewBox="0 0 24 24"
  width={size}
  height={size}
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  <!-- Sabit, kendi çizimimiz olan işaretleme (icons.ts). -->
  {@html icons[name]}
</svg>

<style>
  .icon {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .filled {
    fill: currentColor;
  }
</style>
