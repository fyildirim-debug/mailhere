<script lang="ts">
  // Okuma ekranının tek alt şeridi (56 px + güvenli alan): geniş "Yanıtla"
  // (yumuşak vurgu), Arşivle, Sil ve "Diğer eylemler" (tümüne yanıtla, ilet,
  // taşı, etiket…). Taslakta "Düzenle", Sil ve diğerleri.
  //
  // "Yanıtla" şeridi yerinde yanıt alanına çevirir (QuickReply `compact`,
  // 280 ms yalnız transform/opacity; azaltılmış harekette anlık): alıcılar,
  // hazır yanıtlar, kutu, gönder, tam ekran yaz ve kapat. Alan gezinme
  // yığınında `reply` katmanıdır: geri tuşu önce onu kapatır. Yazılan metin
  // alan kapanınca korunur ("Yanıta devam et"); tam ekran yazmaya taşınır.
  import { tick } from 'svelte';
  import type { MessageDetail } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import QuickReply from '../../lib/mail/QuickReply.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { nav } from './nav.svelte';

  interface Props {
    detail: MessageDetail | null;
    draft: boolean;
    onarchive: () => void;
    ontrash: () => void;
    onmore: () => void;
    oneditdraft: () => void;
    /** Tam ekran yaz: metin yazma ekranına taşınır (ekran itilir). */
    onfull: (text: string) => void;
  }

  let { detail: d, draft, onarchive, ontrash, onmore, oneditdraft, onfull }: Props = $props();

  let text = $state('');
  let quick = $state<QuickReply>();
  /** Bu şerit alanı açtı mı (başka postanın açık bıraktığı katman burada açılmaz). */
  let opened = $state(false);
  /** Alan açık mı: gezinme katmanı açıkken (geri tuşu kapatınca false). */
  const expanded = $derived(opened && nav.has('reply'));

  async function expand() {
    if (!d) return;
    opened = true;
    nav.open('reply');
    await tick();
    quick?.focus();
  }

  function collapse() {
    nav.closeLayer('reply');
  }

  function full() {
    const body = text;
    text = '';
    // Ekran itilince katman girdisi yazma ekranına devredilir.
    onfull(body);
  }

</script>

{#if expanded && d && !draft}
  <section class="replying" aria-label={t('mob.reply.panel')}>
    <QuickReply bind:this={quick} bind:text detail={d} compact onclose={collapse} onfull={full} onsent={collapse} />
  </section>
{:else}
  <div class="strip" role="toolbar" aria-label={t('mob.read.actions')}>
    {#if draft}
      <button class="wide" disabled={!d} onclick={oneditdraft}>
        <Icon name="pen" size={20} /><span class="trunc">{t('mob.read.editDraft')}</span>
      </button>
    {:else}
      <button class="wide" class:resume={!!text} disabled={!d} onclick={expand}>
        <Icon name="reply" size={20} /><span class="trunc">{text ? t('mob.reply.resume') : t('act.reply')}</span>
      </button>
      <button class="btn-icon act" disabled={!d} aria-label={t('act.archive')} title={t('act.archive')} onclick={onarchive}>
        <Icon name="archive" size={22} />
      </button>
    {/if}
    <button class="btn-icon act" disabled={!d} aria-label={t('act.delete')} title={t('act.delete')} onclick={ontrash}>
      <Icon name="trash" size={22} />
    </button>
    <button class="btn-icon act" disabled={!d} aria-label={t('read.more')} title={t('read.more')} aria-haspopup="menu" onclick={onmore}>
      <Icon name="more" size={22} />
    </button>
  </div>
{/if}

<style>
  .strip {
    position: relative;
    z-index: 6;
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: calc(var(--m-bar) + var(--sab));
    /* 1 px üst çizgi + 5 + 44 + 6 = 56 */
    padding: 5px calc(8px + var(--sar)) calc(6px + var(--sab)) calc(12px + var(--sal));
    background: var(--raised);
    border-top: 1px solid var(--line);
  }

  /* Birincil eylem: geniş, yumuşak vurgu zemininde vurgu metni. */
  .wide {
    flex: 1;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: var(--m-touch);
    margin-right: 4px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-card);
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
    transition:
      background var(--t-fast),
      transform var(--t-fast) var(--ease);
  }

  .wide:active {
    transform: scale(0.98);
  }

  .wide:disabled,
  .act:disabled {
    opacity: var(--dim-disabled);
  }

  .act {
    flex: none;
    color: var(--ink-2);
  }

  /* Yanıt alanı şeridin yerinde açılır: aşağıdan hafifçe gelir. */
  .replying {
    position: relative;
    z-index: 6;
    flex: none;
    padding: 4px calc(12px + var(--sar)) calc(10px + var(--sab)) calc(12px + var(--sal));
    background: var(--raised);
    border-top: 1px solid var(--line);
    animation: reply-in var(--t-panel) var(--ease);
  }

  @keyframes reply-in {
    from {
      opacity: 0;
      transform: translateY(16px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .replying {
      animation: none;
    }

    .wide {
      transition: background var(--t-fast);
    }
  }
</style>
