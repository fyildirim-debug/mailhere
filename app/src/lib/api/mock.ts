// Tarayıcıda ve testlerde Tauri yerine çalışan sahte arka uç.
// Davranışı Rust tarafıyla aynı sözleşmeyi izler (types.ts › Commands).

import { createDemoData } from './demo-data';
import {
  defaultSettings,
  type AuthKind,
  type MessageId,
  type OutgoingMessage,
  EVENTS,
  type Account,
  type AppError,
  type CommandArgs,
  type CommandName,
  type CommandResult,
  type Counts,
  type EventPayloads,
  type Mailbox,
  type MailboxRole,
  type MessageDetail,
  type MessageSummary,
  type Settings,
  type View,
} from './types';

type Handler = (payload: unknown) => void;

export interface MockOptions {
  /** Hesapsız başla (hesap ekleme akışı). */
  empty?: boolean;
  now?: Date;
  /** Ayarları localStorage'da tut. */
  persist?: boolean;
}

export interface MockBackend {
  call<K extends CommandName>(name: K, args?: CommandArgs<K>): Promise<CommandResult<K>>;
  on<E extends keyof EventPayloads>(event: E, handler: (p: EventPayloads[E]) => void): () => void;
  emit<E extends keyof EventPayloads>(event: E, payload: EventPayloads[E]): void;
  /** Testler için iç durum. */
  state: { accounts: Account[]; mailboxes: Mailbox[]; messages: MessageDetail[]; settings: Settings };
}

const SETTINGS_KEY = 'mailhere.mock.settings';

function fail(code: AppError['code'], message: string): never {
  throw { code, message } satisfies AppError;
}

function lower(s: string): string {
  return s.toLocaleLowerCase('tr');
}

function summary(m: MessageDetail): MessageSummary {
  const {
    id, accountId, mailboxId, threadId, from, to, subject, snippet, date, unread, starred, hasAttachments, category, snoozedUntil,
  } = m;
  return { id, accountId, mailboxId, threadId, from, to, subject, snippet, date, unread, starred, hasAttachments, category, snoozedUntil };
}

function loadSettings(persist: boolean): Settings {
  if (!persist) return { ...defaultSettings };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    return raw ? { ...defaultSettings, ...JSON.parse(raw) } : { ...defaultSettings };
  } catch {
    return { ...defaultSettings };
  }
}

