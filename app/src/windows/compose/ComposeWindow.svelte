<script lang="ts">
  import { onMount } from 'svelte';
  import {
    appWindow,
    call,
    EVENTS,
    on,
    pickFiles,
    toAppError,
    type Account,
    type Address,
    type Attachment,
    type ComposeMode,
    type MessageDetail,
    type MessageId,
    type OutgoingMessage,
  } from '../../lib/api';
  import { locale, t, when, type MessageKey } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import FileBadge from '../../lib/ui/FileBadge.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import WindowControls from '../../lib/ui/WindowControls.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import { baseSubject, displayName, fileSize, snoozeOptions } from '../../lib/util/format';

  interface Props {
    mode: ComposeMode;
    messageId: number | null;
  }

  let { mode, messageId }: Props = $props();

  let accounts = $state<Account[]>([]);
  let fromId = $state<string>('');
  let to = $state<Address[]>([]);
  let cc = $state<Address[]>([]);
  let bcc = $state<Address[]>([]);
  let showCc = $state(false);
  let showBcc = $state(false);
  let subject = $state('');
  let body = $state('');
  let attachments = $state<string[]>([]);
  let original = $state<MessageDetail | null>(null);
  let laterOpen = $state(false);
  let sendAt = $state<number | null>(null);
  let queued = $state<{ id: string; at: number } | null>(null);
  let error = $state<string | null>(null);
  let bodyEl = $state<HTMLTextAreaElement>();
  let drafts = $state<Record<'to' | 'cc' | 'bcc', string>>({ to: '', cc: '', bcc: '' });
  let suggest = $state<{ field: 'to' | 'cc' | 'bcc'; items: Address[]; active: number } | null>(null);
  let suggestTimer: ReturnType<typeof setTimeout> | null = null;

  // Taslak: sunucudaki Taslaklar kutusuna kendiliğinden kaydedilir.
  let draftId = $state<MessageId | null>(null);
  /** İletilen (ya da yeniden açılan taslaktaki) postanın ekleri; gönderimde arka uç ekler. */
  let forwardFrom = $state<MessageId | null>(null);
  let carried = $state<Attachment[]>([]);
  let draftState = $state<'idle' | 'saving' | 'saved' | 'error'>('idle');
  let draftError = $state<string | null>(null);
  let baseline = '';
  let draftTimer: ReturnType<typeof setTimeout> | null = null;
  let saving: Promise<void> | null = null;
  let ready = $state(false);
  let discarded = false;

  const from = $derived(accounts.find((a) => a.id === fromId));
  const locked = $derived(queued != null);

  // İmza: gönderirken metnin sonuna eklenir. Hesap değişince o hesabın varsayılanı gelir.
  let sigId = $state<string | null>(null);
  $effect(() => {
    const a = from;
    sigId = a ? (mode === 'new' || mode === 'draft' ? a.signatureNew : a.signatureReply) : null;
  });
  const signature = $derived(from?.signatures.find((x) => x.id === sigId) ?? null);

  let menu = $state<{ at: { x: number; y: number }; title: string; items: MenuItem[] } | null>(null);

  function below(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left, y: r.bottom + 4 };
  }

  function signatureMenu(e: MouseEvent) {
    if (!from) return;
    menu = {
      at: below(e),
      title: t('sig.pick'),
      items: [
        { label: t('sig.noneOption'), checked: sigId == null, onclick: () => (sigId = null) },
        ...from.signatures.map((x) => ({ label: x.name || t('sig.unnamed'), checked: x.id === sigId, onclick: () => (sigId = x.id) })),
      ],
    };
  }

  /** Şablonu imlecin olduğu yere ekler; konu boşsa şablonun konusunu yazar. */
  function insertTemplate(id: string) {
    const tpl = settings.value.templates.find((x) => x.id === id);
    if (!tpl) return;
    if (tpl.subject && (!subject.trim() || mode === 'new')) subject = tpl.subject;
    const el = bodyEl;
    // İmleç konumu odak gitse de textarea'da kalır.
    const at = el ? el.selectionStart : 0;
    body = body.slice(0, at) + tpl.body + body.slice(at);
    queueMicrotask(() => {
      el?.focus();
      el?.setSelectionRange(at + tpl.body.length, at + tpl.body.length);
    });
  }

  function templateMenu(e: MouseEvent) {
    const list = settings.value.templates;
    menu = {
      at: below(e),
      title: t('tpl.insert'),
      items: list.length
        ? list.map((x) => ({ label: x.name, icon: 'template' as const, onclick: () => insertTemplate(x.id) }))
        : [{ label: t('tpl.noneCompose'), disabled: true, onclick: () => {} }],
    };
  }

  onMount(() => {
    void init();
    const off = on(EVENTS.outbox, (e) => {
      if (!queued || e.id !== queued.id) return;
      if (e.ok) void appWindow.close();
      else {
        error = t('toast.sendFailed', { message: e.error ?? '' });
        queued = null;
      }
    });
    // Pencere kapanırken bekleyen değişikliği kaydetmeyi dene.
    const flush = () => {
      if (!discarded && !queued && dirty()) void saveDraft();
    };
    window.addEventListener('pagehide', flush);
    return () => {
      off();
      window.removeEventListener('pagehide', flush);
      if (draftTimer) clearTimeout(draftTimer);
    };
  });

  async function init() {
    accounts = await call('accounts_list');
    fromId = accounts[0]?.id ?? '';
    if (messageId != null) {
      try {
        const d = await call('message_get', { id: messageId });
        fromId = d.accountId;
        if (mode === 'draft') openDraft(d);
        else {
          original = d;
          prefill(d);
        }
      } catch (e) {
        error = toAppError(e).message;
      }
    }
    baseline = snapshot();
    ready = true;
    document.title = title();
    setTimeout(() => {
      if (to.length && bodyEl) {
        bodyEl.focus();
        bodyEl.setSelectionRange(0, 0);
        bodyEl.scrollTop = 0;
      } else document.getElementById('compose-to')?.focus();
    });
  }

  function same(a: Address, b: Address) {
    return a.email.toLowerCase() === b.email.toLowerCase();
  }

  function prefill(d: MessageDetail) {
    const me = accounts.find((a) => a.id === d.accountId)?.email.toLowerCase();
    const notMe = (a: Address) => a.email.toLowerCase() !== me;
    const quoted = (d.bodyText ?? d.snippet)
      .split('\n')
      .map((l) => `> ${l}`)
      .join('\n');
    const header = t('compose.wrote', { date: when(d.date), name: displayName(d.from) });
    if (mode === 'forward') {
      subject = `Fwd: ${baseSubject(d.subject)}`;
      body = `\n\n${t('compose.forwarded')}\n${displayName(d.from)} <${d.from.email}>\n${when(d.date)}\n${d.subject}\n\n${d.bodyText ?? d.snippet}`;
      if (d.attachments.length) {
        forwardFrom = d.id;
        carried = d.attachments;
      }
      return;
    }
    subject = `Re: ${baseSubject(d.subject)}`;
    to = (d.replyTo.length ? d.replyTo : [d.from]).filter(notMe);
    if (!to.length) to = d.to.filter(notMe);
    if (mode === 'replyAll') {
      cc = [...d.to, ...d.cc].filter(notMe).filter((a) => !to.some((x) => same(x, a)));
      showCc = cc.length > 0;
    }
    body = `\n\n${header}\n${quoted}`;
  }

  /** Kayıtlı bir taslağı düzenlemek için açar. */
  function openDraft(d: MessageDetail) {
    draftId = d.id;
    to = [...d.to];
    cc = [...d.cc];
    bcc = [...d.bcc];
    showCc = cc.length > 0;
    showBcc = bcc.length > 0;
    subject = d.subject;
    body = d.bodyText ?? '';
    if (d.attachments.length) {
      forwardFrom = d.id;
      carried = d.attachments;
    }
  }

  function title() {
    if (mode === 'draft') return t('compose.draft', { subject: subject.trim() || t('compose.noSubject') });
    if (mode === 'forward' && original) return t('compose.forward', { subject: baseSubject(original.subject) });
    if (original) return t('compose.reply', { subject: baseSubject(original.subject) });
    return t('compose.new');
  }

  function outgoing(): OutgoingMessage | null {
    if (!from) return null;
    return {
      accountId: from.id,
      to: [...to],
      cc: [...cc],
      bcc: [...bcc],
      subject,
      bodyText: body,
      inReplyTo: original?.id ?? null,
      attachments: [...attachments],
      sendAt: null,
      forwardFrom,
      draftId,
    };
  }

  function snapshot(): string {
    return JSON.stringify([fromId, to, cc, bcc, subject, body, attachments, forwardFrom]);
  }

  function dirty(): boolean {
    return ready && snapshot() !== baseline;
  }

  // Her değişiklikten 1,5 sn sonra kaydet (yazarken her tuşta değil).
  $effect(() => {
    const snap = snapshot();
    if (!ready || locked || snap === baseline) return;
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(() => void saveDraft(), 1500);
  });

  async function saveDraft(): Promise<void> {
    if (saving) {
      await saving;
      if (!dirty()) return;
    }
    const msg = outgoing();
    if (!msg || discarded || queued) return;
    const snap = snapshot();
    draftState = 'saving';
    saving = (async () => {
      try {
        try {
          draftId = await call('draft_save', { msg });
        } catch (e) {
          // Taslak bu arada silinmiş olabilir (ör. gönderildi ve geri alındı): yenisini yaz.
          // Ekler o taslaktan geliyorsa artık okunamaz; onlarsız kaydet.
          if (msg.draftId == null || toAppError(e).code !== 'notFound') throw e;
          const lost = msg.forwardFrom != null && msg.forwardFrom === msg.draftId;
          if (lost) {
            forwardFrom = null;
            carried = [];
          }
          draftId = await call('draft_save', { msg: { ...msg, draftId: null, forwardFrom: lost ? null : msg.forwardFrom } });
        }
        baseline = snap;
        draftState = 'saved';
        draftError = null;
      } catch (e) {
        draftState = 'error';
        draftError = toAppError(e).message;
      } finally {
        saving = null;
      }
    })();
    await saving;
  }

  async function discard() {
    discarded = true;
    if (draftTimer) clearTimeout(draftTimer);
    if (saving) await saving;
    if (draftId != null) {
      try {
        await call('draft_delete', { id: draftId });
      } catch {
        /* zaten yoksa sorun değil */
      }
    }
    await appWindow.close();
  }

  function parse(text: string): Address | null {
    const m = text.trim().match(/^(?:"?([^"<]*?)"?\s*<)?([^\s<>@]+@[^\s<>@]+\.[^\s<>@]+)>?$/);
    if (!m) return null;
    return { name: m[1]?.trim() || null, email: m[2] };
  }

  function commit(field: 'to' | 'cc' | 'bcc') {
    const parts = drafts[field].split(/[,;\n]+/).map((x) => x.trim()).filter(Boolean);
    const bad: string[] = [];
    const list = field === 'to' ? to : field === 'cc' ? cc : bcc;
    for (const p of parts) {
      const a = parse(p);
      if (a && !list.some((x) => same(x, a))) list.push(a);
      else if (!a) bad.push(p);
    }
    drafts[field] = bad.join(', ');
  }

  function onRecipientKey(field: 'to' | 'cc' | 'bcc', e: KeyboardEvent) {
    const list = field === 'to' ? to : field === 'cc' ? cc : bcc;
    if (suggest?.field === field) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const n = suggest.items.length;
        suggest.active = (suggest.active + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        pick(field, suggest.items[suggest.active]);
        return;
      }
      if (e.key === 'Escape') {
        e.stopPropagation();
        suggest = null;
        return;
      }
    }
    if (e.key === 'Enter' || e.key === ',' || e.key === ';' || e.key === 'Tab') {
      if (drafts[field].trim()) {
        if (e.key !== 'Tab') e.preventDefault();
        commit(field);
      }
    } else if (e.key === 'Backspace' && !drafts[field] && list.length) {
      list.pop();
    }
  }

  function removeAt(field: 'to' | 'cc' | 'bcc', i: number) {
    (field === 'to' ? to : field === 'cc' ? cc : bcc).splice(i, 1);
  }

  /** Yazılan parçaya uyan adresleri geçmiş postalardan önerir. */
  function lookup(field: 'to' | 'cc' | 'bcc') {
    if (suggestTimer) clearTimeout(suggestTimer);
    const q = drafts[field].trim();
    if (q.length < 2) {
      suggest = null;
      return;
    }
    suggestTimer = setTimeout(async () => {
      const needle = q.toLocaleLowerCase('tr');
      const taken = new Set([...to, ...cc, ...bcc].map((a) => a.email.toLowerCase()));
      const seen = new Map<string, Address>();
      for (const m of await call('search', { q, limit: 30 })) {
        for (const a of [m.from, ...m.to]) {
          const key = a.email.toLowerCase();
          const hay = `${a.name ?? ''} ${a.email}`.toLocaleLowerCase('tr');
          if (!taken.has(key) && !seen.has(key) && hay.includes(needle)) seen.set(key, a);
        }
      }
      const items = [...seen.values()].slice(0, 5);
      suggest = drafts[field].trim() === q && items.length ? { field, items, active: 0 } : null;
    }, 120);
  }

  function pick(field: 'to' | 'cc' | 'bcc', a: Address) {
    const list = field === 'to' ? to : field === 'cc' ? cc : bcc;
    if (!list.some((x) => same(x, a))) list.push(a);
    drafts[field] = '';
    suggest = null;
    document.getElementById(`compose-${field}`)?.focus();
  }

  /** Seçili metni sarar (düz metin, okunur işaretlerle). */
  function wrap(before: string, after = before) {
    const el = bodyEl;
    if (!el || locked) return;
    const { selectionStart: s, selectionEnd: e } = el;
    const sel = body.slice(s, e);
    body = body.slice(0, s) + before + sel + after + body.slice(e);
    queueMicrotask(() => {
      el.focus();
      el.setSelectionRange(s + before.length, s + before.length + sel.length);
    });
  }

  function bullet() {
    const el = bodyEl;
    if (!el || locked) return;
    const s = el.selectionStart;
    const lineStart = body.lastIndexOf('\n', s - 1) + 1;
    body = body.slice(0, lineStart) + '- ' + body.slice(lineStart);
    queueMicrotask(() => {
      el.focus();
      el.setSelectionRange(s + 2, s + 2);
    });
  }

  async function attach() {
    const files = await pickFiles();
    attachments = [...attachments, ...files.filter((f) => !attachments.includes(f))];
  }

  function fileName(path: string) {
    return path.split(/[\\/]/).pop() ?? path;
  }

  async function send() {
    error = null;
    commit('to');
    commit('cc');
    commit('bcc');
    if (!to.length && !cc.length && !bcc.length) {
      error = t('compose.needRecipient');
      return;
    }
    const msg = outgoing();
    if (!from || !msg) return;
    if (draftTimer) clearTimeout(draftTimer);
    if (saving) await saving;
    const sig = signature?.body.trimEnd();
    const text = sig ? `${body.trimEnd()}\n\n${sig}` : body;
    try {
      const item = await call('send', { msg: { ...msg, bodyText: text, sendAt, draftId } });
      queued = { id: item.id, at: item.sendAt };
      laterOpen = false;
    } catch (e) {
      error = toAppError(e).message;
    }
  }

  async function undo() {
    if (!queued) return;
    const ok = await call('send_cancel', { id: queued.id });
    if (ok) {
      queued = null;
      // Gönderimde taslak silinmiş olabilir; içerik kaybolmasın diye yeniden kaydet.
      baseline = '';
    }
  }

  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      if (!locked) void send();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      if (!locked && ready) void saveDraft();
    } else if (e.key === 'Escape' && laterOpen) {
      laterOpen = false;
    }
  }

  const later = $derived(
    snoozeOptions(Date.now())
      .filter((o) => o.key !== 'weekend')
      .map((o) => ({ ...o, label: `snooze.${o.key}` as MessageKey })),
  );

  const seconds = $derived(queued ? Math.max(1, Math.round((queued.at - Date.now()) / 1000)) : 0);
