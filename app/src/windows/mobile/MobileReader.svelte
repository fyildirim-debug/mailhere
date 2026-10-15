<script lang="ts">
  // Mobil okuma ekranı: uygulama çubuğu (geri; yıldız, okunmadı, diğer
  // eylemler), konu 19 px, konuşma ya da tek ileti (MessageView), üstte hızlı
  // yanıt, altta 56 px eylem çubuğu. Menüler alt sayfada açılır (Menu / Sheet).
  //
  // Listeye dönüş: postayı listeden çıkaran eylemler (arşivle, sil, taşı,
  // ertele, elek kararı) ve "okunmadı say" ekranı kapatır; masaüstü sıradaki
  // postaya geçerken telefonda liste ana yerdir ve geri alma toast'u orada
  // görünür. Yıldız, sabitleme, etiket ve hatırlatma ekranda kalır.
  //
  // Komşu postaya geçiş: içerik üzerinde yatay kaydırma (yalnız transform)
  // listedeki önceki / sonraki satırı aynı ekranda açar (nav.replace).
  import { untrack } from 'svelte';
  import { call, toAppError, type MessageId } from '../../lib/api';
  import { t, when } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import Conversation from '../../lib/mail/Conversation.svelte';
  import FollowUpMenu from '../../lib/mail/FollowUpMenu.svelte';
  import MessageView from '../../lib/mail/MessageView.svelte';
  import QuickReply from '../../lib/mail/QuickReply.svelte';
  import ReaderNotes from '../../lib/mail/ReaderNotes.svelte';
  import SnoozeMenu from '../../lib/mail/SnoozeMenu.svelte';
  import { flatten, folderLabel, folderTree } from '../../lib/mail/folders';
  import { orderRows } from '../../lib/mail/listing';
  import { openCompose } from '../../lib/mail/open';
  import { displayName } from '../../lib/util/format';
  import {
    blockSender,
    mail,
    moveTo,
    moveToFolder,
    newRuleFrom,
    openSettings,
    rowsOf,
    select,
    setFollowUp,
    setUnread,
    snooze,
    toggleStar,
    unsubscribe,
    type Row,
  } from '../../lib/stores/mail.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import { bulkMove, senderMessages, setTag, togglePin } from '../../lib/stores/bulk.svelte';
  import { tagStore } from '../../lib/stores/tags.svelte';
  import AppBar from './AppBar.svelte';
  import { nav } from './nav.svelte';

  interface Props {
    id: MessageId;
  }

  let { id }: Props = $props();

  // Ekranın postası kimlik değişince yüklenir (bildirimden açılış, komşuya
  // kaydırma). Liste satırından açılışta seçim zaten yapılmıştır.
  $effect(() => {
    const want = id;
    untrack(() => {
      if (mail.selectedId !== want) void select(want);
    });
  });

  /** Yalnız bu ekranın postası; arşivden sonra sıradakinin yüklenmesi burada görünmez. */
  const d = $derived(mail.detail?.id === id ? mail.detail : null);
  const account = $derived(d ? mail.accounts.find((a) => a.id === d.accountId) : undefined);
  const role = $derived(d ? mail.mailboxes.find((b) => b.id === d.mailboxId)?.role : undefined);
  const isDraft = $derived(role === 'drafts');
  const isSent = $derived(role === 'sent');
  const conversation = $derived(settings.value.conversationView && !isDraft);
  const screening = $derived(mail.view.kind === 'screener' && mail.search == null);
  const senderScope = $derived(mail.search == null && (mail.view.kind === 'inbox' || mail.view.kind === 'mailbox' || mail.view.kind === 'tag'));
  const error = $derived(!d && !mail.detailLoading ? mail.detailError : null);

  let quick = $state<QuickReply>();
  let menu = $state<{ title?: string; items: MenuItem[] } | null>(null);
  let snoozeOpen = $state(false);
  let followOpen = $state(false);

  /** Bu postanın satırı (konuşmanın tüm iletileri); liste eylemleriyle aynı kapsam. */
  function ids(): MessageId[] {
    return rowsOf(mail.items, mail.search == null).find((r) => r.ids.includes(id))?.ids ?? [id];
  }

  function leave() {
    nav.pop();
  }

  // ------------------------------------------------------------------ eylemler

  function archive() {
    void moveTo(ids(), 'archive');
    leave();
  }

  function trash() {
    void moveTo(ids(), 'trash');
    leave();
  }

  function markUnread() {
    void setUnread([id], true);
    leave();
  }

  function reply() {
    if (isDraft) editDraft();
    else quick?.focus();
  }

  function editDraft() {
    openCompose({ mode: 'draft', messageId: id });
  }

  function compose(mode: 'replyAll' | 'forward') {
    openCompose({ mode, messageId: id });
  }

  async function saveEml() {
    try {
      const path = await call('message_save_eml', { id });
      if (path) showToast({ text: t('print.saved', { path }) });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  function moveMenu() {
    if (!d) return;
    const here = d.mailboxId;
    const nodes = flatten(folderTree(mail.mailboxes, d.accountId, true)).filter((n) => n.mailbox.id !== here && n.mailbox.role !== 'drafts');
    menu = {
      title: t('read.moveTo'),
      items: nodes.map((n) => ({
        key: n.mailbox.id,
        label: `${'  '.repeat(n.depth)}${folderLabel(n.mailbox)}`,
        icon: 'folder' as const,
        onclick: () => {
          void moveToFolder(ids(), n.mailbox, folderLabel(n.mailbox));
          leave();
        },
      })),
    };
  }

  function tagMenu() {
    if (!d) return;
    const msg = d;
    const items: MenuItem[] = tagStore.list.map((tg) => {
      const has = msg.tags.includes(tg.id);
      return { key: tg.id, label: tg.name, checked: has, onclick: () => void setTag(ids(), tg, !has) };
    });
    items.push({ label: t('tag.manage'), icon: 'settings', onclick: () => openSettings('tags') });
    menu = { title: t('tag.menu'), items };
  }

  /**
   * Göndericiye göre toplu temizlik: kapsamdaki postalar sayılırken alt sayfa
   * açık kalır (aynı sayfa içerik değiştirir), sonra sayı ve onay gelir.
   */
  async function senderAction(mode: 'archive' | 'trash' | 'keepLatest') {
    if (!d) return;
    const msg = d;
    menu = { items: [{ label: t('mob.read.counting'), disabled: true, onclick: () => {} }] };
    let list: MessageId[];
    try {
      list = await senderMessages(msg.accountId, msg.from.email);
    } catch (e) {
      menu = null;
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
      return;
    }
    // En yenisini tut: liste yeniden eskiye gelir, ilki kalır.
    if (mode === 'keepLatest') list = list.slice(1);
    if (!list.length) {
      menu = null;
      showToast({ text: t(mode === 'keepLatest' ? 'bulk.senderOnlyLatest' : 'bulk.senderNone') });
      return;
    }
    const target = mode === 'trash' ? 'trash' : 'archive';
    menu = {
      title: t(target === 'trash' ? 'bulk.confirmTrash' : 'bulk.confirmArchive', { n: list.length }),
      items: [
        {
          label: t('bulk.confirmGo'),
          icon: target === 'trash' ? 'trash' : 'archive',
          danger: target === 'trash',
          onclick: () => {
            void bulkMove(list, target);
            if (list.includes(id)) leave();
          },
        },
        { label: t('act.cancel'), icon: 'x', onclick: () => {} },
      ],
    };
  }

  function moreMenu() {
    if (!d) return;
    const msg = d;
    const sender = msg.from.email;
    menu = {
      items: [
        { label: t('read.moveTo'), icon: 'folder', onclick: moveMenu },
        { label: t('tag.menu'), icon: 'tag', onclick: tagMenu },
        { label: msg.pinned ? t('pin.unpin') : t('pin.pin'), icon: 'pin', onclick: () => void togglePin(ids()) },
        { label: t('act.snooze'), icon: 'clock', onclick: () => (snoozeOpen = true) },
        // Gönderilmiş posta: yanıt gelmezse hatırlat.
        ...(isSent ? [{ label: t('follow.menu'), icon: 'followUp' as const, onclick: () => (followOpen = true) }] : []),
        { label: t('print.saveEml'), icon: 'download', onclick: () => void saveEml() },
        ...(msg.unsubscribe ? [{ label: t('read.unsubscribe'), icon: 'unsubscribe' as const, onclick: () => void unsubscribe(msg) }] : []),
        ...(senderScope
          ? [
              { label: t('bulk.senderArchive'), icon: 'archive' as const, onclick: () => void senderAction('archive') },
              { label: t('bulk.senderKeepLatest'), icon: 'archive' as const, onclick: () => void senderAction('keepLatest') },
              { label: t('bulk.senderTrash'), icon: 'trash' as const, onclick: () => void senderAction('trash') },
            ]
          : []),
        {
          label: t('read.ruleFromSender'),
          icon: 'rules',
          onclick: () =>
            newRuleFrom({
              id: '',
              name: t('read.ruleName', { name: displayName(msg.from) }),
              enabled: true,
              accountId: msg.accountId,
              match: 'all',
              conditions: [{ kind: 'from', op: 'is', value: sender }],
              actions: [{ kind: 'archive' }],
              stop: false,
            }),
        },
        { label: t('read.block', { email: sender }), icon: 'block', danger: true, onclick: () => void blockSender(msg.accountId, sender) },
      ],
    };
  }

  // ------------------------------------------------ komşu postaya kaydırma

  const neighbors = $derived.by(() => {
    const rows = orderRows(rowsOf(mail.items, mail.search == null), { view: mail.view, search: mail.search, now: Date.now() });
    const i = rows.findIndex((r) => r.ids.includes(id));
    return { prev: i > 0 ? rows[i - 1] : null, next: i >= 0 && i < rows.length - 1 ? rows[i + 1] : null };
  });

  const SLOP = 24;
  const COMMIT = 96;
  let shift = $state(0);
  let dragging = $state(false);
  /** Yeni içeriğin geldiği yön (sınıf); animasyon bitince sıfırlanır. */
  let slide = $state<'left' | 'right' | null>(null);
  let drag: { x: number; y: number; id: number } | null = null;
  let scroller = $state<HTMLDivElement>();

  function goTo(row: Row | null, dir: 'left' | 'right') {
    if (!row) return;
    slide = dir;
    nav.replace({ name: 'read', id: row.id });
    if (scroller) scroller.scrollTop = 0;
  }

  function down(e: PointerEvent) {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    drag = { x: e.clientX, y: e.clientY, id: e.pointerId };
    dragging = false;
  }

  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (!dragging) {
      if (Math.abs(dx) < SLOP && Math.abs(dy) < SLOP) return;
      if (Math.abs(dx) < Math.abs(dy) * 1.5) {
        drag = null;
        return;
      }
      dragging = true;
      scroller?.setPointerCapture(e.pointerId);
    }
    // Komşu yoksa direnç: yarı mesafe.
    const free = dx < 0 ? neighbors.next : neighbors.prev;
    shift = free ? dx : dx / 3;
  }

  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    drag = null;
    if (!dragging) return;
    dragging = false;
    const dx = shift;
    shift = 0;
    if (e.type === 'pointercancel' || Math.abs(dx) < COMMIT) return;
    if (dx < 0) goTo(neighbors.next, 'left');
    else goTo(neighbors.prev, 'right');
  }
