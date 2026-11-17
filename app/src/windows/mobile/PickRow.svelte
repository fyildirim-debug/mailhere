<script lang="ts">
  // Mobil ayar satırı: ad (ve açıklama) solda, seçili değer sağda; satırın
  // tamamı dokunma hedefidir (56 px) ve seçenekleri alt sayfada açar.
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';

  interface Props {
    name: string;
    hint?: string;
    value: string;
    /** Değerin ikonu (kaydırma eylemleri). */
    icon?: IconName;
    onclick: () => void;
  }

  let { name, hint, value, icon, onclick }: Props = $props();
</script>

<button class="pick" aria-haspopup="menu" {onclick}>
  <span class="set-name">{name}{#if hint}<span class="set-hint">{hint}</span>{/if}</span>
  <span class="value">{#if icon}<Icon name={icon} size={18} />{/if}{value}</span>
  <Icon name="chevronRight" size={18} class="chev" />
</button>

<style>
  /* Ayar satırı gibi: ad solda, seçili değer sağda, alt çizgi; tamamı dokunma hedefi. */
  .pick {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 56px;
    padding: 10px 0;
    border: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
    color: var(--ink);
    text-align: left;
    transition: background var(--t-fast);
  }

  .pick:active {
    background: var(--hover);
  }

  .pick .set-name {
    flex: 1;
    min-width: 0;
  }

  .value {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 55%;
    color: var(--ink-2);
    text-align: right;
  }

  .value :global(.icon) {
    color: var(--ink-3);
  }

  .pick :global(.chev) {
    color: var(--ink-3);
  }
</style>