</script>

<svelte:window onkeydown={onKey} />

<div class="win">
  <header class="titlebar" data-tauri-drag-region>
    <span class="title trunc" data-tauri-drag-region>{title()}</span>
    <span class="grow" data-tauri-drag-region></span>
    <WindowControls height={40} />
  </header>

  <div class="fields">
    <div class="field">
      <label class="flabel" for="compose-from">{t('compose.from')}</label>
      <select id="compose-from" class="from" bind:value={fromId} disabled={locked || accounts.length < 2}>
        {#each accounts as a (a.id)}
          <option value={a.id}>{a.displayName} · {a.email}</option>
        {/each}
      </select>
      {#if from}<span class="acc-dot" style:background={from.color}></span>{/if}
    </div>

    {#each [{ key: 'to', label: 'compose.to', list: to, show: true }, { key: 'cc', label: 'compose.cc', list: cc, show: showCc }, { key: 'bcc', label: 'compose.bcc', list: bcc, show: showBcc }] as row (row.key)}
      {#if row.show}
        {@const field = row.key as 'to' | 'cc' | 'bcc'}
        <div class="field wrap">
          <label class="flabel" for="compose-{field}">{t(row.label as MessageKey)}</label>
          {#each row.list as a, i (a.email)}
            <span class="chip" title={a.email}>
              <Avatar address={a} size={20} />
              <span>{displayName(a)}</span>
              <button aria-label={t('compose.removeRecipient', { name: displayName(a) })} disabled={locked} onclick={() => removeAt(field, i)}>
                <Icon name="x" size={12} />
              </button>
            </span>
          {/each}
          <input
            id="compose-{field}"
            class="rinput"
            placeholder={row.list.length ? '' : t('compose.addRecipient')}
            bind:value={drafts[field]}
            oninput={() => lookup(field)}
            onkeydown={(e) => onRecipientKey(field, e)}
            onblur={() => setTimeout(() => {
              if (suggest?.field === field) suggest = null;
              commit(field);
            }, 120)}
            role="combobox"
            aria-expanded={suggest?.field === field}
            aria-controls="suggest-{field}"
            aria-autocomplete="list"
            disabled={locked}
            autocomplete="email"
          />
          {#if suggest?.field === field}
            <div class="popover suggest" id="suggest-{field}" role="listbox">
              {#each suggest.items as a, i (a.email)}
                <button
                  class="menu-item"
                  role="option"
                  aria-selected={i === suggest.active}
                  class:active={i === suggest.active}
                  onmousedown={(e) => {
                    e.preventDefault();
                    pick(field, a);
                  }}
                >
                  <Avatar address={a} size={24} />
                  <span class="trunc">{displayName(a)}</span>
                  <span class="meta trunc">{a.email}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if field === 'to'}
            {#if !showCc}<button class="btn-ghost" onclick={() => (showCc = true)}>{t('compose.cc')}</button>{/if}
            {#if !showBcc}<button class="btn-ghost" onclick={() => (showBcc = true)}>{t('compose.bcc')}</button>{/if}
          {/if}
        </div>
      {/if}
    {/each}

    <div class="field">
      <label class="flabel" for="compose-subject">{t('compose.subject')}</label>
      <input id="compose-subject" class="rinput" bind:value={subject} disabled={locked} />
    </div>
  </div>

  <div class="tools">
    <button class="btn-icon small" aria-label={t('compose.bold')} title={t('compose.bold')} onclick={() => wrap('**')}><Icon name="bold" size={16} /></button>
    <button class="btn-icon small" aria-label={t('compose.italic')} title={t('compose.italic')} onclick={() => wrap('_')}><Icon name="italic" size={16} /></button>
    <button class="btn-icon small" aria-label={t('compose.link')} title={t('compose.link')} onclick={() => wrap('[', '](https://)')}><Icon name="link" size={16} /></button>
    <button class="btn-icon small" aria-label={t('compose.bullet')} title={t('compose.bullet')} onclick={bullet}><Icon name="list" size={16} /></button>
    <span class="vsep"></span>
    <button class="btn-icon small" aria-label={t('act.attach')} title={t('act.attach')} onclick={attach} disabled={locked}><Icon name="paperclip" size={16} /></button>
    <button class="btn-icon small" aria-label={t('tpl.insert')} title={t('tpl.insert')} aria-haspopup="menu" onclick={templateMenu} disabled={locked}><Icon name="template" size={16} /></button>
    <span class="grow"></span>
    {#if from?.signatures.length}
      <button class="btn-ghost sig-pick" aria-haspopup="menu" disabled={locked} onclick={signatureMenu}>
        <Icon name="pen" size={14} />{t('compose.signature', { name: signature ? signature.name || t('sig.unnamed') : t('sig.noneOption') })}<Icon name="chevronDown" size={14} />
      </button>
    {/if}
  </div>

  <div class="editor">
    <textarea bind:this={bodyEl} bind:value={body} placeholder={t('compose.bodyPlaceholder')} aria-label={t('compose.body')} disabled={locked}></textarea>
    {#if signature?.body.trim()}<div class="signature">{signature.body}</div>{/if}
    {#if attachments.length || carried.length}
      <div class="files">
        {#each carried as a (a.index)}
          <span class="file" title="{a.name} · {t('compose.original')}">
            <FileBadge name={a.name} />
            <span class="trunc">{a.name}</span>
            <span class="meta num size">{fileSize(a.size, locale())}</span>
          </span>
        {/each}
        {#each attachments as path (path)}
          <span class="file" title={path}>
            <FileBadge name={fileName(path)} />
            <span class="trunc">{fileName(path)}</span>
            <button class="btn-icon small" aria-label={t('compose.removeAttachment')} disabled={locked} onclick={() => (attachments = attachments.filter((p) => p !== path))}>
              <Icon name="x" size={12} />
            </button>
          </span>
        {/each}
      </div>
    {/if}
  </div>

  <footer class="foot">
    {#if queued}
      <span class="status"><Icon name="check" size={14} />{t('compose.queued', { n: seconds })}<button class="link" onclick={undo}>{t('act.undo')}</button></span>
    {:else if error}
      <span class="status err" role="alert">{error}</span>
    {:else if sendAt}
      <span class="status"><Icon name="clock" size={14} />{t('compose.scheduled', { when: when(sendAt) })}<button class="link" onclick={() => (sendAt = null)}>{t('act.cancel')}</button></span>
    {:else if draftState === 'saving'}
      <span class="meta" role="status">{t('compose.draftSaving')}</span>
    {:else if draftState === 'saved'}
      <span class="meta" role="status">{t('compose.draftSaved')}</span>
    {:else if draftState === 'error'}
      <span class="status err" role="alert">{t('compose.draftFailed', { message: draftError ?? '' })}</span>
    {:else}
      <span class="meta">{t('read.sendHint')}</span>
    {/if}
    <span class="grow"></span>
    <button class="btn-icon" aria-label={t('compose.discard')} title={t('compose.discard')} disabled={locked} onclick={discard}><Icon name="trash" /></button>
    <div class="split">
      <button class="send" onclick={send} disabled={locked}><Icon name="send" size={16} />{sendAt ? t('act.schedule') : t('act.send')}<span class="kbd on-accent">Ctrl Enter</span></button>
      <button class="more" aria-label={t('compose.later')} title={t('compose.later')} aria-haspopup="menu" aria-expanded={laterOpen} disabled={locked} onclick={() => (laterOpen = !laterOpen)}>
        <Icon name="chevronDown" size={16} />
      </button>
      {#if laterOpen}
        <div class="popover later" role="menu">
          <div class="menu-title">{t('compose.later')}</div>
          {#each later as o (o.key)}
            <button class="menu-item" role="menuitem" onclick={() => ((sendAt = o.at), (laterOpen = false))}>
              <Icon name="clock" size={16} />
              <span class="grow">{t(o.label)}</span>
              <span class="meta num">{when(o.at)}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </footer>
</div>

{#if menu}<Menu at={menu.at} title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}

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
    height: 40px;
    display: flex;
    align-items: center;
    padding-left: 16px;
    background: var(--ground);
    border-bottom: 1px solid var(--line);
  }

  .title {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-2);
  }

  .grow {
    flex: 1;
  }

  .fields {
    flex: none;
    padding: 0 20px;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    border-bottom: 1px solid var(--line);
  }

  .field.wrap {
    position: relative;
    flex-wrap: wrap;
    padding: 6px 0;
  }

  .suggest {
    top: calc(100% - 2px);
    left: 56px;
    width: 380px;
  }

  .suggest .active {
    background: var(--sel);
  }

  .flabel {
    width: 56px;
    flex: none;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .from {
    flex: 1;
    min-width: 0;
    height: 32px;
    border: 0;
    background: transparent;
    color: var(--ink);
    outline: none;
    appearance: none;
  }

  .acc-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .rinput {
    flex: 1;
    min-width: 120px;
    height: 32px;
    border: 0;
    outline: none;
    background: transparent;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
  }

  .chip button {
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--ink-3);
  }

  .chip button:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 14px;
    border-bottom: 1px solid var(--line);
  }

  .sig-pick {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .vsep {
    width: 1px;
    height: 18px;
    margin: 0 6px;
    background: var(--line-strong);
  }

  .editor {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 20px 8px;
    overflow-y: auto;
  }

  textarea {
    flex: 1;
    min-height: 160px;
    border: 0;
    outline: none;
    resize: none;
    background: transparent;
    line-height: 1.65;
  }

  .signature {
    color: var(--ink-3);
    white-space: pre-line;
  }

  .files {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .file {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    max-width: 320px;
    padding: 4px 4px 4px 4px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
  }

  .file .size {
    padding-right: 8px;
  }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px 14px 20px;
    border-top: 1px solid var(--line);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .status :global(.icon) {
    color: var(--ok);
  }

  .status .link {
    font-size: var(--fs-meta);
  }

  .err {
    color: var(--danger);
  }

  .split {
    position: relative;
    display: flex;
  }

  .send {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 12px 0 14px;
    border: 0;
    border-radius: var(--r-btn) 0 0 var(--r-btn);
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 500;
  }

  .more {
    width: 34px;
    height: 36px;
    display: grid;
    place-items: center;
    border: 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-ink) 25%, transparent);
    border-radius: 0 var(--r-btn) var(--r-btn) 0;
    background: var(--accent);
    color: var(--accent-ink);
  }

  .send:hover:not(:disabled),
  .more:hover:not(:disabled) {
    filter: brightness(1.07);
  }

  .send:disabled,
  .more:disabled {
    opacity: 0.45;
  }

  .on-accent {
    background: transparent;
    border-color: color-mix(in srgb, var(--accent-ink) 35%, transparent);
    color: inherit;
  }

  .later {
    right: 0;
    bottom: 44px;
    width: 280px;
  }
</style>
