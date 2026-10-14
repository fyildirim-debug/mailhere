<script lang="ts">
  // Mobil liste satırı (72 px, sıkı 60): gönderen + saat, konu, önizleme.
  // Seçim kipinde avatarın yerine işaret gelir; hareket mantığı SwipeRow'da.
  import { locale, t } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { followUpDue } from '../../lib/util/followup';
  import { displayName, listTime } from '../../lib/util/format';
  import type { Row } from '../../lib/stores/mail.svelte';
  import { tagsOf } from '../../lib/stores/tags.svelte';

  interface Props {
    message: Row;
    now: number;
    compact: boolean;
    checked: boolean;
    /** Seçim kipi: dokunma işaretler, açmaz. */
    selecting: boolean;
    onopen: () => void;
    ontoggle: () => void;
  }

  let { message, now, compact, checked, selecting, onopen, ontoggle }: Props = $props();

  const tags = $derived(tagsOf(message.tags));
  // Yanıt hatırlatmalı (gönderilmiş) satırda beklenen kişi, yani ilk alıcı görünür.
  const who = $derived(message.followUp != null && message.to[0] ? message.to[0] : message.from);
  const awaiting = $derived(followUpDue(message.followUp, now));
</script>

<button
  class="row"
  class:unread={message.unread}
  class:checked
  class:compact
  data-id={message.id}
  aria-pressed={selecting ? checked : undefined}
  aria-label={selecting ? t('list.selectRow', { subject: message.subject || displayName(message.from) }) : undefined}
  onclick={() => (selecting ? ontoggle() : onopen())}
>
  <span class="dot" aria-hidden="true"></span>
  <span class="av" aria-hidden="true">
    {#if checked}
      <span class="tick"><Icon name="check" size={20} /></span>
    {:else}
      <Avatar address={who} size={40} />
    {/if}
  </span>
  <span class="text">
    <span class="line1">
      <span class="sender trunc">{displayName(who)}</span>
      {#if awaiting}<span class="tag">{t('follow.awaiting')}</span>{/if}
      {#if message.count > 1}<span class="count num">{message.count}</span>{/if}
      {#if message.hasAttachments}<Icon name="paperclip" size={14} label={t('list.attachment')} class="quiet" />{/if}
      {#if message.starred}<span class="star"><Icon name="star" size={14} filled label={t('list.starred')} /></span>{/if}
      {#if message.pinned}<Icon name="pin" size={14} label={t('list.pinnedMark')} class="quiet" />{/if}
      {#if tags.length}
        <span class="tags" role="img" aria-label={t('list.tagsOf', { list: tags.map((x) => x.name).join(', ') })}>
          {#each tags.slice(0, 3) as tg (tg.id)}<span class="tdot" style:background={tg.color}></span>{/each}
        </span>
      {/if}
      <span class="time num">{listTime(message.date, now, locale())}</span>
    </span>
    <span class="subject trunc">{message.subject || '—'}</span>
    {#if !compact}<span class="snippet trunc">{message.snippet}</span>{/if}
  </span>
</button>

<style>
  .row {
    display: grid;
    grid-template-columns: 8px 40px minmax(0, 1fr);
    column-gap: 10px;
    align-items: start;
    width: 100%;
    height: 100%;
    padding: 6px 16px 6px 6px;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--ink);
    line-height: 20px;
    text-align: left;
    transition: background var(--t-fast);
  }

  .row:active {
    background: var(--hover);
  }

  .row.checked {
    background: var(--sel);
  }

  .row:focus-visible {
    outline-offset: -2px;
  }

  .dot {
    width: 7px;
    height: 7px;
    margin-top: 7px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0;
  }

  .unread .dot {
    opacity: 1;
  }

  .av {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    margin-top: 10px;
  }

  .compact .av {
    margin-top: 4px;
  }

  .tick {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-ink);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .compact .text {
    margin-top: 4px;
  }

  .line1 {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .sender {
    font-weight: 500;
  }

  .unread .sender {
    font-weight: 600;
  }

  .count {
    flex: none;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .line1 :global(.quiet) {
    color: var(--ink-3);
  }

  .star {
    display: inline-flex;
    color: var(--amber);
  }

  .tags {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .tdot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .time {
    flex: none;
    margin-left: auto;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .subject {
    color: var(--ink-2);
  }

  .unread .subject {
    color: var(--ink);
    font-weight: 500;
  }

  .snippet {
    color: var(--ink-3);
  }
</style>
