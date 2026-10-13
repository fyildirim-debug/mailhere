<script lang="ts">
  import { call, toAppError } from '../../lib/api';
  import { locale, t, when } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import FileBadge from '../../lib/ui/FileBadge.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { flatten, folderLabel, folderTree } from '../../lib/mail/folders';
  import MailBody from '../../lib/mail/MailBody.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import SnoozeMenu from '../../lib/mail/SnoozeMenu.svelte';
  import { displayName, fileSize, foreignReplyDomains, replyTargets } from '../../lib/util/format';
  import {
    allowRemote,
    blockSender,
    mail,
    moveTo,
    moveToFolder,
    newRuleFrom,
    select,
    selectedIds,
    sendQuickReply,
    setUnread,
    snooze,
    toggleStar,
    unsubscribe,
  } from '../../lib/stores/mail.svelte';

  let reply = $state('');
  let sending = $state(false);
  let snoozeOpen = $state(false);
  let replyBox = $state<HTMLTextAreaElement>();

  const d = $derived(mail.detail);
  const account = $derived(d ? mail.accounts.find((a) => a.id === d.accountId) : undefined);
  const recipients = $derived.by(() => {
    if (!d) return '';
    const mine = account?.email.toLowerCase();
    return [...d.to, ...d.cc].map((a) => (a.email.toLowerCase() === mine ? t('read.me') : displayName(a))).join(', ');
  });
  // Yanıt alıcıları adresleriyle gösterilir: gizli bir Reply-To fark edilsin.
  const replyList = $derived(d ? replyTargets(d).map((a) => a.email).join(', ') : '');
  const replyForeign = $derived(d ? foreignReplyDomains(d) : []);
  const totalSize = $derived(d ? d.attachments.reduce((s, a) => s + a.size, 0) : 0);
  const isDraft = $derived(d ? mail.mailboxes.find((b) => b.id === d.mailboxId)?.role === 'drafts' : false);

  export function focusReply() {
    if (isDraft) editDraft();
    else replyBox?.focus();
  }

  function editDraft() {
    if (d) void call('open_compose', { prefill: { mode: 'draft', messageId: d.id } });
  }

  export function openSnooze() {
    snoozeOpen = true;
  }

  async function send() {
    if (!d || !reply.trim() || sending) return;
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

  async function openAttachment(index: number) {
    if (!d) return;
    try {
      await call('attachment_open', { id: d.id, index });
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
        label: `${'  '.repeat(n.depth)}${folderLabel(n.mailbox)}`,
        icon: 'folder' as const,
        onclick: () => void moveToFolder(selectedIds(), n.mailbox, folderLabel(n.mailbox)),
      })),
    };
  }

  function moreMenu(e: MouseEvent) {
    if (!d) return;
    const msg = d;
    const sender = msg.from.email;
    menu = {
      at: anchorOf(e),
      items: [
        { label: t('act.markUnread'), icon: 'mail', hint: 'U', onclick: () => void setUnread([msg.id], true) },
        { label: t('read.openQuick'), icon: 'external', onclick: () => void call('open_quick', { id: msg.id }) },
        ...(msg.unsubscribe ? [{ label: t('read.unsubscribe'), icon: 'unsubscribe' as const, onclick: () => void unsubscribe(msg) }] : []),
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
    if (d) void call('open_compose', { prefill: { mode, messageId: d.id } });
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
      {#key d.id}
        <article class="article">
          <div class="subject-row">
            <h2 class="selectable">{d.subject || '—'}</h2>
            {#if account && mail.accounts.length > 1}<span class="tag">{account.name}</span>{/if}
          </div>
          <div class="meta">{t('read.thread', { n: d.thread.length + 1 })} · {d.from.email}</div>

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

          <div class="sender">
            <Avatar address={d.from} size={40} />
            <div class="who">
              <div class="trunc"><span class="name">{displayName(d.from)}</span><span class="meta addr">{d.from.email}</span></div>
              <div class="meta trunc">{t('read.to', { list: recipients })}</div>
            </div>
            <span class="meta num">{when(d.date)}</span>
            <button class="btn-icon" aria-label={d.starred ? t('act.unstar') : t('act.star')} title="{t('act.star')} · S" onclick={() => toggleStar(d.id)}>
              <span class:star={d.starred}><Icon name="star" filled={d.starred} /></span>
            </button>
          </div>

          {#if d.unsubscribe}
            <div class="unsub">
              <Icon name="unsubscribe" size={16} />
              <span class="grow">{t('read.listMail')}</span>
              <button class="link small" onclick={() => d && unsubscribe(d)}>{t('read.unsubscribe')}</button>
            </div>
          {/if}

          {#if d.hasRemoteContent && !d.remoteAllowed}
            <div class="remote">
              <Icon name="shield" />
              <span class="remote-text">{t('read.remoteBlocked')}</span>
              <button class="link small" onclick={() => allowRemote(d.id)}>{t('read.remoteAllow')}</button>
            </div>
          {/if}

          <div class="body">
            {#if d.bodyText == null && d.bodyHtml == null}
              <p class="meta">{t('read.noBody')}</p>
            {:else}
              <MailBody text={d.bodyText} html={d.bodyHtml} remoteAllowed={d.remoteAllowed} />
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
        </article>
      {/key}
    </div>

    {#if !isDraft}<div class="reply-wrap">
      {#if replyForeign.length}
        <p class="reply-warn" role="note">{t('read.replyElsewhere', { domains: replyForeign.join(', ') })}</p>
      {/if}
      <div class="replybox">
        <textarea
          bind:this={replyBox}
          bind:value={reply}
          rows="2"
          placeholder={t('read.replyPlaceholder')}
          aria-label={t('read.replyPlaceholder')}
          onkeydown={onKey}
        ></textarea>
        <div class="reply-bar">
          <span class="meta trunc" title={replyList}>{t('read.replyTo', { list: replyList })}</span>
          <span class="grow"></span>
          <span class="meta">{t('read.sendHint')}</span>
          <button class="btn-primary small" disabled={!reply.trim() || sending} onclick={send}><Icon name="send" size={16} />{t('act.send')}</button>
        </div>
      </div>
    </div>{/if}
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

<style>
  .reply-warn {
    margin: 0 0 8px;
    font-size: var(--fs-meta);
    color: var(--amber);
  }

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

  .sender {
    margin-top: 22px;
    display: flex;
    align-items: center;
    gap: 12px;
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

  .star {
    display: inline-flex;
    color: var(--amber);
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

  .reply-wrap {
    flex: none;
    padding: 12px 40px 20px;
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

  textarea {
    display: block;
    width: 100%;
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
