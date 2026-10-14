<script lang="ts">
  import { call, toAppError } from '../../lib/api';
  import { locale, t, when } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { flatten, folderLabel, folderTree } from '../../lib/mail/folders';
  import Conversation from '../../lib/mail/Conversation.svelte';
  import MessageView from '../../lib/mail/MessageView.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import SnoozeMenu from '../../lib/mail/SnoozeMenu.svelte';
  import { printDocument, printInFrame } from '../../lib/mail/print';
  import FollowUpMenu from '../../lib/mail/FollowUpMenu.svelte';
  import QuickReply from '../../lib/mail/QuickReply.svelte';
  import ReaderNotes from '../../lib/mail/ReaderNotes.svelte';
  import { openCompose } from '../../lib/mail/open';
  import { displayName, fileSize } from '../../lib/util/format';
  import {
    blockSender,
    mail,
    moveTo,
    moveToFolder,
    newRuleFrom,
    openSettings,
    select,
    selectedIds,
    setFollowUp,
    setUnread,
    snooze,
    unsubscribe,
  } from '../../lib/stores/mail.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import { bulkMove, senderMessages, setTag, togglePin } from '../../lib/stores/bulk.svelte';
  import { tagStore, tagsOf } from '../../lib/stores/tags.svelte';

  let snoozeOpen = $state(false);
  /** Hızlı yanıt kutusu (odak için). */
  let quick = $state<QuickReply>();
  /** "Yanıt gelmezse hatırlat" menüsünün konumu (açıkken). */
  let followAt = $state<{ x: number; y: number } | null>(null);

  const d = $derived(mail.detail);
  const account = $derived(d ? mail.accounts.find((a) => a.id === d.accountId) : undefined);
  const isDraft = $derived(d ? mail.mailboxes.find((b) => b.id === d.mailboxId)?.role === 'drafts' : false);
  /** Konuşma görünümü: ayar açık ve taslak değil. */
  const conversation = $derived(settings.value.conversationView && !isDraft);
  const isSent = $derived(d ? mail.mailboxes.find((b) => b.id === d.mailboxId)?.role === 'sent' : false);
  /** Elek görünümünde: gönderici için kabul / engelle kararı. */
  const screening = $derived(mail.view.kind === 'screener' && mail.search == null);
  /** Göndericiye göre temizlik yalnız arka ucun kapsadığı görünümlerde: gelen kutusu, klasör, etiket (arama dışı). */
  const senderScope = $derived(mail.search == null && (mail.view.kind === 'inbox' || mail.view.kind === 'mailbox' || mail.view.kind === 'tag'));
  const tags = $derived(d ? tagsOf(d.tags) : []);

  export function focusReply() {
    if (isDraft) editDraft();
    else quick?.focus();
  }

  function editDraft() {
    if (d) openCompose({ mode: 'draft', messageId: d.id });
  }

  export function openSnooze() {
    snoozeOpen = true;
  }

  /** Başlıklar ve gövde yalıtılmış bir çerçevede yazdırılır; arayüz basılmaz. */
  async function print() {
    if (!d) return;
    const doc = printDocument({
      detail: d,
      labels: { from: t('print.from'), to: t('print.to'), cc: t('print.cc'), date: t('print.date'), attachments: t('print.attachments') },
      when: when(d.date),
      fileSize: (n) => fileSize(n, locale()),
    });
    try {
      await printInFrame(doc);
    } catch {
      showToast({ text: t('print.failed') });
    }
  }

  async function saveEml() {
    if (!d) return;
    try {
      const path = await call('message_save_eml', { id: d.id });
      if (path) showToast({ text: t('print.saved', { path }) });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  let menu = $state<{ at: { x: number; y: number }; title?: string; items: MenuItem[] } | null>(null);

  function anchorOf(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.right - 260, y: r.bottom + 4 };
  }

  function moveMenu(e: MouseEvent) {
    if (!d) return;
    const here = d.mailboxId;
    const nodes = flatten(folderTree(mail.mailboxes, d.accountId, true)).filter((n) => n.mailbox.id !== here && n.mailbox.role !== 'drafts');
    menu = {
      at: anchorOf(e),
      title: t('read.moveTo'),
      items: nodes.map((n) => ({
        key: n.mailbox.id,
        label: `${'  '.repeat(n.depth)}${folderLabel(n.mailbox)}`,
        icon: 'folder' as const,
        onclick: () => void moveToFolder(selectedIds(), n.mailbox, folderLabel(n.mailbox)),
      })),
    };
  }

  function tagMenu(e: MouseEvent) {
    if (!d) return;
    const msg = d;
    const items: MenuItem[] = tagStore.list.map((tg) => {
      const has = msg.tags.includes(tg.id);
      return { key: tg.id, label: tg.name, checked: has, onclick: () => void setTag(selectedIds(), tg, !has) };
    });
    items.push({ label: t('tag.manage'), icon: 'settings', onclick: () => openSettings('tags') });
    menu = { at: anchorOf(e), title: t('tag.menu'), items };
  }

  /**
   * Göndericiye göre toplu temizlik: kapsamdaki postalar bulunur, sayı
   * gösterilip onay istenir, sonra geri alınabilir toplu taşıma yapılır.
   */
  async function senderAction(mode: 'archive' | 'trash' | 'keepLatest', at: { x: number; y: number }) {
    if (!d) return;
    const msg = d;
    let ids: number[];
    try {
      ids = await senderMessages(msg.accountId, msg.from.email);
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
      return;
    }
    // En yenisini tut: liste yeniden eskiye gelir, ilki kalır.
    if (mode === 'keepLatest') ids = ids.slice(1);
    if (!ids.length) {
      showToast({ text: t(mode === 'keepLatest' ? 'bulk.senderOnlyLatest' : 'bulk.senderNone') });
      return;
    }
    const target = mode === 'trash' ? 'trash' : 'archive';
    menu = {
      at,
      title: t(target === 'trash' ? 'bulk.confirmTrash' : 'bulk.confirmArchive', { n: ids.length }),
      items: [
        { label: t('bulk.confirmGo'), icon: target === 'trash' ? 'trash' : 'archive', danger: target === 'trash', onclick: () => void bulkMove(ids, target) },
        { label: t('act.cancel'), icon: 'x', onclick: () => {} },
      ],
    };
  }

  function moreMenu(e: MouseEvent) {
    if (!d) return;
    const msg = d;
    const sender = msg.from.email;
    const at = anchorOf(e);
    menu = {
      at,
      items: [
        { label: t('act.markUnread'), icon: 'mail', hint: 'U', onclick: () => void setUnread([msg.id], true) },
        { label: t('read.openQuick'), icon: 'external', onclick: () => void call('open_quick', { id: msg.id }) },
        { label: t('print.action'), icon: 'print', onclick: () => void print() },
        { label: t('print.saveEml'), icon: 'download', onclick: () => void saveEml() },
        // Gönderilmiş posta: yanıt gelmezse hatırlat.
        ...(isSent ? [{ label: t('follow.menu'), icon: 'followUp' as const, onclick: () => (followAt = at) }] : []),
        ...(msg.unsubscribe ? [{ label: t('read.unsubscribe'), icon: 'unsubscribe' as const, onclick: () => void unsubscribe(msg) }] : []),
        ...(senderScope
          ? [
              { label: t('bulk.senderArchive'), icon: 'archive' as const, onclick: () => void senderAction('archive', at) },
              { label: t('bulk.senderKeepLatest'), icon: 'archive' as const, onclick: () => void senderAction('keepLatest', at) },
              { label: t('bulk.senderTrash'), icon: 'trash' as const, onclick: () => void senderAction('trash', at) },
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

  function compose(mode: 'replyAll' | 'forward') {
    if (d) openCompose({ mode, messageId: d.id });
  }
</script>

<main class="reader">
  {#if d}
    <div class="toolbar">
      <button class="btn-icon" aria-label={t('act.archive')} title="{t('act.archive')} · E" onclick={() => moveTo(selectedIds(), 'archive')}>
        <Icon name="archive" />
      </button>
      <div class="anchor">
        <button class="btn-icon" class:on={snoozeOpen} aria-label={t('act.snooze')} title="{t('act.snooze')} · H" aria-haspopup="menu" aria-expanded={snoozeOpen} onclick={() => (snoozeOpen = !snoozeOpen)}>
          <Icon name="clock" />
        </button>
        {#if snoozeOpen}
          <SnoozeMenu
            onclose={() => (snoozeOpen = false)}
            onpick={(at) => {
              snoozeOpen = false;
              void snooze(selectedIds(), at);
            }}
          />
        {/if}
      </div>
      <button class="btn-icon" aria-label={t('act.delete')} title="{t('act.delete')} · Del" onclick={() => moveTo(selectedIds(), 'trash')}>
        <Icon name="trash" />
      </button>
      <button class="btn-icon" aria-label={t('read.moveTo')} title={t('read.moveTo')} aria-haspopup="menu" onclick={moveMenu}>
        <Icon name="folder" />
      </button>
      <button class="btn-icon" aria-label={t('act.markUnread')} title="{t('act.markUnread')} · U" onclick={() => setUnread([d.id], true)}>
        <Icon name="mail" />
      </button>
      <button
        class="btn-icon"
        class:on={d.pinned}
        aria-label={d.pinned ? t('pin.unpin') : t('pin.pin')}
        aria-pressed={d.pinned}
        title="{d.pinned ? t('pin.unpin') : t('pin.pin')} · P"
        onclick={() => togglePin(selectedIds())}
      >
        <Icon name="pin" />
      </button>
      <button class="btn-icon" class:on={tags.length > 0} aria-label={t('tag.menu')} title={t('tag.menu')} aria-haspopup="menu" onclick={tagMenu}>
        <Icon name="tag" />
      </button>
      <span class="grow"></span>
      {#if isDraft}
        <button class="btn-primary small" title="{t('read.editDraft')} · R" onclick={editDraft}><Icon name="pen" size={16} />{t('read.editDraft')}</button>
      {:else}
        <button class="btn-soft" title="{t('act.reply')} · R" onclick={focusReply}><Icon name="reply" size={16} />{t('act.reply')}</button>
        <button class="btn-soft" title="{t('act.replyAllLong')} · A" onclick={() => compose('replyAll')}><Icon name="replyAll" size={16} />{t('act.replyAll')}</button>
        <button class="btn-soft" title="{t('act.forward')} · F" onclick={() => compose('forward')}><Icon name="forward" size={16} />{t('act.forward')}</button>
      {/if}
      <button class="btn-icon" aria-label={t('read.more')} title={t('read.more')} aria-haspopup="menu" onclick={moreMenu}>
        <Icon name="more" />
      </button>
    </div>

    <div class="scroll">
      {#if conversation}
        {#key `${d.accountId}|${d.threadId}`}
          <article class="article">
            <div class="subject-row">
              <h2 class="selectable">{d.subject || '—'}</h2>
              {#if account && mail.accounts.length > 1}<span class="tag">{account.name}</span>{/if}
            </div>
            <ReaderNotes detail={d} {screening} />
            <Conversation selected={d} />
          </article>
        {/key}
      {:else}
        {#key d.id}
          <article class="article">
            <div class="subject-row">
              <h2 class="selectable">{d.subject || '—'}</h2>
              {#if account && mail.accounts.length > 1}<span class="tag">{account.name}</span>{/if}
            </div>
            <div class="meta">{t('read.thread', { n: d.thread.length + 1 })} · {d.from.email}</div>
            <ReaderNotes detail={d} {screening} />

            {#if d.thread.length}
              <div class="thread">
                {#each d.thread as p (p.id)}
                  <button class="prior" onclick={() => select(p.id, { markRead: false })}>
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
          </article>
        {/key}
      {/if}
    </div>

    {#if !isDraft}<div class="reply-wrap"><QuickReply bind:this={quick} detail={d} /></div>{/if}
  {:else if mail.detailError}
    <div class="empty">
      <Icon name="mail" size={28} />
      <div class="empty-title">{t('read.loadError')}</div>
      <div class="meta">{mail.detailError}</div>
    </div>
  {:else if mail.detailLoading}
    <div class="empty"><div class="meta">{t('read.loading')}</div></div>
  {:else}
    <div class="empty">
      <Icon name="inbox" size={28} />
      <div class="empty-title">{t('read.empty')}</div>
      <div class="meta">{t('read.emptyHint')}</div>
    </div>
  {/if}
</main>

{#if menu}<Menu at={menu.at} title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}
{#if followAt && d}
  <FollowUpMenu
    at={followAt}
    current={d.followUp}
    onpick={(at) => {
      const id = d.id;
      followAt = null;
      void setFollowUp(id, at);
    }}
    onclear={d.followUp != null
      ? () => {
          const id = d.id;
          followAt = null;
          void setFollowUp(id, null);
        }
      : undefined}
    onclose={() => (followAt = null)}
  />
{/if}

<style>
  .reader {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--paper);
  }

  .toolbar {
    flex: none;
    height: 52px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 16px;
    border-bottom: 1px solid var(--line);
  }

  .toolbar .btn-soft {
    margin-left: 6px;
  }

  .anchor {
    position: relative;
  }

  .grow {
    flex: 1;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .article {
    max-width: 760px;
    margin: 0 auto;
    padding: 30px 40px 24px;
    animation: fade-up var(--t-state) ease-out;
  }

  .subject-row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  h2 {
    flex: 1;
    font-size: var(--fs-title);
    line-height: 1.25;
    font-weight: 600;
    letter-spacing: -0.02em;
    text-wrap: balance;
  }

  .subject-row .tag {
    margin-top: 4px;
  }

  .article > .meta {
    margin-top: 6px;
  }

  .thread {
    margin-top: 20px;
    border-top: 1px solid var(--line);
  }

  .prior {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 12px;
    width: 100%;
    padding: 10px 4px;
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

  .prior:hover .prior-text {
    color: var(--ink-2);
  }

  .single {
    margin-top: 22px;
  }

  .reply-wrap {
    flex: none;
    padding: 12px 40px 20px;
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 40px;
    color: var(--ink-3);
    text-align: center;
  }

  .empty-title {
    color: var(--ink);
    font-weight: 600;
  }
</style>
