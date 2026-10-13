<script lang="ts">
  import type { Address } from '../api/types';
  import { avatarTone, initials } from '../util/format';

  interface Props {
    address: Address;
    size?: number;
  }

  let { address, size = 34 }: Props = $props();

  const tone = $derived(avatarTone(address.email));
</script>

<span
  class="avatar"
  style:width="{size}px"
  style:height="{size}px"
  style:font-size={size >= 40 ? '15px' : '12px'}
  style:background="var(--av-{tone}-bg)"
  style:color="var(--av-{tone}-fg)"
  aria-hidden="true">{initials(address)}</span
>

<style>
  .avatar {
    display: grid;
    flex: none;
    place-items: center;
    border-radius: 50%;
    font-weight: 600;
    letter-spacing: 0.02em;
    line-height: 1;
  }
</style>
