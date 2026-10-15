<script lang="ts">
  // Kartın eylem sırası: birincil "Aç" (doğrulama kodunda "Kodu kopyala" +
  // ikincil "Aç") ve ayarda seçilen en çok üç düğme. Etiketler sığmazsa
  // (kod kartı, uzun dil) seçilen düğmeler ikona iner; ad aria-label'da kalır.
  import { onMount, tick } from 'svelte';
  import type { NotifyAction } from '../../lib/api';
  import { locale, t } from '../../lib/i18n/index.svelte';
  import { NOTIFY_ACTION_INFO } from '../../lib/notify-actions';
  import Icon from '../../lib/ui/Icon.svelte';

  interface Props {
    actions: NotifyAction[];
    otp: string | null | undefined;
    copied: boolean;
    busy: boolean;
    onopen: () => void;
    oncopy: () => void;
    onaction: (a: NotifyAction) => void;
  }

  let { actions, otp, copied, busy, onopen, oncopy, onaction }: Props = $props();

  let row = $state<HTMLDivElement>();
  let compact = $state(false);

  /** Etiketli hali ölçülür; taşıyorsa ikonlu hale geçilir. */
  async function fit() {
    compact = false;
    await tick();
    if (row && row.scrollWidth > row.clientWidth + 1) compact = true;
  }

  $effect(() => {
    // Düğmeler, dil ya da "Kod kopyalandı" değişince yeniden ölç.
    void actions.join();
    void otp;
    void copied;
    void locale();
    void fit();
  });

  onMount(() => {
    // Yazı tipi yüklenince ölçü değişebilir.
    void document.fonts?.ready.then(fit);
  });
</script>

<div class="acts" class:compact bind:this={row} role="group" aria-label={t('notify.actions')}>
  {#if otp}
    <!-- Doğrulama kodu postası: birincil eylem kodu kopyalamak; açmak ikincil kalır. -->
    <button class="btn-primary small" onclick={oncopy}>{copied ? t('notify.codeCopied') : t('notify.copyCode')}</button>
    <button class="btn-soft small" onclick={onopen}>{t('act.open')}</button>
  {:else}
    <button class="btn-primary small" onclick={onopen}>{t('act.open')}</button>
  {/if}
  {#each actions as a, i (a)}
    {@const info = NOTIFY_ACTION_INFO[a]}
    <button
      class="btn-soft small act"
      class:first={i === 0}
      data-action={a}
      aria-label={t(info.aria)}
      title={t(info.aria)}
      disabled={busy}
      onclick={() => onaction(a)}
    >
      {#if compact}<Icon name={info.icon} size={16} />{:else}{t(info.label)}{/if}
    </button>
  {/each}
</div>

<style>
  .acts {
    display: flex;
    gap: 8px;
    min-width: 0;
    margin-top: 14px;
  }

  .acts > button {
    flex: none;
  }

  /* İkonlu hal: seçilen düğmeler sağda, kare. */
  .compact .act {
    width: 32px;
    padding: 0;
    color: var(--ink-2);
  }

  .compact .act:hover {
    color: var(--ink);
  }

  .compact .act.first {
    margin-left: auto;
  }
</style>