export function createMockBackend(opts: MockOptions = {}): MockBackend {
  const now = () => (opts.now ? opts.now.getTime() : Date.now());
  const data = createDemoData(opts.now ?? new Date());
  const state = {
    accounts: opts.empty ? [] : data.accounts,
    mailboxes: opts.empty ? [] : data.mailboxes,
    messages: opts.empty ? [] : data.messages,
    settings: loadSettings(!!opts.persist),
  };
  const handlers = new Map<string, Set<Handler>>();
  const outbox = new Map<string, ReturnType<typeof setTimeout>>();
  let nextId = 1000;

  function emit<E extends keyof EventPayloads>(event: E, payload: EventPayloads[E]) {
    handlers.get(event)?.forEach((h) => h(payload));
  }

  function roleOf(mailboxId: string): MailboxRole {
    return state.mailboxes.find((b) => b.id === mailboxId)?.role ?? 'other';
  }

  function boxFor(accountId: string, role: MailboxRole): string {
    const b = state.mailboxes.find((x) => x.accountId === accountId && x.role === role);
    if (!b) fail('notFound', 'Kutu bulunamadı.');
    return b.id;
  }

  function find(id: number): MessageDetail {
    const m = state.messages.find((x) => x.id === id);
    if (!m) fail('notFound', 'Posta bulunamadı.');
    return m;
  }

  function inView(m: MessageDetail, view: View): boolean {
    const role = roleOf(m.mailboxId);
    const snoozed = m.snoozedUntil != null && m.snoozedUntil > now();
    switch (view.kind) {
      case 'inbox':
        return role === 'inbox' && !snoozed;
      case 'snoozed':
        return snoozed;
      case 'starred':
        return m.starred && role !== 'trash';
      case 'mailbox':
        return m.mailboxId === view.id;
      default:
        return role === view.kind;
    }
  }

  function counts(): Counts {
    const perAccountUnread: Record<string, number> = {};
    let inboxUnread = 0;
    for (const m of state.messages) {
      if (m.unread && inView(m, { kind: 'inbox' })) {
        inboxUnread++;
        perAccountUnread[m.accountId] = (perAccountUnread[m.accountId] ?? 0) + 1;
      }
    }
    return {
      inboxUnread,
      snoozed: state.messages.filter((m) => inView(m, { kind: 'snoozed' })).length,
      drafts: state.messages.filter((m) => roleOf(m.mailboxId) === 'drafts').length,
      perAccountUnread,
    };
  }

  function detail(m: MessageDetail): MessageDetail {
    const thread = state.messages
      .filter((x) => x.threadId === m.threadId && x.id !== m.id && x.date < m.date && x.accountId === m.accountId)
      .sort((a, b) => a.date - b.date)
      .map((x) => ({ id: x.id, from: x.from, date: x.date, snippet: x.snippet }));
    const allowed = m.remoteAllowed || state.settings.remoteAllow.includes(lower(m.from.email));
    let bodyHtml = m.bodyHtml;
    if (bodyHtml && allowed) bodyHtml = bodyHtml.replace(/data-mh-src=/g, 'src=');
    return structuredClone({ ...m, thread, remoteAllowed: allowed, bodyHtml });
  }

  function addMailboxes(accountId: string) {
    for (const [path, name, role] of [
      ['INBOX', 'Gelen Kutusu', 'inbox'],
      ['Sent', 'Gönderilenler', 'sent'],
      ['Drafts', 'Taslaklar', 'drafts'],
      ['Archive', 'Arşiv', 'archive'],
      ['Trash', 'Çöp', 'trash'],
    ] as const) {
      state.mailboxes.push({ id: `${accountId}:${path}`, accountId, path, name, role, unread: 0, total: 0 });
    }
  }

  function newAccount(partial: Pick<Account, 'name' | 'email' | 'displayName' | 'kind' | 'imap' | 'smtp' | 'jmapUrl' | 'auth'> & { color: string | null }): Account {
    const colors = ['#4C6FD8', '#2E9A61', '#D08A1E', '#9A5BD6', '#D0566E'];
    const account: Account = {
      ...partial,
      id: `acc-${nextId++}`,
      color: partial.color ?? colors[state.accounts.length % colors.length],
      signature: null,
      createdAt: now(),
    };
    state.accounts.push(account);
    addMailboxes(account.id);
    emit(EVENTS.sync, { accountId: account.id, state: 'syncing', done: 0, total: 0 });
    return account;
  }

  const grants = new Map<string, { provider: AuthKind; email: string; displayName: string | null }>();

  function draftMessage(msg: OutgoingMessage, id: MessageId): MessageDetail {
    const account = state.accounts.find((a) => a.id === msg.accountId);
    if (!account) fail('notFound', 'Hesap bulunamadı.');
    return {
      id, accountId: account.id, mailboxId: boxFor(account.id, 'drafts'), threadId: `draft-${id}`,
      from: { name: account.displayName, email: account.email }, to: msg.to, subject: msg.subject,
      snippet: msg.bodyText.replace(/\s+/g, ' ').slice(0, 200), date: now(), unread: false, starred: false,
      hasAttachments: msg.attachments.length > 0, category: 'people', snoozedUntil: null, cc: msg.cc, replyTo: [],
      messageIdHeader: null, references: [], bodyText: msg.bodyText, bodyHtml: null, hasRemoteContent: false,
      remoteAllowed: false, attachments: [], thread: [],
    };
  }

  const commands: { [K in CommandName]: (args: CommandArgs<K>) => CommandResult<K> | Promise<CommandResult<K>> } = {
    accounts_list: () => structuredClone(state.accounts),
    account_discover: ({ email }) => {
      const domain = email.split('@')[1]?.trim();
      if (!domain || !domain.includes('.')) fail('invalid', 'Geçerli bir e-posta adresi yaz.');
      return {
        domain,
        imap: { host: `imap.${domain}`, port: 993, security: 'tls', username: email },
        smtp: { host: `smtp.${domain}`, port: 465, security: 'tls', username: email },
        source: 'autoconfig',
      };
    },
    account_add: ({ input }) => {
      if (!input.password) fail('auth', 'Parola gerekli.');
      if (input.password === 'yanlis') fail('auth', 'Sunucu parolayı kabul etmedi. Parolayı ya da uygulama parolasını kontrol et.');
      const pinned = state.settings.trustedCerts.some((c) => c.host === input.imap.host);
      if (input.imap.host.includes('kendinden') && !pinned) {
        fail('untrusted', `${input.imap.host} sunucusunun sertifikası güvenilir bir kuruluşça imzalanmamış.`);
      }
      return structuredClone(
        newAccount({ ...input, kind: 'imap', jmapUrl: null, auth: 'password' }),
      );
    },
    account_remove: ({ id }) => {
      state.accounts = state.accounts.filter((a) => a.id !== id);
      state.mailboxes = state.mailboxes.filter((b) => b.accountId !== id);
      state.messages = state.messages.filter((m) => m.accountId !== id);
      return null;
    },
    mailboxes_list: () => {
      return state.mailboxes.map((b) => {
        const inBox = state.messages.filter((m) => m.mailboxId === b.id);
        return { ...b, total: inBox.length, unread: inBox.filter((m) => m.unread).length };
      });
    },
    counts: () => counts(),
    messages_list: ({ query }) => {
      let items = state.messages
        .filter((m) => inView(m, query.view))
        .filter((m) => !query.accountId || m.accountId === query.accountId)
        .filter((m) => !query.category || m.category === query.category)
        .sort((a, b) => b.date - a.date || b.id - a.id);
      if (query.cursor) {
        const [d, i] = query.cursor.split(':').map(Number);
        items = items.filter((m) => m.date < d || (m.date === d && m.id < i));
      }
      const page = items.slice(0, query.limit);
      const last = page[page.length - 1];
      return {
        items: page.map(summary),
        nextCursor: items.length > query.limit && last ? `${last.date}:${last.id}` : null,
      };
    },
    message_get: ({ id }) => detail(find(id)),
    messages_flag: ({ ids, unread, starred }) => {
      for (const id of ids) {
        const m = find(id);
        if (unread != null) m.unread = unread;
        if (starred != null) m.starred = starred;
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    messages_move: ({ ids, target }) => {
      for (const id of ids) {
        const m = find(id);
        m.mailboxId = boxFor(m.accountId, target);
        if (target !== 'inbox') m.snoozedUntil = null;
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    messages_snooze: ({ ids, until }) => {
      for (const id of ids) find(id).snoozedUntil = until;
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    search: ({ q, limit }) => {
      const needle = lower(q.trim());
      if (!needle) return [];
      return state.messages
        .filter((m) => roleOf(m.mailboxId) !== 'trash')
        .filter((m) =>
          [m.subject, m.from.name ?? '', m.from.email, m.bodyText ?? ''].some((f) => lower(f).includes(needle)),
        )
        .sort((a, b) => b.date - a.date)
        .slice(0, limit)
        .map(summary);
    },
    remote_allow: ({ id }) => {
      const m = find(id);
      m.remoteAllowed = true;
      const who = lower(m.from.email);
      if (!state.settings.remoteAllow.includes(who)) state.settings.remoteAllow.push(who);
      return detail(m);
    },
    attachment_open: ({ id, index }) => {
      const a = find(id).attachments.find((x) => x.index === index);
      if (!a) fail('notFound', 'Ek bulunamadı.');
      return `/tmp/mailhere/${a.name}`;
    },
    send: ({ msg }) => {
      if (!msg.to.length) fail('invalid', 'En az bir alıcı ekle.');
      const id = `out-${nextId++}`;
      const sendAt = msg.sendAt ?? now() + state.settings.undoSendSeconds * 1000;
      if (msg.draftId != null) state.messages = state.messages.filter((m) => m.id !== msg.draftId);
      const timer = setTimeout(
        () => {
          outbox.delete(id);
          const account = state.accounts.find((a) => a.id === msg.accountId);
          if (account) {
            state.messages.push({
              id: nextId++, accountId: account.id, mailboxId: boxFor(account.id, 'sent'), threadId: `out-${id}`,
              from: { name: account.displayName, email: account.email }, to: msg.to, subject: msg.subject,
              snippet: msg.bodyText.replace(/\s+/g, ' ').slice(0, 200), date: now(), unread: false, starred: false,
              hasAttachments: msg.attachments.length > 0, category: 'people', snoozedUntil: null, cc: msg.cc, replyTo: [],
              messageIdHeader: null, references: [], bodyText: msg.bodyText, bodyHtml: null, hasRemoteContent: false,
              remoteAllowed: false, attachments: [], thread: [],
            });
          }
          emit(EVENTS.outbox, { id, ok: true, error: null });
        },
        Math.max(0, sendAt - now()),
      );
      outbox.set(id, timer);
      return { id, sendAt };
    },
    send_cancel: ({ id }) => {
      const t = outbox.get(id);
      if (!t) return false;
      clearTimeout(t);
      outbox.delete(id);
      return true;
    },
    settings_get: () => structuredClone(state.settings),
    settings_set: ({ settings }) => {
      state.settings = { ...settings };
      if (opts.persist) {
        try {
          localStorage.setItem(SETTINGS_KEY, JSON.stringify(state.settings));
        } catch {
          /* özel pencerede depolama kapalı olabilir */
        }
      }
      emit(EVENTS.settings, structuredClone(state.settings));
      return structuredClone(state.settings);
    },
    sync_now: () => {
      for (const a of state.accounts) emit(EVENTS.sync, { accountId: a.id, state: 'online' });
      return null;
    },
    open_main: () => {
      if (typeof window !== 'undefined') window.location.hash = '#/main';
      return null;
    },
    open_quick: ({ id }) => {
      if (typeof window !== 'undefined') window.open(`#/quick/${id}`, `quick-${id}`, 'popup=yes,width=440,height=700');
      return null;
    },
    open_compose: ({ prefill }) => {
      if (typeof window !== 'undefined') {
        window.open(`#/compose/${prefill.mode}/${prefill.messageId ?? 0}`, `compose-${nextId++}`, 'popup=yes,width=820,height=660');
      }
      return null;
    },
    notify_layout: () => null,
    notify_hide: () => null,
    notify_pending: () => [],
    notify_snooze: () => null,
    jmap_discover: ({ email }) => {
      const domain = email.split('@')[1]?.trim();
      if (!domain || !domain.includes('.')) fail('discovery', 'Bu adres için JMAP sunucusu bulunamadı.');
      return `https://${domain}/.well-known/jmap`;
    },
    account_add_jmap: ({ input }) => {
      if (!input.password) fail('auth', 'Parola gerekli.');
      return newAccount({ ...input, kind: 'jmap', imap: null, smtp: null, jmapUrl: input.sessionUrl, auth: 'password' });
    },
    oauth_providers: () => ['google', 'microsoft'],
    oauth_sign_in: async ({ provider, email }) => {
      await new Promise((r) => setTimeout(r, 300));
      const grantId = `grant-${nextId++}`;
      const addr = email?.includes('@') ? email : provider === 'google' ? 'furkan@gmail.com' : 'furkan@outlook.com';
      grants.set(grantId, { provider, email: addr, displayName: 'Furkan Yıldırım' });
      return { grantId, provider, email: addr, displayName: 'Furkan Yıldırım' };
    },
    account_add_oauth: ({ input }) => {
      const g = grants.get(input.grantId);
      if (!g) fail('auth', 'Giriş süresi doldu. Yeniden giriş yap.');
      grants.delete(input.grantId);
      const google = g.provider === 'google';
      const imap = { host: google ? 'imap.gmail.com' : 'outlook.office365.com', port: 993, security: 'tls' as const, username: g.email };
      const smtp = { host: google ? 'smtp.gmail.com' : 'smtp.office365.com', port: google ? 465 : 587, security: google ? ('tls' as const) : ('starttls' as const), username: g.email };
      return newAccount({ name: input.name, email: g.email, displayName: input.displayName, color: input.color, kind: 'imap', imap, smtp, jmapUrl: null, auth: g.provider });
    },
    account_update: ({ id, update }) => {
      const a = state.accounts.find((x) => x.id === id);
      if (!a) fail('notFound', 'Hesap bulunamadı.');
      Object.assign(a, update);
      emit(EVENTS.changed, { mailboxIds: [] });
      return structuredClone(a);
    },
    cert_inspect: ({ host, port }) => ({
      host,
      port,
      sha256: 'A1:B2:C3:D4:E5:F6:07:18:29:3A:4B:5C:6D:7E:8F:90:01:12:23:34:45:56:67:78:89:9A:AB:BC:CD:DE:EF:F0',
      subject: `CN=${host}`,
      issuer: `CN=${host}`,
      notBefore: now() - 30 * 86_400_000,
      notAfter: now() + 335 * 86_400_000,
      names: [host],
      problem: 'kendinden imzalı',
    }),
    cert_trust: ({ host, port, sha256 }) => {
      state.settings.trustedCerts = [...state.settings.trustedCerts.filter((c) => !(c.host === host && c.port === port)), { host, port, sha256 }];
      emit(EVENTS.settings, structuredClone(state.settings));
      return null;
    },
    draft_save: ({ msg }) => {
      const id = msg.draftId ?? nextId++;
      state.messages = state.messages.filter((m) => m.id !== id);
      state.messages.push(draftMessage(msg, id));
      emit(EVENTS.changed, { mailboxIds: [] });
      return id;
    },
    draft_delete: ({ id }) => {
      state.messages = state.messages.filter((m) => m.id !== id);
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    update_check: () =>
      typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('guncelleme')
        ? { version: '0.2.0', notes: 'JMAP ve Google girişi.', date: '2026-10-20' }
        : null,
    update_install: async () => {
      await new Promise((r) => setTimeout(r, 600));
      return null;
    },
    demo_notify: () => {
      const m = state.messages.find((x) => x.subject === 'Cumartesi kahvaltı?') ?? state.messages[0];
      if (m) {
        const a = state.accounts.find((x) => x.id === m.accountId);
        emit(EVENTS.notify, { message: summary(m), accountName: a?.name ?? '', accountColor: a?.color ?? '#4C6FD8' });
      }
      return null;
    },
  };

  return {
    state,
    emit,
    async call(name, args) {
      // Gerçek arka uç gibi eşzamansız dön.
      await Promise.resolve();
      const fn = commands[name] as (a: unknown) => unknown;
      // Gerçek IPC gibi argümanları ve sonucu JSON üzerinden kopyala.
      const result = await fn(JSON.parse(JSON.stringify(args ?? {})));
      return (result === undefined ? null : JSON.parse(JSON.stringify(result))) as never;
    },
    on(event, handler) {
      let set = handlers.get(event);
      if (!set) handlers.set(event, (set = new Set()));
      set.add(handler as Handler);
      return () => set.delete(handler as Handler);
    },
  };
}
