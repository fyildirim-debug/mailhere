<script lang="ts">
  import { fileBadge, fileKind } from '../util/format';

  interface Props {
    name: string;
    mime?: string;
    size?: number;
  }

  let { name, mime = '', size = 36 }: Props = $props();

  const kind = $derived(fileKind(name, mime));
</script>

<span
  class="badge"
  style:width="{size}px"
  style:height="{size}px"
  style:background="var(--file-{kind}-bg)"
  style:color="var(--file-{kind}-fg)"
  aria-hidden="true">{fileBadge(name)}</span
>

<style>
  .badge {
    display: grid;
    flex: none;
    place-items: center;
    border-radius: var(--r-icon);
    font-size: var(--fs-meta);
    font-weight: 600;
    letter-spacing: 0.01em;
  }
</style>
