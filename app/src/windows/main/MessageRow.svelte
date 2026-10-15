<script lang="ts">
  import { call } from '../../lib/api';
  import { locale, t } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { followUpDue } from '../../lib/util/followup';
  import { displayName, listTime, snoozeOptions } from '../../lib/util/format';
  import { mail, moveTo, snooze, type Row } from '../../lib/stores/mail.svelte';
  import { clickRow, isChecked, toggleCheck } from '../../lib/stores/selection.svelte';
  import { tagsOf } from '../../lib/stores/tags.svelte';
  import { showReader } from './layout.svelte';
  import { teach } from './teach.svelte';

  interface Props {
    message: Row;
    now: number;
    compact: boolean;
  }

  let { message, now, compact }: Props = $props();

  const selected = $derived(mail.selectedId != null && message.ids.includes(mail.selectedId));
  const checked = $derived(isChecked(message.id));
  const tags = $derived(tagsOf(message.tags));
  const tomorrow = $derived(snoozeOptions(now).find((o) => o.key === 'tomorrow')!.at);
  // Yanıt hatırlatmalı (gönderilmiş) satırda beklenen kişi, yani ilk alıcı görünür.
  const who = $derived(message.followUp != null && message.to[0] ? message.to[0] : message.from);
  const awaiting = $derived(followUpDue(message.followUp, now));
</script>

<div class="row" class:selected class:checked class:unread={message.unread} class:compact data-id={message.id}>
  <button
    class="main"
    aria-current={selected ? 'true' : undefined}
    onclick={(e) => {
      clickRow(e, message.id);
      // Yığın kipinde (dar pencere) düz tık okuma alanını açar.
      if (!e.ctrlKey && !e.metaKey && !e.shiftKey) showReader();
    }}
    ondblclick={() => call('open_quick', { id: message.id })}
  >
    <span class="dot" aria-hidden="true"></span>
    <Avatar address={who} />
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
            {#each tags.slice(0, 3) as tg (tg.id)}<span class="tdot" style:background={tg.color} title={tg.name}></span>{/each}
            {#if tags.length > 3}<span class="more num">+{tags.length - 3}</span>{/if}
          </span>
        {/if}
        <span class="time num">{listTime(message.date, now, locale())}</span>
      </span>
      <span class="subject trunc">{message.subject || '—'}</span>
      {#if !compact}<span class="snippet trunc">{message.snippet}</span>{/if}
    </span>
  </button>
  <!-- Avatarın üstünde seçim düğmesi: üzerine gelince ya da seçiliyken görünür. -->
  <button
    class="pick"
    aria-label={t('list.selectRow', { subject: message.subject || displayName(message.from) })}
    aria-pressed={checked}
    onclick={() => toggleCheck(message.id)}
  >
    <Icon name="check" size={16} />
  </button>
  <div class="acts">
    <button class="btn-icon small" aria-label={t('act.archive')} title="{t('act.archive')} · E" onclick={(e) => void teach('archive', e, moveTo(message.ids, 'archive'))}>
      <Icon name="archive" size={16} />
    </button>
    <button class="btn-icon small" aria-label={t('act.snooze')} title="{t('snooze.tomorrow')} · H" onclick={(e) => void teach('snooze', e, snooze(message.ids, tomorrow))}>
      <Icon name="clock" size={16} />
    </button>
    <button class="btn-icon small" aria-label={t('act.delete')} title="{t('act.delete')} · #" onclick={(e) => void teach('delete', e, moveTo(message.ids, 'trash'))}>
      <Icon name="trash" size={16} />
    </button>
  </div>
</div>

<style>
  .row {
    position: relative;
    margin: 0 8px;
    border-radius: var(--r-row);
    transition: background var(--t-fast);
  }

  .row:hover {
    background: var(--hover);
  }

  .row.selected,
  .row.checked {
    background: var(--sel);
  }

  .main {
    display: grid;
    grid-template-columns: 8px 34px minmax(0, 1fr);
    column-gap: 10px;
    align-items: start;
    width: 100%;
    padding: 11px 12px 11px 4px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--ink);
    line-height: 20px;
    text-align: left;
  }

  .compact .main {
    padding-block: 9px;
  }

  .dot {
    width: 7px;
    height: 7px;
    margin-top: 13px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0;
  }

  .unread .dot {
    opacity: 1;
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
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

  .more {
    font-size: var(--fs-meta);
    color: var(--ink-3);
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

  .pick {
    position: absolute;
    top: 11px;
    left: 22px;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: 50%;
    background: var(--paper);
    color: var(--ink-2);
    opacity: 0;
    transition:
      opacity var(--t-fast),
      background var(--t-fast);
  }

  .compact .pick {
    top: 9px;
  }

  .row:hover .pick,
  .pick:focus-visible,
  .pick[aria-pressed='true'] {
    opacity: 1;
  }

  .pick[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-ink);
  }

  .acts {
    position: absolute;
    top: 7px;
    right: 10px;
    display: flex;
    gap: 2px;
    padding: 3px;
    border-radius: var(--r-row);
    background: var(--raised);
    box-shadow: var(--shadow-sm);
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-fast);
  }

  .row:hover .acts,
  .row:focus-within .acts {
    opacity: 1;
    pointer-events: auto;
  }

  .row:hover .time,
  .row:focus-within .time {
    visibility: hidden;
  }
</style>
