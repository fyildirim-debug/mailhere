<script lang="ts">
  // Planlanmış gönderimler: giden kutusunda bekleyen postalar. Masaüstünde
  // ana pencerenin sağ bölgesi; mobilde (`embedded`) uygulama çubuğu başlığı
  // taşıdığı için başlık ve kenar çizgisi yoktur, satırlar tek sütundur.
  import { onMount } from 'svelte';
  import { call, EVENTS, on, toAppError, type ScheduledItem } from '../../lib/api';
  import { t, when } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { displayName } from '../../lib/util/format';
  import { mail, refreshCounts } from '../../lib/stores/mail.svelte';

  interface Props {
    /** Mobil kabukta itilen ekranın içinde: başlık dışarıda, tek sütun. */
    embedded?: boolean;
  }

  let { embedded = false }: Props = $props();

  let items = $state<ScheduledItem[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);

  async function load() {
    try {
      items = await call('outbox_list');
      error = null;
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      loaded = true;
    }
  }

  onMount(() => {
    void load();
    const offs = [on(EVENTS.outbox, () => void load()), on(EVENTS.changed, () => void load())];
    return () => offs.forEach((off) => off());
  });

  async function act(item: ScheduledItem, kind: 'now' | 'edit' | 'cancel') {
    if (busy) return;
    busy = item.id;
    try {
      if (kind === 'now') {
        await call('outbox_send_now', { id: item.id });
        showToast({ text: t('sched.sending') });
      } else if (kind === 'edit') {
        const draft = await call('outbox_to_draft', { id: item.id });
        await call('open_compose', { prefill: { mode: 'draft', messageId: draft } });
      } else if (await call('send_cancel', { id: item.id })) {
        showToast({ text: t('sched.cancelled') });
      }
      await load();
      void refreshCounts();
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
      await load();
    } finally {
      busy = null;
    }
  }

  function account(id: string) {
    return mail.accounts.find((a) => a.id === id);
  }

  function recipients(item: ScheduledItem): string {
    const [first, ...rest] = item.to;
    if (!first) return '—';
    return rest.length ? t('sched.andMore', { name: displayName(first), n: rest.length }) : displayName(first);
  }
</script>

<section class="sched" class:embedded aria-labelledby={embedded ? undefined : 'sched-title'} aria-label={embedded ? t('nav.scheduled') : undefined}>
  <div class="inner">
    <header class="head">
      {#if !embedded}<h1 id="sched-title">{t('nav.scheduled')}</h1>{/if}
      <span class="meta">{t('sched.lead')}</span>
    </header>

    {#if error}<p class="set-error" role="alert">{t('sched.loadFailed', { message: error })}</p>{/if}

    {#if items.length}
      <ul class="list">
        {#each items as item (item.id)}
          {@const a = account(item.accountId)}
          <li class="item">
            <div class="when">
              <Icon name="calendarClock" size={18} />
              <span class="num">{when(item.sendAt)}</span>
            </div>
            <div class="text">
              <span class="trunc subject">{item.subject || t('compose.noSubject')}</span>
              <span class="trunc meta">
                {#if a}<span class="dot" style:background={a.color}></span>{`${a.name} · `}{/if}{t('sched.to', { list: recipients(item) })}
              </span>
              {#if item.error}<span class="err" role="alert">{t('sched.failed', { message: item.error })}</span>{/if}
            </div>
            <div class="tools">
              <button class="btn-ghost" disabled={busy === item.id} onclick={() => act(item, 'edit')}>{t('sched.edit')}</button>
              <button class="btn-ghost" disabled={busy === item.id} onclick={() => act(item, 'cancel')}>{t('act.cancel')}</button>
              <button class="btn-soft small" disabled={busy === item.id} onclick={() => act(item, 'now')}><Icon name="send" size={14} />{t('sched.now')}</button>
            </div>
          </li>
        {/each}
      </ul>
    {:else if loaded && !error}
      <div class="empty quiet">
        <Icon name="calendarClock" size={28} />
        <span class="empty-title">{t('sched.emptyTitle')}</span>
        <span class="meta">{t('sched.emptyBody')}</span>
      </div>
    {/if}
  </div>
</section>

<style>
  .sched {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    background: var(--paper);
    border-left: 1px solid var(--line);
  }

  .inner {
    max-width: 820px;
    margin: 0 auto;
    padding: 32px 40px 56px;
    animation: fade-up var(--t-state) ease-out;
  }

  .head {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 18px;
  }

  h1 {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--line);
  }

  .item {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr) auto;
    align-items: center;
    gap: 16px;
    min-height: 72px;
    padding: 10px 4px;
    border-bottom: 1px solid var(--line);
  }

  .when {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--accent-text);
    font-weight: 500;
  }

  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .subject {
    font-weight: 500;
  }

  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 6px;
    border-radius: 50%;
    vertical-align: 1px;
  }

  .err {
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  /* Sessiz boş durum (global.css `.empty.quiet`); simge vurgu renginde. */
  .empty {
    gap: 10px;
    padding: 72px 24px;
    color: var(--accent);
  }

  @media (max-width: 980px) {
    .item {
      grid-template-columns: minmax(0, 1fr);
      gap: 8px;
    }
  }

  .embedded {
    border-left: 0;
  }

  .embedded .inner {
    padding: 12px 16px calc(24px + env(safe-area-inset-bottom, 0px));
    animation: none;
  }

  .embedded .head {
    margin-bottom: 12px;
  }

  .embedded .item {
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
    padding: 12px 0;
  }

  .embedded .tools {
    flex-wrap: wrap;
    justify-content: flex-end;
  }
</style>
