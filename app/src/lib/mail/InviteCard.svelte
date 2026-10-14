<script lang="ts">
  // Takvim daveti kartı: etkinlik bilgisi, yanıt düğmeleri ve "Takvime ekle".
  // Yanıt düzenleyene postayı alan hesaptan gider (arka uç kurar ve gönderir).
  import { call, toAppError, type InviteResponse, type MessageDetail } from '../api';
  import { locale, t, type MessageKey } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';
  import { showToast } from '../ui/toast.svelte';
  import { displayName } from '../util/format';
  import { inviteWhen } from './invite';

  interface Props {
    detail: MessageDetail;
  }

  let { detail: d }: Props = $props();

  // Kart yalnız daveti olan postada çizilir.
  const inv = $derived(d.invite!);
  const cancelled = $derived(inv.method === 'cancel');
  const responses: [InviteResponse, MessageKey][] = [
    ['accepted', 'ics.accept'],
    ['tentative', 'ics.tentative'],
    ['declined', 'ics.decline'],
  ];

  let busy = $state<InviteResponse | null>(null);
  let replied = $state<InviteResponse | null>(null);

  async function respond(r: InviteResponse) {
    if (busy) return;
    busy = r;
    try {
      await call('invite_reply', { id: d.id, response: r });
      replied = r;
      showToast({ text: t(`ics.replied.${r}`) });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    } finally {
      busy = null;
    }
  }

  async function addToCalendar() {
    try {
      await call('invite_open', { id: d.id });
      showToast({ text: t('ics.opened') });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }
</script>

<section class="invite" class:cancelled aria-label={t('ics.title')}>
  <div class="kicker"><Icon name="calendar" size={16} /><span>{t('ics.title')}</span></div>
  <h3 class="title">{inv.summary || t('ics.untitled')}</h3>
  <p class="when">{inviteWhen(inv, locale(), t('ics.allDay'))}</p>
  {#if inv.location}
    <p class="row"><Icon name="mapPin" size={14} /><span class="trunc">{inv.location}</span></p>
  {/if}
  <p class="row">
    {#if inv.organizer}<Icon name="user" size={14} /><span class="trunc">{t('ics.organizer', { name: displayName(inv.organizer) })}</span><span class="dot">·</span>{/if}
    <Icon name="users" size={14} /><span class="num">{t('ics.attendees', { n: inv.attendees })}</span>
  </p>
  {#if cancelled}
    <p class="cancel-note">{t('ics.cancelled')}</p>
  {:else}
    <div class="actions">
      {#each responses as [r, key] (r)}
        <button class="btn-soft small" class:chosen={replied === r} disabled={busy != null} aria-pressed={replied === r} onclick={() => respond(r)}>
          {#if replied === r}<Icon name="check" size={16} />{/if}{busy === r ? t('ics.sending') : t(key)}
        </button>
      {/each}
      <span class="grow"></span>
      <button class="link small" onclick={addToCalendar}>{t('ics.addToCalendar')}</button>
    </div>
    <p class="note">{inv.organizer ? t('ics.replyNote', { email: inv.organizer.email }) : t('ics.noOrganizer')}</p>
  {/if}
</section>

<style>
  .invite {
    margin-top: 18px;
    padding: 14px 16px 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    background: var(--surface);
  }

  .kicker {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--accent-text);
  }

  .cancelled .kicker {
    color: var(--ink-3);
  }

  .title {
    margin-top: 6px;
    font-size: var(--fs-section);
    font-weight: 600;
    line-height: 1.3;
    letter-spacing: -0.015em;
    text-wrap: balance;
  }

  .cancelled .title {
    text-decoration: line-through;
    color: var(--ink-2);
  }

  .when {
    margin-top: 4px;
    font-weight: 500;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
    min-width: 0;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .dot {
    margin: 0 2px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }

  .chosen {
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .grow {
    flex: 1;
  }

  .link.small {
    font-size: var(--fs-meta);
  }

  .note,
  .cancel-note {
    margin-top: 10px;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .cancel-note {
    color: var(--ink-2);
    font-weight: 500;
  }
</style>
