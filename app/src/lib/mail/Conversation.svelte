<script lang="ts">
  // Konuşma görünümü: konuşmanın tüm iletileri alt alta. Eskiler tek satıra
  // katlanır; okunmamışlar ve en yeni ileti açık gelir. Gövdesi yerelde
  // olmayan ileti açılınca indirilir.
  import { untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { call, toAppError, type MessageDetail, type MessageId } from '../api';
  import { t, when } from '../i18n/index.svelte';
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import { showToast } from '../ui/toast.svelte';
  import { displayName } from '../util/format';
  import { mail, setUnread } from '../stores/mail.svelte';
  import MessageView from './MessageView.svelte';

  interface Props {
    /** Listede seçili ileti; konuşma onun hesabı ve kimliğiyle yüklenir. */
    selected: MessageDetail;
  }

  let { selected }: Props = $props();

  let items = $state<MessageDetail[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  const open = new SvelteSet<MessageId>();
  let token = 0;

  const threadKey = $derived(`${selected.accountId}|${selected.threadId}`);
  const allOpen = $derived(items.length > 0 && items.every((m) => open.has(m.id)));
  const mine = $derived(new Set(mail.accounts.map((a) => a.email.toLowerCase())));

  function label(m: MessageDetail): string {
    return mine.has(m.from.email.toLowerCase()) ? t('conv.me') : displayName(m.from);
  }

  // Konuşma değişince yeniden yüklenir (aynı konuşmada başka ileti seçilirse değil).
  $effect(() => {
    void threadKey;
    void load();
  });

  // Seçili ileti güncellenince (bayrak, uzak görsel izni) listedeki kopyası da güncellenir.
  $effect(() => {
    const s = selected;
    const list = untrack(() => items);
    if (list.some((m) => m.id === s.id && m !== s)) items = list.map((m) => (m.id === s.id ? s : m));
  });

  // Seçili ileti okundu sayılınca açık gelen diğer okunmamışlar da okundu sayılır.
  $effect(() => {
    if (selected.unread) return;
    untrack(markOpenUnread);
  });

  /** Açık gelen, seçili olmayan okunmamış iletileri okundu sayar. */
  function markOpenUnread() {
    const ids = items.filter((m) => m.unread && m.id !== selected.id && open.has(m.id)).map((m) => m.id);
    if (ids.length) markRead(ids);
  }

  function markRead(ids: MessageId[]) {
    const set = new Set(ids);
    items = items.map((m) => (set.has(m.id) ? { ...m, unread: false } : m));
    void setUnread(ids, false, { quiet: true });
  }

  /** Bir ileti değişince (yıldız, uzak içerik izni) listedeki kopyası güncellenir. */
  function update(d: MessageDetail) {
    items = items.map((m) => (m.id === d.id ? d : m));
  }

  async function load() {
    const my = ++token;
    const id = untrack(() => selected.id);
    loading = true;
    error = null;
    try {
      const list = await call('thread_get', { id });
      if (my !== token) return;
      items = list;
      const newest = list[list.length - 1]?.id;
      open.clear();
      for (const m of list) if (m.unread || m.id === newest || m.id === id) open.add(m.id);
      // Konuşma, seçili ileti okundu sayıldıktan sonra gelmiş olabilir; açık okunmamışlar burada işaretlenir.
      if (!untrack(() => selected.unread)) markOpenUnread();
    } catch (e) {
      if (my === token) error = toAppError(e).message;
    } finally {
      if (my === token) loading = false;
    }
  }

  async function fetchBody(id: MessageId) {
    try {
      const d = await call('message_get', { id });
      items = items.map((m) => (m.id === id ? d : m));
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  async function toggle(m: MessageDetail) {
    if (open.has(m.id)) {
      open.delete(m.id);
      return;
    }
    open.add(m.id);
    if (m.unread) markRead([m.id]);
    if (m.bodyText == null && m.bodyHtml == null) await fetchBody(m.id);
  }

  function toggleAll() {
    if (allOpen) {
      open.clear();
      // En yeni ileti her zaman okunur kalır.
      const newest = items[items.length - 1];
      if (newest) open.add(newest.id);
    } else {
      for (const m of items) {
        open.add(m.id);
        if (m.bodyText == null && m.bodyHtml == null) void fetchBody(m.id);
      }
      const unread = items.filter((m) => m.unread).map((m) => m.id);
      if (unread.length) markRead(unread);
    }
  }
</script>

<div class="conv">
  {#if error}
    <div class="state">
      <p class="meta">{t('conv.loadError', { message: error })}</p>
      <button class="btn-soft small" onclick={load}>{t('act.retry')}</button>
    </div>
  {:else if loading && !items.length}
    <p class="state meta">{t('conv.loading')}</p>
  {:else}
    <div class="conv-head" class:alone={items.length === 1}>
      <span class="meta num">{t('conv.count', { n: items.length })}</span>
      <span class="grow"></span>
      {#if items.length > 1}
        <button class="btn-ghost" onclick={toggleAll}>{allOpen ? t('conv.collapseAll') : t('conv.expandAll')}</button>
      {/if}
    </div>
    <ol class="msgs" aria-label={t('conv.list')}>
      {#each items as m (m.id)}
        <li class="msg" class:open={open.has(m.id)} class:current={m.id === selected.id}>
          {#if open.has(m.id)}
            <div class="opened">
              {#if items.length > 1}
                <button class="btn-icon small collapse" aria-expanded="true" aria-label={t('conv.collapse', { name: label(m) })} title={t('conv.collapse', { name: label(m) })} onclick={() => toggle(m)}>
                  <Icon name="chevronUp" size={16} />
                </button>
              {/if}
              <MessageView detail={m} actions onupdate={update} />
            </div>
          {:else}
            <button class="prior" aria-expanded="false" aria-label={t('conv.expand', { name: label(m) })} onclick={() => toggle(m)}>
              <Avatar address={m.from} size={28} />
              <span class="trunc"><span class="prior-name" class:unread={m.unread}>{label(m)}</span><span class="prior-text">{m.snippet}</span></span>
              {#if m.unread}<span class="dot" aria-hidden="true"></span><span class="sr-only">{t('conv.unread')}</span>{/if}
              {#if m.hasAttachments}<span class="clip"><Icon name="paperclip" size={14} /></span>{/if}
              <span class="meta num">{when(m.date)}</span>
            </button>
          {/if}
        </li>
      {/each}
    </ol>
  {/if}
</div>

<style>
  .conv {
    margin-top: 16px;
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
    padding: 16px 0;
  }

  .conv-head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
  }

  .msgs {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .msg {
    border-bottom: 1px solid var(--line);
  }

  .msg:last-child {
    border-bottom: 0;
  }

  .opened {
    position: relative;
    padding: 18px 0 24px;
  }

  .collapse {
    position: absolute;
    top: 14px;
    right: 0;
  }

  /* Açık iletide yıldız düğmesi daraltma düğmesine yer açar. */
  .opened :global(.sender) {
    padding-right: 32px;
  }

  .prior {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto auto auto;
    align-items: center;
    column-gap: 12px;
    width: 100%;
    padding: 10px 4px;
    border: 0;
    background: transparent;
    color: var(--ink);
    text-align: left;
    border-radius: var(--r-icon);
  }

  .prior:hover {
    background: var(--hover);
  }

  .prior-name {
    font-weight: 500;
  }

  .prior-name.unread {
    font-weight: 600;
  }

  .prior-text {
    margin-left: 8px;
    color: var(--ink-3);
  }

  .prior:hover .prior-text {
    color: var(--ink-2);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }

  .clip {
    display: inline-flex;
    color: var(--ink-3);
  }
</style>
