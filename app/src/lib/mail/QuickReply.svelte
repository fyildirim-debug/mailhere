<script lang="ts">
  // Hızlı yanıt: çipler (ayarlardaki hazır metinler, değişkenleri doldurulmuş)
  // ve kısa yanıt kutusu. Okuma alanında (masaüstü) ve mobil okuma ekranında
  // (`compact`) aynı mantık; gönderim `sendQuickReply` ile giden kutusuna
  // gider ve geri alınırsa metin geri gelir.
  import type { MessageDetail } from '../api';
  import { locale, t } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';
  import { foreignReplyDomains } from '../util/format';
  import { fillVariables } from '../util/variables';
  import { mail, quickReplyTargets, sendQuickReply } from '../stores/mail.svelte';
  import { quickReplies, settings } from '../stores/settings.svelte';

  interface Props {
    detail: MessageDetail;
    /** Mobil: tek satırlık kutu + yuvarlak gönder düğmesi; alıcı satırı yalnız yazarken. */
    compact?: boolean;
  }

  let { detail: d, compact = false }: Props = $props();

  let reply = $state('');
  let sending = $state(false);
  let focused = $state(false);
  let replyBox = $state<HTMLTextAreaElement>();

  const account = $derived(mail.accounts.find((a) => a.id === d.accountId));
  // Yanıt alıcıları adresleriyle gösterilir: gizli bir Reply-To fark edilsin.
  const replyList = $derived(quickReplyTargets(d, account).map((a) => a.email).join(', '));
  const replyForeign = $derived(foreignReplyDomains(d));
  const chips = $derived(quickReplies(settings.value));
  /** Mobilde alıcı satırı yalnız kutu etkinken; masaüstünde hep. */
  const showTargets = $derived(!compact || focused || reply.length > 0);
  /** Mobilde hazır yanıtlar da yalnız kutu odaktayken: postaya yer kalır. */
  const showChips = $derived(chips.length > 0 && !reply && (!compact || focused));

  /** Hızlı yanıt değişkenleri: yanıtlanan kişi ve hesabın gönderen adı. */
  function useChip(text: string) {
    reply = fillVariables(text, {
      recipient: quickReplyTargets(d, account)[0] ?? null,
      sender: account ? { name: account.displayName, email: account.email } : null,
      locale: locale(),
    });
    replyBox?.focus();
  }

  export function focus() {
    replyBox?.focus();
  }

  async function send() {
    if (!reply.trim() || sending) return;
    sending = true;
    const text = reply;
    const ok = await sendQuickReply(d, text, (back) => (reply = back));
    if (ok) reply = '';
    sending = false;
  }

  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      void send();
    }
  }
</script>

<div class="quick" class:compact>
  {#if replyForeign.length && showTargets}
    <p class="reply-warn" role="note">{t('read.replyElsewhere', { domains: replyForeign.join(', ') })}</p>
  {/if}
  {#if showChips}
    <div class="chips" role="group" aria-label={t('qr.title')}>
      {#each chips as c (c)}
        <!-- Basış kutudan odağı almaz: çip kaybolmadan tıklama yerine ulaşır. -->
        <button class="chip" onmousedown={(e) => e.preventDefault()} onclick={() => useChip(c)}>{c}</button>
      {/each}
    </div>
  {/if}
  <div class="replybox">
    {#if compact && showTargets}
      <span class="meta trunc targets" title={replyList}>{t('read.replyTo', { list: replyList })}</span>
    {/if}
    <div class="line">
      <textarea
        bind:this={replyBox}
        bind:value={reply}
        rows={compact ? 1 : 2}
        placeholder={t('read.replyPlaceholder')}
        aria-label={t('read.replyPlaceholder')}
        onkeydown={onKey}
        onfocus={() => (focused = true)}
        onblur={() => (focused = false)}
      ></textarea>
      {#if compact}
        <button class="btn-primary send-round" aria-label={t('act.send')} title={t('act.send')} disabled={!reply.trim() || sending} onclick={send}>
          <Icon name="send" size={20} />
        </button>
      {/if}
    </div>
    {#if !compact}
      <div class="reply-bar">
        <span class="meta trunc" title={replyList}>{t('read.replyTo', { list: replyList })}</span>
        <span class="grow"></span>
        <span class="meta">{t('read.sendHint')}</span>
        <button class="btn-primary small" disabled={!reply.trim() || sending} onclick={send}><Icon name="send" size={16} />{t('act.send')}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .chips {
    max-width: 680px;
    margin: 0 auto 8px;
    display: flex;
    gap: 6px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .chips::-webkit-scrollbar {
    display: none;
  }

  .chip {
    flex: none;
    height: 28px;
    padding: 0 12px;
    border-radius: 999px;
    border: 1px solid var(--line-strong);
    background: var(--paper);
    color: var(--ink-2);
    font-size: var(--fs-meta);
    white-space: nowrap;
    transition:
      background var(--t-fast),
      border-color var(--t-fast),
      color var(--t-fast);
  }

  .chip:hover {
    background: var(--surface);
    border-color: var(--ink-3);
    color: var(--ink);
  }

  .reply-warn {
    margin: 0 0 8px;
    font-size: var(--fs-meta);
    color: var(--amber);
  }

  .replybox {
    max-width: 680px;
    margin: 0 auto;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-float);
    background: var(--paper);
    transition:
      border-color var(--t-state),
      box-shadow var(--t-state);
  }

  .replybox:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  .line {
    display: flex;
    align-items: flex-end;
  }

  textarea {
    display: block;
    flex: 1;
    width: 100%;
    min-width: 0;
    min-height: 46px;
    max-height: 220px;
    padding: 12px 14px 4px;
    border: 0;
    outline: none;
    resize: none;
    background: transparent;
    line-height: 1.5;
    field-sizing: content;
  }

  .reply-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px 8px 14px;
  }

  .grow {
    flex: 1;
  }

  /* ------------------------------------------------------------ mobil (compact) */

  .compact .chips {
    margin: 0 0 8px;
    padding: 0 2px;
  }

  .compact .chip {
    height: 32px;
    padding: 0 14px;
  }

  .compact .replybox {
    max-width: none;
    margin: 0;
  }

  .compact .targets {
    display: block;
    padding: 8px 14px 0;
  }

  .compact textarea {
    min-height: 44px;
    max-height: 140px;
    padding: 11px 14px;
  }

  .send-round {
    flex: none;
    width: 40px;
    height: 40px;
    margin: 0 4px 4px 0;
    padding: 0;
    border-radius: 50%;
  }
</style>
