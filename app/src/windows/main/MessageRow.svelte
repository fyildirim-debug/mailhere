<script lang="ts">
  import { call } from '../../lib/api';
  import { locale, t } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { displayName, listTime, snoozeOptions } from '../../lib/util/format';
  import { mail, moveTo, select, snooze, type Row } from '../../lib/stores/mail.svelte';

  interface Props {
    message: Row;
    now: number;
    compact: boolean;
  }

  let { message, now, compact }: Props = $props();

  const selected = $derived(mail.selectedId != null && message.ids.includes(mail.selectedId));
  const tomorrow = $derived(snoozeOptions(now).find((o) => o.key === 'tomorrow')!.at);
</script>

<div class="row" class:selected class:unread={message.unread} class:compact data-id={message.id}>
  <button
    class="main"
    aria-current={selected ? 'true' : undefined}
    onclick={() => select(message.id)}
    ondblclick={() => call('open_quick', { id: message.id })}
  >
    <span class="dot" aria-hidden="true"></span>
    <Avatar address={message.from} />
    <span class="text">
      <span class="line1">
        <span class="sender trunc">{displayName(message.from)}</span>
        {#if message.count > 1}<span class="count num">{message.count}</span>{/if}
        {#if message.hasAttachments}<Icon name="paperclip" size={14} label={t('list.attachment')} class="quiet" />{/if}
        {#if message.starred}<span class="star"><Icon name="star" size={14} filled label={t('list.starred')} /></span>{/if}
        <span class="time num">{listTime(message.date, now, locale())}</span>
      </span>
      <span class="subject trunc">{message.subject || '—'}</span>
      {#if !compact}<span class="snippet trunc">{message.snippet}</span>{/if}
    </span>
  </button>
  <div class="acts">
    <button class="btn-icon small" aria-label={t('act.archive')} title="{t('act.archive')} · E" onclick={() => moveTo(message.ids, 'archive')}>
      <Icon name="archive" size={16} />
    </button>
    <button class="btn-icon small" aria-label={t('act.snooze')} title="{t('snooze.tomorrow')} · H" onclick={() => snooze(message.ids, tomorrow)}>
      <Icon name="clock" size={16} />
    </button>
    <button class="btn-icon small" aria-label={t('act.delete')} title="{t('act.delete')} · Del" onclick={() => moveTo(message.ids, 'trash')}>
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

  .row.selected {
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
