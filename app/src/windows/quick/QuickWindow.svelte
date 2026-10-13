<script lang="ts">
  import { onMount } from 'svelte';
  import { appWindow, call, EVENTS, on, toAppError, type Account, type MessageDetail } from '../../lib/api';
  import { t, when } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import FileBadge from '../../lib/ui/FileBadge.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Logo from '../../lib/ui/Logo.svelte';
  import WindowControls from '../../lib/ui/WindowControls.svelte';
  import MailBody from '../../lib/mail/MailBody.svelte';
  import QuickRepliesEditor from '../../lib/mail/QuickRepliesEditor.svelte';
  import SnoozeMenu from '../../lib/mail/SnoozeMenu.svelte';
  import { baseSubject, displayName, fileSize, foreignReplyDomains, replyTargets } from '../../lib/util/format';
  import { locale } from '../../lib/i18n/index.svelte';
  import { quoteFor, signatureFor } from '../../lib/stores/mail.svelte';
  import { quickReplies, settings } from '../../lib/stores/settings.svelte';

  interface Props {
    id: number;
  }

  let { id }: Props = $props();

  let d = $state<MessageDetail | null>(null);
  let account = $state<Account | null>(null);
  let error = $state<string | null>(null);
  let pinned = $state(false);
  let reply = $state('');
  let sending = $state(false);
  let sent = $state<{ outboxId: string; text: string } | null>(null);
  let done = $state<{ text: string; undo: () => Promise<void> } | null>(null);
  let snoozeOpen = $state(false);
  let closeTimer: ReturnType<typeof setTimeout> | null = null;

  const chips = $derived(quickReplies(settings.value));
  let editingChips = $state(false);

  onMount(() => {
    void load();
    // Gönderilen yanıt giden kutusundan çıkınca pencere kapanır.
    const off = on(EVENTS.outbox, (e) => {
      if (sent && e.id === sent.outboxId && e.ok) scheduleClose(1500);
    });
    return () => {
      off();
      if (closeTimer) clearTimeout(closeTimer);
    };
  });

  async function load() {
    try {
      const [detail, accounts] = await Promise.all([call('message_get', { id }), call('accounts_list')]);
      d = detail;
      account = accounts.find((a) => a.id === detail.accountId) ?? null;
      document.title = `${displayName(detail.from)} · ${detail.subject}`;
      if (detail.unread) await call('messages_flag', { ids: [id], unread: false, starred: null });
    } catch (e) {
      error = toAppError(e).message;
    }
  }

  function scheduleClose(ms: number) {
    if (closeTimer) clearTimeout(closeTimer);
    closeTimer = setTimeout(() => void appWindow.close(), ms);
  }

  function finish(text: string, undo: () => Promise<void>) {
    done = { text, undo };
    scheduleClose(4000);
  }

  async function archive() {
    await call('messages_move', { ids: [id], target: 'archive' });
    finish(t('toast.archived'), () => call('messages_move', { ids: [id], target: 'inbox' }).then(() => {}));
  }

  async function remove() {
    await call('messages_move', { ids: [id], target: 'trash' });
    finish(t('toast.deleted'), () => call('messages_move', { ids: [id], target: 'inbox' }).then(() => {}));
  }

  async function snoozeAt(at: number) {
    snoozeOpen = false;
    await call('messages_snooze', { ids: [id], until: at });
    finish(t('toast.snoozed', { when: when(at) }), () => call('messages_snooze', { ids: [id], until: null }).then(() => {}));
  }

  async function restore() {
    if (closeTimer) clearTimeout(closeTimer);
    const u = done?.undo;
    done = null;
    await u?.();
  }

  async function send() {
    if (!d || !reply.trim() || sending) return;
    sending = true;
    try {
      const text = reply;
      const sig = signatureFor(account ?? undefined, 'reply');
      const body = sig ? `${text}\n\n${sig}` : text;
      const item = await call('send', {
        msg: {
          accountId: d.accountId,
          to: replyTargets(d),
          cc: [],
          bcc: [],
          subject: `Re: ${baseSubject(d.subject)}`,
          bodyText: quoteFor(d, body),
          inReplyTo: d.id,
          attachments: [],
          sendAt: null,
        },
      });
      sent = { outboxId: item.id, text };
      reply = '';
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      sending = false;
    }
  }

  async function undoSend() {
    if (!sent) return;
    if (await call('send_cancel', { id: sent.outboxId })) reply = sent.text;
    sent = null;
  }

  async function togglePin() {
    pinned = !pinned;
    await appWindow.setAlwaysOnTop(pinned);
  }

  async function openInApp() {
    await call('open_main', { id });
    await appWindow.close();
  }

  function onKey(e: KeyboardEvent) {
    const editing = (e.target as HTMLElement | null)?.closest?.('textarea, input');
    if (e.key === 'Escape' && !snoozeOpen) void appWindow.close();
    else if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      void send();
    } else if (!editing && !e.ctrlKey && !e.metaKey && !e.altKey && d && !done) {
      if (e.key === 'e') void archive();
      else if (e.key === 'Delete' || e.key === '#') void remove();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="win">
  <header class="titlebar" data-tauri-drag-region>
    <Logo size={18} dot={false} />
    <span class="title" data-tauri-drag-region>{t('quick.title')}</span>
    {#if account}<span class="meta" data-tauri-drag-region>· {account.name}</span>{/if}
    <span class="grow" data-tauri-drag-region></span>
    <button class="btn-icon small" class:on={pinned} aria-pressed={pinned} aria-label={t('act.pin')} title={t('act.pin')} onclick={togglePin}>
      <Icon name="pin" size={16} />
    </button>
    <button class="btn-icon small" aria-label={t('act.openInApp')} title={t('act.openInApp')} onclick={openInApp}>
      <Icon name="external" size={16} />
    </button>
    <WindowControls maximize={false} minimize={false} height={42} />
  </header>

  {#if done}
    <div class="done">
      <span class="done-icon"><Icon name="check" size={30} /></span>
      <div class="done-title">{done.text}</div>
      <div class="meta">{t('quick.closing')}</div>
      <div class="done-acts">
        <button class="link" onclick={restore}>{t('act.undo')}</button>
        <button class="link" onclick={() => appWindow.close()}>{t('act.close')}</button>
      </div>
    </div>
  {:else if error && !d}
    <div class="done"><div class="done-title">{t('read.loadError')}</div><div class="meta">{error}</div></div>
  {:else if d}
    <div class="scroll">
      <div class="sender">
        <Avatar address={d.from} size={40} />
        <div class="who">
          <span class="name trunc">{displayName(d.from)}</span>
          <span class="meta trunc">{d.from.email}</span>
        </div>
        <span class="meta num">{when(d.date)}</span>
      </div>
      <h1 class="selectable">{d.subject || '—'}</h1>
      <div class="body">
        <MailBody text={d.bodyText} html={d.bodyHtml} remoteAllowed={d.remoteAllowed} />
      </div>
      {#if d.attachments.length}
        <div class="atts">
          {#each d.attachments as a (a.index)}
            <button class="att" onclick={() => call('attachment_open', { id, index: a.index })} title={a.name}>
              <FileBadge name={a.name} mime={a.mime} />
              <span class="att-text"><span class="trunc">{a.name}</span><span class="meta">{fileSize(a.size, locale())}</span></span>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="actions">
      <button class="act" onclick={archive}><Icon name="archive" size={16} />{t('act.archive')}</button>
      <div class="anchor">
        <button class="act" aria-haspopup="menu" aria-expanded={snoozeOpen} onclick={() => (snoozeOpen = !snoozeOpen)}><Icon name="clock" size={16} />{t('act.snooze')}</button>
        {#if snoozeOpen}
          <SnoozeMenu style="bottom: 40px; left: 0" onpick={snoozeAt} onclose={() => (snoozeOpen = false)} />
        {/if}
      </div>
      <button class="act" onclick={remove}><Icon name="trash" size={16} />{t('act.delete')}</button>
    </div>

    <div class="reply">
      {#if sent}
        <div class="sent">
          <Icon name="check" size={18} />
          <span class="grow">{t('quick.sent')}</span>
          <button class="link" onclick={undoSend}>{t('act.undo')}</button>
        </div>
      {:else}
        {#if editingChips}
          <div class="chip-editor">
            <div class="chip-editor-head">
              <span class="chip-editor-title">{t('qr.title')}</span>
              <button class="link" onclick={() => (editingChips = false)}>{t('qr.done')}</button>
            </div>
            <QuickRepliesEditor compact />
          </div>
        {:else}
          <div class="chips">
            {#each chips as c (c)}
              <button class="chip" onclick={() => (reply = c)}>{c}</button>
            {/each}
            <button class="chip edit" aria-label={t('qr.edit')} title={t('qr.edit')} onclick={() => (editingChips = true)}>
              <Icon name={chips.length ? 'pen' : 'plus'} size={14} />{#if !chips.length}{t('qr.add')}{/if}
            </button>
          </div>
        {/if}
        {#if foreignReplyDomains(d).length}
          <p class="reply-warn" role="note">{t('read.replyElsewhere', { domains: foreignReplyDomains(d).join(', ') })}</p>
        {/if}
        <div class="box">
          <textarea
            bind:value={reply}
            rows="2"
            placeholder={t('quick.replyTo', { name: displayName(replyTargets(d)[0]) })}
            aria-label={t('act.reply')}
          ></textarea>
          <div class="box-bar">
            <span class="meta trunc" title={replyTargets(d).map((a) => a.email).join(', ')}>{t('read.replyTo', { list: replyTargets(d).map((a) => a.email).join(', ') })}</span>
            <span class="grow"></span>
            <button class="btn-primary small" disabled={!reply.trim() || sending} onclick={send}><Icon name="send" size={15} />{t('act.send')}</button>
          </div>
        </div>
        {#if error}<p class="error">{error}</p>{/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .win {
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--paper);
  }

  .titlebar {
    flex: none;
    height: 42px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 14px;
    background: var(--ground);
    border-bottom: 1px solid var(--line);
  }

  .title {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-2);
  }

  .titlebar .meta {
    color: var(--ink-2);
  }

  .grow {
    flex: 1;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 18px 20px 8px;
    animation: fade-up var(--t-state) ease-out;
  }

  .sender {
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

  h1 {
    margin-top: 18px;
    font-size: var(--fs-section);
    line-height: 1.3;
    font-weight: 600;
    letter-spacing: -0.015em;
  }

  .body {
    margin-top: 14px;
  }

  .atts {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 18px;
  }

  .att {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px 6px 6px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    background: var(--paper);
    text-align: left;
  }

  .att:hover {
    background: var(--hover);
  }

  .att-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .actions {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 12px;
    border-top: 1px solid var(--line);
  }

  .anchor {
    position: relative;
  }

  .act {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink-2);
    font-weight: 500;
  }

  .act:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .reply {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 4px 16px 16px;
  }

  .chips {
    display: flex;
    gap: 6px;
    overflow-x: auto;
  }

  .chip {
    flex: none;
    height: 30px;
    padding: 0 12px;
    border-radius: 999px;
    border: 1px solid var(--line-strong);
    background: var(--paper);
    white-space: nowrap;
    transition:
      background var(--t-fast),
      border-color var(--t-fast);
  }

  .chip:hover {
    background: var(--surface);
    border-color: var(--ink-3);
  }

  .chip.edit {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    border-style: dashed;
    color: var(--ink-2);
  }

  .chip-editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: var(--r-card);
    background: var(--surface);
    border: 1px solid var(--line);
    animation: fade-up var(--t-state) ease-out;
  }

  .chip-editor-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .chip-editor-title {
    font-weight: 600;
  }

  .box {
    border: 1px solid var(--line-strong);
    border-radius: var(--r-card);
    background: var(--paper);
    transition:
      border-color var(--t-state),
      box-shadow var(--t-state);
  }

  .box:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  textarea {
    display: block;
    width: 100%;
    min-height: 44px;
    max-height: 160px;
    padding: 10px 12px 2px;
    border: 0;
    outline: none;
    resize: none;
    background: transparent;
    line-height: 1.5;
    field-sizing: content;
  }

  .reply-warn {
    margin-bottom: 8px;
    font-size: var(--fs-meta);
    color: var(--amber);
  }

  .box-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px 6px 12px;
  }

  .sent {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--r-card);
    background: var(--surface);
    border: 1px solid var(--line);
    color: var(--ok);
    animation: fade-up var(--t-state) var(--ease);
  }

  .sent .grow {
    color: var(--ink);
  }

  .done {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 32px;
    text-align: center;
    animation: fade-up var(--t-state) var(--ease);
  }

  .done-icon {
    color: var(--accent);
  }

  .done-title {
    font-weight: 600;
  }

  .done-acts {
    display: flex;
    gap: 16px;
    margin-top: 6px;
  }

  .error {
    font-size: var(--fs-meta);
    color: var(--danger);
  }
</style>
