<script lang="ts">
  // Tek bir iletinin tam görünümü: gönderen (kişi kartı), davet, şeritler,
  // gövde ve ekler. Okuma alanında ve konuşma görünümünde kullanılır.
  import { call, toAppError, type MessageDetail } from '../api';
  import { locale, t, when } from '../i18n/index.svelte';
  import Avatar from '../ui/Avatar.svelte';
  import FileBadge from '../ui/FileBadge.svelte';
  import Icon from '../ui/Icon.svelte';
  import { showToast } from '../ui/toast.svelte';
  import { displayName, fileSize } from '../util/format';
  import { allowRemote, mail, toggleStar, unsubscribe } from '../stores/mail.svelte';
  import { darkMailWanted, settings } from '../stores/settings.svelte';
  import ContactCard from './ContactCard.svelte';
  import { openCompose } from './open';
  import InviteCard from './InviteCard.svelte';
  import MailBody from './MailBody.svelte';

  interface Props {
    detail: MessageDetail;
    /** Yanıtla / ilet düğmeleri (konuşma görünümünde her iletide). */
    actions?: boolean;
    /** İleti burada değişince (yıldız, uzak içerik izni) kapsayıcı kendi kopyasını günceller. */
    onupdate?: (d: MessageDetail) => void;
  }

  let { detail: d, actions = false, onupdate }: Props = $props();

  const account = $derived(mail.accounts.find((a) => a.id === d.accountId));
  const recipients = $derived.by(() => {
    const mine = account?.email.toLowerCase();
    return [...d.to, ...d.cc].map((a) => (a.email.toLowerCase() === mine ? t('read.me') : displayName(a))).join(', ');
  });
  const totalSize = $derived(d.attachments.reduce((s, a) => s + a.size, 0));

  /** Koyu temada posta kendi renkleriyle ("Özgün renkler"); ileti değişince sıfırlanır. */
  let original = $state(false);
  $effect(() => {
    void d.id;
    original = false;
  });
  /** Geçiş yalnız HTML postada ve koyu uyarlama etkinken; ayardan bilinir, yerleşim oynamaz. */
  const canToggleColors = $derived(d.bodyHtml != null && darkMailWanted(settings.value));

  let card = $state<{ x: number; y: number } | null>(null);
  /** Kişi kartını açan düğme; kart kapanınca odak buraya döner. */
  let cardOpener: HTMLElement | null = null;

  function openCard(e: MouseEvent) {
    cardOpener = e.currentTarget as HTMLElement;
    const r = cardOpener.getBoundingClientRect();
    card = { x: r.left, y: r.bottom + 6 };
  }

  function closeCard() {
    card = null;
    cardOpener?.focus();
  }

  /** Yıldız iyimser çevrilir; arka uç kabul etmezse eski haline döner. */
  async function star() {
    const was = d.starred;
    onupdate?.({ ...d, starred: !was });
    const now = await toggleStar(d.id, was);
    if (now === was) onupdate?.({ ...d, starred: was });
  }

  async function allow() {
    const next = await allowRemote(d.id);
    if (next) onupdate?.(next);
  }

  async function openAttachment(index: number) {
    try {
      await call('attachment_open', { id: d.id, index });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  function compose(mode: 'reply' | 'forward') {
    openCompose({ mode, messageId: d.id });
  }
</script>

<div class="sender">
  <button class="who-btn" aria-label={t('contact.open', { name: displayName(d.from) })} aria-haspopup="dialog" aria-expanded={card != null} onclick={openCard}>
    <Avatar address={d.from} size={40} />
    <span class="who">
      <span class="trunc"><span class="name">{displayName(d.from)}</span><span class="meta addr">{d.from.email}</span></span>
      <span class="meta trunc"><span class="when-inline num">{`${when(d.date)} · `}</span>{t('read.to', { list: recipients })}</span>
    </span>
  </button>
  <span class="meta num">{when(d.date)}</span>
  {#if actions}
    <button class="btn-icon small" aria-label={t('act.reply')} title={t('act.reply')} onclick={() => compose('reply')}><Icon name="reply" size={16} /></button>
    <button class="btn-icon small" aria-label={t('act.forward')} title={t('act.forward')} onclick={() => compose('forward')}><Icon name="forward" size={16} /></button>
  {/if}
  {#if canToggleColors}
    <button
      class="btn-icon"
      class:on={original}
      aria-pressed={original}
      aria-label={t('read.originalColors')}
      title={t('read.originalColors')}
      onclick={() => (original = !original)}
    >
      <Icon name="contrast" />
    </button>
  {/if}
  <button class="btn-icon" aria-label={d.starred ? t('act.unstar') : t('act.star')} title="{t('act.star')} · S" onclick={star}>
    <span class:star={d.starred}><Icon name="star" filled={d.starred} /></span>
  </button>
</div>
{#if card}
  <ContactCard address={d.from} accountId={d.accountId} at={card} onclose={closeCard} />
{/if}

{#if d.invite}
  <InviteCard detail={d} />
{/if}

{#if d.unsubscribe}
  <div class="unsub">
    <Icon name="unsubscribe" size={16} />
    <span class="grow">{t('read.listMail')}</span>
    <button class="link small" onclick={() => unsubscribe(d)}>{t('read.unsubscribe')}</button>
  </div>
{/if}

{#if d.hasRemoteContent && !d.remoteAllowed}
  <div class="remote">
    <Icon name="shield" />
    <span class="remote-text">{t('read.remoteBlocked')} <span class="remote-why">{t('read.remoteWhy')}</span></span>
    <button class="link small" onclick={allow}>{t('read.remoteAllow')}</button>
  </div>
{/if}

<div class="body">
  {#if d.bodyText == null && d.bodyHtml == null}
    <p class="meta">{t('read.noBody')}</p>
  {:else}
    <MailBody id={d.id} text={d.bodyText} html={d.bodyHtml} remoteAllowed={d.remoteAllowed} bind:original />
  {/if}
</div>

{#if d.attachments.length}
  <div class="atts">
    <div class="atts-head">
      <span class="atts-title">{t('read.attachments', { n: d.attachments.length, size: fileSize(totalSize, locale()) })}</span>
    </div>
    <div class="atts-list">
      {#each d.attachments as a (a.index)}
        <button class="att" onclick={() => openAttachment(a.index)} title={a.name}>
          <FileBadge name={a.name} mime={a.mime} />
          <span class="att-text">
            <span class="trunc att-name">{a.name}</span>
            <span class="meta">{fileSize(a.size, locale())} · {t('read.preview')}</span>
          </span>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .sender {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .who-btn {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 8px 4px 4px;
    margin-left: -4px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--ink);
    text-align: left;
    transition: background var(--t-fast);
  }

  .who-btn:hover {
    background: var(--hover);
  }

  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .name {
    font-weight: 600;
  }

  .addr {
    margin-left: 8px;
  }

  /* Dar okuma ekranı tarihi alıcı satırının başında gösterir (MobileReader). */
  .when-inline {
    display: none;
  }

  .star {
    display: inline-flex;
    color: var(--amber);
  }

  .grow {
    flex: 1;
  }

  .unsub {
    margin-top: 18px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
    color: var(--ink-2);
    font-size: var(--fs-meta);
  }

  .unsub + .remote {
    margin-top: 8px;
  }

  .remote {
    margin-top: 18px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
    color: var(--accent-text);
  }

  .remote-text {
    flex: 1;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .link.small {
    font-size: var(--fs-meta);
  }

  .body {
    margin-top: 22px;
  }

  .atts {
    margin-top: 26px;
    padding-top: 18px;
    border-top: 1px solid var(--line);
  }

  .atts-head {
    margin-bottom: 10px;
  }

  .atts-title {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .atts-list {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .att {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 252px;
    padding: 8px 12px 8px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    background: var(--paper);
    color: var(--ink);
    text-align: left;
    transition:
      border-color var(--t-fast),
      background var(--t-fast);
  }

  .att:hover {
    border-color: var(--line-strong);
    background: var(--hover);
  }

  .att-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .att-name {
    font-weight: 500;
  }
</style>