</script>

<AppBar leading="back" onleading={() => nav.back()}>
  {#snippet actions()}
    {#if d}
      <button
        class="btn-icon"
        class:star={d.starred}
        aria-label={d.starred ? t('act.unstar') : t('act.star')}
        aria-pressed={d.starred}
        title={d.starred ? t('act.unstar') : t('act.star')}
        onclick={() => void toggleStar(d.id)}
      >
        <Icon name="star" size={22} filled={d.starred} />
      </button>
      <button class="btn-icon" aria-label={t('act.markUnread')} title={t('act.markUnread')} onclick={markUnread}>
        <Icon name="mail" size={22} />
      </button>
      <button class="btn-icon" aria-label={t('read.more')} title={t('read.more')} aria-haspopup="menu" onclick={moreMenu}>
        <Icon name="more" size={22} />
      </button>
    {/if}
  {/snippet}
</AppBar>

<div
  class="scroll"
  role="presentation"
  bind:this={scroller}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
>
  {#if d}
    <article
      class="article"
      class:dragging
      class:from-left={slide === 'right'}
      class:from-right={slide === 'left'}
      style:transform={shift ? `translateX(${shift}px)` : undefined}
      onanimationend={() => (slide = null)}
    >
      <div class="subject-row">
        <h2 class="selectable">{d.subject || '—'}</h2>
        {#if account && mail.accounts.length > 1}<span class="tag">{account.name}</span>{/if}
      </div>
      <ReaderNotes detail={d} {screening} onnavigate={leave} onscreened={leave} />

      {#if conversation}
        {#key `${d.accountId}|${d.threadId}`}
          <Conversation selected={d} />
        {/key}
      {:else}
        {#if d.thread.length}
          <div class="meta thread-meta">{t('read.thread', { n: d.thread.length + 1 })}</div>
          <div class="thread">
            {#each d.thread as p (p.id)}
              <button class="prior" onclick={() => nav.replace({ name: 'read', id: p.id })}>
                <Avatar address={p.from} size={28} />
                <span class="trunc"><span class="prior-name">{displayName(p.from)}</span><span class="prior-text">{p.snippet}</span></span>
                <span class="meta num">{when(p.date)}</span>
              </button>
            {/each}
          </div>
        {/if}
        <div class="single">
          <MessageView detail={d} />
        </div>
      {/if}
    </article>
  {:else if error}
    <div class="m-empty">
      <span class="m-empty-icon"><Icon name="mail" size={28} /></span>
      <div class="m-empty-title">{t('read.loadError')}</div>
      <div class="m-empty-body">{error}</div>
      <button class="btn-soft" onclick={() => void select(id)}>{t('act.retry')}</button>
    </div>
  {:else}
    <div class="m-empty"><div class="m-empty-body">{t('read.loading')}</div></div>
  {/if}
</div>

{#if d && !isDraft}
  <div class="reply-wrap">
    <QuickReply bind:this={quick} detail={d} compact />
  </div>
{/if}

<div class="bar" role="toolbar" aria-label={t('mob.read.actions')}>
  {#if isDraft}
    <button class="act" disabled={!d} onclick={editDraft}>
      <Icon name="pen" size={22} /><span>{t('mob.read.editDraft')}</span>
    </button>
    <button class="act" disabled={!d} onclick={trash}>
      <Icon name="trash" size={22} /><span>{t('act.delete')}</span>
    </button>
  {:else}
    <button class="act" disabled={!d} onclick={reply}>
      <Icon name="reply" size={22} /><span>{t('act.reply')}</span>
    </button>
    <button class="act" disabled={!d} title={t('act.replyAllLong')} onclick={() => compose('replyAll')}>
      <Icon name="replyAll" size={22} /><span>{t('act.replyAll')}</span>
    </button>
    <button class="act" disabled={!d} onclick={() => compose('forward')}>
      <Icon name="forward" size={22} /><span>{t('act.forward')}</span>
    </button>
    <button class="act" disabled={!d} onclick={archive}>
      <Icon name="archive" size={22} /><span>{t('act.archive')}</span>
    </button>
    <button class="act" disabled={!d} onclick={trash}>
      <Icon name="trash" size={22} /><span>{t('act.delete')}</span>
    </button>
  {/if}
</div>

{#if menu}<Menu title={menu.title} label={menu.title ?? t('read.more')} items={menu.items} onclose={() => (menu = null)} />{/if}
{#if snoozeOpen}
  <SnoozeMenu
    onclose={() => (snoozeOpen = false)}
    onpick={(at) => {
      snoozeOpen = false;
      void snooze(ids(), at);
      leave();
    }}
  />
{/if}
{#if followOpen && d}
  <FollowUpMenu
    current={d.followUp}
    onpick={(at) => {
      followOpen = false;
      void setFollowUp(id, at);
    }}
    onclear={d.followUp != null
      ? () => {
          followOpen = false;
          void setFollowUp(id, null);
        }
      : undefined}
    onclose={() => (followOpen = false)}
  />
{/if}

<style>
  .star {
    color: var(--amber);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    touch-action: pan-y;
    background: var(--paper);
  }

  .article {
    padding: 14px var(--m-gutter) 28px;
    transition: transform var(--t-state) var(--ease);
  }

  .article.dragging {
    transition: none;
  }

  .article.from-right {
    animation: slide-right var(--t-state) var(--ease);
  }

  .article.from-left {
    animation: slide-left var(--t-state) var(--ease);
  }

  @keyframes slide-right {
    from {
      opacity: 0;
      transform: translateX(28px);
    }
  }

  @keyframes slide-left {
    from {
      opacity: 0;
      transform: translateX(-28px);
    }
  }

  .subject-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  h2 {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-section);
    line-height: 1.3;
    font-weight: 600;
    letter-spacing: -0.015em;
    text-wrap: balance;
    overflow-wrap: anywhere;
  }

  .subject-row .tag {
    flex: none;
    margin-top: 3px;
  }

  .thread-meta {
    margin-top: 6px;
    font-weight: 500;
  }

  .thread {
    margin-top: 16px;
    border-top: 1px solid var(--line);
  }

  .prior {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 12px;
    width: 100%;
    min-height: var(--m-touch);
    padding: 8px 4px;
    border: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
    color: var(--ink);
    text-align: left;
  }

  .prior-name {
    font-weight: 500;
  }

  .prior-text {
    margin-left: 8px;
    color: var(--ink-3);
  }

  .single {
    margin-top: 10px;
  }

  /* Konuşma ve ileti alt öğeleri dar ekrana: ekler tam genişlik, dokunma hedefleri 44. */
  .article :global(.att) {
    width: 100%;
    min-height: var(--m-touch);
  }

  /* Gönderen satırı: üstte ad, altında "tarih · kime" (tarih kesilmez). */
  .article :global(.sender) {
    flex-wrap: wrap;
    column-gap: 0;
    row-gap: 0;
  }

  .article :global(.sender .who-btn) {
    flex: 1 1 100%;
  }

  .article :global(.sender > .meta) {
    display: none;
  }

  .article :global(.sender .when-inline) {
    display: inline;
  }

  /* Konuşmadaki eski bir ileti açılınca kendi eylemleri ikinci sırada, ad
     sütununa hizalı: metin 52 px'te, simge düğme kutusunda 12 px içeride. */
  .article :global(.msg:not(.current) .sender > .meta + .btn-icon) {
    margin-left: 40px;
  }

  /* Açık ileti (alt çubuğun ve üst çubuğun işlediği): yanıtla, ilet ve yıldız
     çubuklarda, abonelikten çık "Diğer eylemler"de. Gönderen tek satır, yalnız
     "Özgün renkler" adın yanında kalır; posta ekranın üstünden başlar. */
  .article :global(:is(.msg.current, .single) .sender .msg-act),
  .article :global(:is(.msg.current, .single) .unsub) {
    display: none;
  }

  .article :global(:is(.msg.current, .single) .sender .who-btn) {
    flex: 1 1 0;
    min-width: 0;
  }

  /* Tek iletili konuşmada "1 ileti" başlığı yinelemedir; daraltma düğmesi de
     olmadığından gönderen satırı sağ kenara kadar uzanır. */
  .article :global(.conv-head.alone) {
    display: none;
  }

  .article :global(.conv-head.alone + .msgs .sender) {
    padding-right: 0;
  }

  .article :global(.conv) {
    margin-top: 4px;
  }

  .article :global(.msg .opened) {
    padding: 12px 0 20px;
  }

  /* Şeritler tek satır: kısa metin, gerekirse kısalır; eylem 44 px hedef. */
  .article :global(.unsub),
  .article :global(.remote) {
    margin-top: 10px;
    padding: 0 4px 0 12px;
  }

  .article :global(.unsub + .remote) {
    margin-top: 6px;
  }

  .article :global(.remote-long) {
    display: none;
  }

  .article :global(.remote-short) {
    display: inline;
  }

  .article :global(.remote-text) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .article :global(.unsub .link),
  .article :global(.remote .link) {
    flex: none;
    min-height: var(--m-touch);
    padding: 0 8px;
  }

  .article :global(.body) {
    margin-top: 10px;
  }

  .reply-wrap {
    flex: none;
    padding: 8px 12px 10px;
    background: var(--paper);
    border-top: 1px solid var(--line);
  }

  /* Alt sayfa (Sheet) listesi ve kişi kartı okuma ekranında menü olarak açılır. */

  .bar {
    position: relative;
    z-index: 6;
    flex: none;
    display: flex;
    align-items: stretch;
    height: calc(var(--m-bar) + var(--sab));
    padding: 0 calc(4px + var(--sar)) var(--sab) calc(4px + var(--sal));
    background: var(--raised);
    border-top: 1px solid var(--line);
  }

  .act {
    flex: 1;
    min-width: var(--m-touch);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    padding: 4px 0;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink-2);
    font-size: var(--fs-meta);
    font-weight: 500;
    white-space: nowrap;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .act:active {
    background: var(--hover);
    color: var(--ink);
  }

  .act:disabled {
    opacity: 0.45;
    background: transparent;
  }

  @media (prefers-reduced-motion: reduce) {
    .article,
    .article.from-right,
    .article.from-left {
      transition: none;
      animation: none;
    }
  }
</style>
