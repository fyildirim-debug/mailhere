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
  type FollowUp,
  type Mailbox,
  type MailboxRole,
  type MessageDetail,
  type MessageSummary,
  type Rule,
  type RuleCondition,
  type ScheduledItem,
  type ScreenedSender,
  type ServerConfig,
  type Settings,
  type Signature,
  type Template,
  type Vacation,
  type View,
  QUICK_REPLIES_MAX,
  QUICK_REPLY_CHARS,
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
  state: {
    accounts: Account[];
    mailboxes: Mailbox[];
    messages: MessageDetail[];
    settings: Settings;
    rules: Rule[];
    /** Yanıt hatırlatmaları: posta kimliği → an. */
    followUps: Record<number, { at: number; createdAt: number }>;
    /** Elekte bekleyen posta kimlikleri. */
    screened: MessageId[];
    /** Bilinen göndericiler: "hesap|adres". */
    known: string[];
  };
}

const SETTINGS_KEY = 'mailhere.mock.settings';

function fail(code: AppError['code'], message: string): never {
  throw { code, message } satisfies AppError;
}

function lower(s: string): string {
  return s.toLocaleLowerCase('tr');
}

function summary(m: MessageDetail, followUp: number | null = m.followUp): MessageSummary {
  const {
    id, accountId, mailboxId, threadId, from, to, subject, snippet, date, unread, starred, hasAttachments, category, snoozedUntil,
  } = m;
  return { id, accountId, mailboxId, threadId, from, to, subject, snippet, date, unread, starred, hasAttachments, category, snoozedUntil, followUp };
}

/** Arka uçtaki `normalize_settings` ile aynı kurallar. */
export function cleanQuickReplies(list: string[] | null): string[] | null {
  if (list == null) return null;
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of list) {
    // eslint-disable-next-line no-control-regex
    const t = [...raw.replace(/[\r\n\t]+/g, ' ').replace(/[\u0000-\u001f\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, '').trim()].slice(0, QUICK_REPLY_CHARS).join('').trim();
    const key = t.toLocaleLowerCase('tr');
    if (!t || seen.has(key)) continue;
    seen.add(key);
    out.push(t);
    if (out.length === QUICK_REPLIES_MAX) break;
  }
  return out;
}

let uid = 0;
function newId(prefix: string): string {
  uid += 1;
  return `${prefix}-${Date.now().toString(36)}-${uid}`;
}

function cleanTemplates(list: Template[]): Template[] {
  const seen = new Set<string>();
  const out: Template[] = [];
  for (const t of list.slice(0, 50)) {
    const name = t.name.trim().slice(0, 80);
    const body = t.body.slice(0, 20_000);
    if (!name && !body.trim()) continue;
    const id = t.id && !seen.has(t.id) ? t.id : newId('sablon');
    seen.add(id);
    out.push({ id, name: name || 'Şablon', subject: t.subject.replace(/\s+/g, ' ').trim().slice(0, 300), body });
  }
  return out;
}

function cleanSignatures(list: Signature[]): Signature[] {
  if (list.length > 20) fail('invalid', 'Bir hesapta en çok 20 imza olabilir.');
  const seen = new Set<string>();
  return list.map((sig, i) => {
    const id = sig.id || newId('imza');
    if (seen.has(id)) fail('invalid', 'İki imza aynı kimliği taşıyor. Sayfayı yenileyip yeniden dene.');
    seen.add(id);
    return { id, name: sig.name.trim().slice(0, 60) || `İmza ${i + 1}`, body: sig.body.replace(/\s+$/, '').slice(0, 10_000) };
  });
}

/** Kural koşulu (arka uçtaki gibi Türkçe katlamalı, büyük/küçük harf duyarsız). */
function matches(m: MessageDetail, c: RuleCondition): boolean {
  const fold = (x: string) => x.toLocaleLowerCase('tr');
  switch (c.kind) {
    case 'hasAttachment':
      return m.hasAttachments;
    case 'category':
      return m.category === c.value;
    default: {
      const hay =
        c.kind === 'from'
          ? [`${m.from.name ?? ''} ${m.from.email}`]
          : c.kind === 'to'
            ? [...m.to, ...m.cc].map((a) => `${a.name ?? ''} ${a.email}`)
            : c.kind === 'subject'
              ? [m.subject]
              : [m.bodyText ?? m.snippet];
      const v = fold(c.value.trim());
      if (c.op === 'is') {
        const exact = c.kind === 'from' ? [m.from.email] : c.kind === 'to' ? [...m.to, ...m.cc].map((a) => a.email) : hay;
        return exact.some((h) => fold(h.trim()) === v);
      }
      const hit = hay.some((h) => fold(h).includes(v));
      return c.op === 'contains' ? hit : !hit;
    }
  }
}

function sameServer(a: ServerConfig | null, b: ServerConfig): boolean {
  return !!a && a.host.trim().toLowerCase() === b.host.trim().toLowerCase() && a.port === b.port && a.username.trim() === b.username.trim();
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
    rules: opts.empty
      ? []
      : ([
          {
            id: 'kural-bulten', name: 'Bültenler sessiz', enabled: true, accountId: null, match: 'all',
            conditions: [{ kind: 'category', value: 'newsletters' }], actions: [{ kind: 'mute' }], stop: false,
          },
          {
            id: 'kural-fatura', name: 'Faturalar klasöre', enabled: true, accountId: 'demo-kisisel', match: 'any',
            conditions: [{ kind: 'from', op: 'contains', value: 'fatura@' }, { kind: 'subject', op: 'contains', value: 'fatura' }],
            actions: [{ kind: 'move', mailboxId: 'demo-kisisel:Faturalar' }, { kind: 'markRead' }], stop: true,
          },
        ] satisfies Rule[]) as Rule[],
    followUps: {} as Record<number, { at: number; createdAt: number }>,
    screened: [] as MessageId[],
    known: [] as string[],
  };
  const scheduled = new Map<string, { item: ScheduledItem; msg: OutgoingMessage; timer: ReturnType<typeof setTimeout> }>();
  const vacations = new Map<string, Vacation>();
  // Sunucudan bulunan (otomatik) roller: elle atama sıfırlanınca geri gelir.
  const autoRole = new Map(state.mailboxes.map((b) => [b.id, b.role]));
  const handlers = new Map<string, Set<Handler>>();
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

  /** Açık yanıt hatırlatmasının anı (yoksa null). */
  function followUpOf(id: MessageId): number | null {
    return state.followUps[id]?.at ?? null;
  }

  function isScreened(id: MessageId): boolean {
    return state.screened.includes(id);
  }

  function inView(m: MessageDetail, view: View): boolean {
    const role = roleOf(m.mailboxId);
    const snoozed = m.snoozedUntil != null && m.snoozedUntil > now();
    switch (view.kind) {
      case 'inbox':
        // Elekte bekleyenler gelen kutusunda görünmez.
        return role === 'inbox' && !snoozed && !isScreened(m.id);
      case 'snoozed':
        return snoozed;
      case 'starred':
        return m.starred && role !== 'trash';
      case 'mailbox':
        return m.mailboxId === view.id;
      case 'followups':
        return followUpOf(m.id) != null && role !== 'trash' && role !== 'junk';
      case 'screener':
        return isScreened(m.id);
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
      scheduled: scheduled.size,
      perAccountUnread,
      followUps: state.messages.filter((m) => inView(m, { kind: 'followups' })).length,
      screener: state.messages.filter((m) => isScreened(m.id)).length,
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
    return structuredClone({ ...m, thread, remoteAllowed: allowed, bodyHtml, followUp: followUpOf(m.id) });
  }

  /** Süresi dolan hatırlatmaların postaları, gelen kutusunun ilk sayfasının üstüne (en eski önce). */
  function dueFollowUps(accountId: string | null, category: string | null): MessageSummary[] {
    return state.messages
      .filter((m) => inView(m, { kind: 'followups' }) && followUpOf(m.id)! <= now())
      .filter((m) => !accountId || m.accountId === accountId)
      .filter((m) => !category || m.category === category)
      .sort((a, b) => followUpOf(a.id)! - followUpOf(b.id)! || a.id - b.id)
      .map((m) => summary(m, followUpOf(m.id)));
  }

  function knownKey(accountId: string, email: string) {
    return `${accountId}|${lower(email.trim())}`;
  }

  function addMailboxes(accountId: string) {
    for (const [path, name, role] of [
      ['INBOX', 'Gelen Kutusu', 'inbox'],
      ['Sent', 'Gönderilenler', 'sent'],
      ['Drafts', 'Taslaklar', 'drafts'],
      ['Archive', 'Arşiv', 'archive'],
      ['Trash', 'Çöp', 'trash'],
    ] as const) {
      const id = `${accountId}:${path}`;
      state.mailboxes.push({ id, accountId, path, name, role, unread: 0, total: 0, parentId: null, hidden: false });
      autoRole.set(id, role);
    }
  }

  function newAccount(partial: Pick<Account, 'name' | 'email' | 'displayName' | 'kind' | 'imap' | 'smtp' | 'jmapUrl' | 'auth'> & { color: string | null }): Account {
    const colors = ['#4C6FD8', '#2E9A61', '#D08A1E', '#9A5BD6', '#D0566E'];
    const account: Account = {
      ...partial,
      id: `acc-${nextId++}`,
      color: partial.color ?? colors[state.accounts.length % colors.length],
      signatures: [],
      signatureNew: null,
      signatureReply: null,
      createdAt: now(),
      notify: true,
    };
    state.accounts.push(account);
    addMailboxes(account.id);
    // İlk eşitleme: kutular sırayla iner, sonra "çevrimiçi" (gerçek arka uçtaki sıra).
    const id = account.id;
    const total = state.mailboxes.filter((m) => m.accountId === id).length;
    emit(EVENTS.sync, { accountId: id, state: 'syncing', done: 0, total: 0 });
    for (let done = 1; done <= total; done++) {
      setTimeout(() => emit(EVENTS.sync, { accountId: id, state: 'syncing', done, total }), done * 250);
    }
    setTimeout(() => emit(EVENTS.sync, { accountId: id, state: 'online' }), (total + 1) * 250);
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
      hasAttachments: msg.attachments.length > 0, category: 'people', snoozedUntil: null, followUp: null, cc: msg.cc, bcc: msg.bcc, replyTo: [],
      messageIdHeader: null, references: [], bodyText: msg.bodyText, bodyHtml: null, hasRemoteContent: false,
      remoteAllowed: false, attachments: [], thread: [], unsubscribe: null, invite: null,
    };
  }

  function deliver(id: string) {
    const entry = scheduled.get(id);
    if (!entry) return;
    clearTimeout(entry.timer);
    scheduled.delete(id);
    const { msg } = entry;
    const account = state.accounts.find((a) => a.id === msg.accountId);
    if (account) {
      const sentId = nextId++;
      // Yanıtlanan postanın konuşmasında kalır.
      const original = msg.inReplyTo != null ? state.messages.find((m) => m.id === msg.inReplyTo) : undefined;
      state.messages.push({
        id: sentId, accountId: account.id, mailboxId: boxFor(account.id, 'sent'), threadId: original?.threadId ?? `out-${id}`,
        from: { name: account.displayName, email: account.email }, to: msg.to, subject: msg.subject,
        snippet: msg.bodyText.replace(/\s+/g, ' ').slice(0, 200), date: now(), unread: false, starred: false,
        hasAttachments: msg.attachments.length > 0, category: 'people', snoozedUntil: null, followUp: null, cc: msg.cc, bcc: [], replyTo: [],
        messageIdHeader: null, references: [], bodyText: msg.bodyText, bodyHtml: null, hasRemoteContent: false,
        remoteAllowed: false, attachments: [], thread: [], unsubscribe: null, invite: null,
      });
      // Alıcılar bilinen gönderici olur; istenmişse yanıt hatırlatması kurulur.
      for (const a of [...msg.to, ...msg.cc]) {
        const k = knownKey(account.id, a.email);
        if (!state.known.includes(k)) state.known.push(k);
      }
      if (msg.followUpAt != null) state.followUps[sentId] = { at: msg.followUpAt, createdAt: now() };
    }
    emit(EVENTS.outbox, { id, ok: true, error: null });
    emit(EVENTS.changed, { mailboxIds: [] });
  }

  /** Kuralı doğrular (arka uçtaki `rule_save` kuralları). */
  function checkRule(rule: Rule): Rule {
    const name = rule.name.trim().slice(0, 80);
    if (!name) fail('invalid', 'Kurala bir ad ver.');
    if (!rule.conditions.length || rule.conditions.length > 20) fail('invalid', 'Kuralın 1 ile 20 arasında koşulu olmalı.');
    if (!rule.actions.length || rule.actions.length > 10) fail('invalid', 'Kuralın 1 ile 10 arasında eylemi olmalı.');
    for (const c of rule.conditions) {
      if ('value' in c && typeof c.value === 'string' && !c.value.trim()) fail('invalid', 'Koşulların değeri boş olamaz.');
    }
    for (const a of rule.actions) {
      if (a.kind === 'move') {
        if (!rule.accountId) fail('invalid', 'Klasöre taşımak için kurala bir hesap seç.');
        if (!state.mailboxes.some((b) => b.id === a.mailboxId && b.accountId === rule.accountId)) fail('invalid', 'Taşınacak klasör bu hesapta yok.');
      }
      if (a.kind === 'forward' && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(a.to.trim())) fail('invalid', 'İletilecek adres geçerli bir e-posta adresi olmalı.');
    }
    return { ...rule, name };
  }

  const commands: { [K in CommandName]: (args: CommandArgs<K>) => CommandResult<K> | Promise<CommandResult<K>> } = {
    accounts_list: () => structuredClone(state.accounts),
    account_discover: ({ email }) => {
      const domain = email.split('@')[1]?.trim();
      if (!domain || !domain.includes('.')) fail('invalid', 'Geçerli bir e-posta adresi yaz.');
      // "srv-" ile başlayan alan adları DNS'ten başka bir alan adına yönlendirir (testler için).
      const foreign = domain.startsWith('srv-');
      const host = (kind: string) => (foreign ? `${kind}.barindirma.example` : `${kind}.${domain}`);
      return {
        domain,
        imap: { host: host('imap'), port: 993, security: 'tls', username: email },
        smtp: { host: host('smtp'), port: 465, security: 'tls', username: email },
        source: foreign ? 'srv' : 'autoconfig',
        otherDomain: foreign,
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
        .filter((m) => (query.only === 'unread' ? m.unread : query.only === 'starred' ? m.starred : query.only === 'attachments' ? m.hasAttachments : true))
        .sort((a, b) => b.date - a.date || b.id - a.id);
      // Yanıt bekleyenler: hatırlatma anına göre, imleçsiz (küçük liste).
      if (query.view.kind === 'followups') {
        items.sort((a, b) => followUpOf(a.id)! - followUpOf(b.id)! || a.id - b.id);
        return { items: items.map((m) => summary(m, followUpOf(m.id))), nextCursor: null };
      }
      if (query.cursor) {
        const [d, i] = query.cursor.split(':').map(Number);
        items = items.filter((m) => m.date < d || (m.date === d && m.id < i));
      }
      const page = items.slice(0, query.limit);
      const last = page[page.length - 1];
      let out = page.map((m) => summary(m, followUpOf(m.id)));
      // Süresi dolan hatırlatmalar gelen kutusunun ilk sayfasının en üstüne düşer.
      if (query.view.kind === 'inbox' && !query.cursor && !query.only) {
        const due = dueFollowUps(query.accountId, query.category);
        const dup = new Set(due.map((m) => m.id));
        out = [...due, ...out.filter((m) => !dup.has(m.id))];
      }
      return {
        items: out,
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
    messages_move_to: ({ ids, mailboxId }) => {
      const b = state.mailboxes.find((x) => x.id === mailboxId);
      if (!b) fail('notFound', 'Klasör bulunamadı; sunucuda silinmiş ya da yeniden adlandırılmış olabilir.');
      for (const id of ids) if (find(id).accountId !== b.accountId) fail('invalid', 'Posta yalnız kendi hesabının klasörlerine taşınabilir.');
      for (const id of ids) {
        const m = find(id);
        m.mailboxId = b.id;
        m.snoozedUntil = null;
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
      // Arka uçtaki işleçlerin sade karşılığı: from:, to:, konu:, has:ek, is:okunmamış, is:yıldızlı, in:.
      const tests: ((m: MessageDetail) => boolean)[] = [];
      let inBox = false;
      const free = q.replace(/(\S+?):("([^"]*)"|\S+)/g, (all, op: string, _v: string, quoted: string | undefined) => {
        const v = lower(quoted ?? _v);
        const o = lower(op);
        if (o === 'from') tests.push((m) => lower(`${m.from.name ?? ''} ${m.from.email}`).includes(v));
        else if (o === 'to') tests.push((m) => [...m.to, ...m.cc].some((a) => lower(`${a.name ?? ''} ${a.email}`).includes(v)));
        else if (o === 'subject' || o === 'konu') tests.push((m) => lower(m.subject).includes(v));
        else if (o === 'has' && ['ek', 'attachment'].includes(v)) tests.push((m) => m.hasAttachments);
        else if (o === 'is' && ['okunmamış', 'okunmamis', 'unread'].includes(v)) tests.push((m) => m.unread);
        else if (o === 'is' && ['yıldızlı', 'yildizli', 'starred'].includes(v)) tests.push((m) => m.starred);
        else if (o === 'in') {
          inBox = true;
          tests.push((m) => {
            const b = state.mailboxes.find((x) => x.id === m.mailboxId);
            return !!b && (lower(b.name).includes(v) || b.role === v);
          });
        } else return all;
        return ' ';
      });
      const needle = lower(free.trim());
      if (!needle && !tests.length) return [];
      return state.messages
        .filter((m) => inBox || roleOf(m.mailboxId) !== 'trash')
        .filter((m) => tests.every((f) => f(m)))
        .filter((m) => !needle || [m.subject, m.from.name ?? '', m.from.email, m.bodyText ?? ''].some((f) => lower(f).includes(needle)))
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
      const item: ScheduledItem = { id, accountId: msg.accountId, sendAt, to: msg.to, subject: msg.subject, error: null };
      scheduled.set(id, { item, msg, timer: setTimeout(() => deliver(id), Math.max(0, sendAt - now())) });
      if (msg.sendAt) emit(EVENTS.changed, { mailboxIds: [] });
      return { id, sendAt };
    },
    send_cancel: ({ id }) => {
      const entry = scheduled.get(id);
      if (!entry) return false;
      clearTimeout(entry.timer);
      scheduled.delete(id);
      emit(EVENTS.changed, { mailboxIds: [] });
      return true;
    },
    settings_get: () => structuredClone(state.settings),
    settings_set: ({ settings }) => {
      state.settings = { ...settings, templates: cleanTemplates(settings.templates ?? []), quickReplies: cleanQuickReplies(settings.quickReplies) };
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
        // Alıcılar arka uçtaki gibi `?to=Ad <adres>` sorgusuyla taşınır.
        const q = new URLSearchParams();
        for (const a of (prefill.to ?? []).slice(0, 20)) q.append('to', a.name ? `${a.name} <${a.email}>` : a.email);
        const query = q.size ? `?${q}` : '';
        window.open(`#/compose/${prefill.mode}/${prefill.messageId ?? 0}${query}`, `compose-${nextId++}`, 'popup=yes,width=820,height=660');
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
      const signatures = cleanSignatures(update.signatures);
      // Yeni imzalar kimliksiz gelir: varsayılan seçimi listedeki sıraya göre eşle.
      const resolve = (want: string | null) => {
        if (want == null) return null;
        const i = update.signatures.findIndex((x) => x.id === want);
        return i >= 0 ? signatures[i].id : null;
      };
      Object.assign(a, update, { signatures, signatureNew: resolve(update.signatureNew), signatureReply: resolve(update.signatureReply) });
      emit(EVENTS.changed, { mailboxIds: [] });
      return structuredClone(a);
    },
    account_update_server: ({ id, update }) => {
      const a = state.accounts.find((x) => x.id === id);
      if (!a) fail('notFound', 'Hesap bulunamadı.');
      if (a.kind !== 'imap' || a.auth !== 'password') {
        fail('invalid', 'Bu hesabın sunucu ayarları değiştirilemez. Gerekirse hesabı kaldırıp yeniden ekle.');
      }
      for (const [cfg, what] of [[update.imap, 'Gelen posta'], [update.smtp, 'Giden posta']] as const) {
        if (!cfg.host.trim() || !cfg.port || !cfg.username.trim()) fail('invalid', `${what} sunucu adı, kapı ve kullanıcı adı boş olamaz.`);
      }
      const moved = !sameServer(a.imap, update.imap) || !sameServer(a.smtp, update.smtp);
      if (moved && !update.password) fail('invalid', 'Sunucu değişince parolayı yeniden yaz.');
      if (update.password === 'yanlis') fail('auth', 'Sunucu parolayı kabul etmedi. Parolayı ya da uygulama parolasını kontrol et.');
      for (const cfg of [update.imap, update.smtp]) {
        const pinned = state.settings.trustedCerts.some((c) => c.host === cfg.host && c.port === cfg.port);
        if (cfg.host.includes('kendinden') && !pinned) fail('untrusted', `${cfg.host} sunucusunun sertifikası güvenilir bir kuruluşça imzalanmamış.`);
      }
      a.imap = { ...update.imap, host: update.imap.host.trim(), username: update.imap.username.trim() };
      a.smtp = { ...update.smtp, host: update.smtp.host.trim(), username: update.smtp.username.trim() };
      emit(EVENTS.changed, { mailboxIds: [] });
      return structuredClone(a);
    },
    account_set_password: ({ id, password }) => {
      const a = state.accounts.find((x) => x.id === id);
      if (!a) fail('notFound', 'Hesap bulunamadı.');
      if (a.auth !== 'password') fail('invalid', 'Bu hesap tarayıcıyla giriş kullanıyor; parolası yok.');
      if (!password) fail('invalid', 'Parola boş olamaz.');
      if (password === 'yanlis') fail('auth', 'Sunucu parolayı kabul etmedi. Parolayı ya da uygulama parolasını kontrol et.');
      return null;
    },
    mailbox_set_hidden: ({ id, hidden }) => {
      const b = state.mailboxes.find((x) => x.id === id);
      if (!b) fail('notFound', 'Klasör bulunamadı.');
      if (b.role === 'inbox' && hidden) fail('invalid', 'Gelen kutusu gizlenemez.');
      b.hidden = hidden;
      emit(EVENTS.changed, { mailboxIds: [id] });
      return null;
    },
    mailbox_set_role: ({ id, role }) => {
      const b = state.mailboxes.find((x) => x.id === id);
      if (!b) fail('notFound', 'Klasör bulunamadı.');
      if (role === ('inbox' as string) || role === ('other' as string)) fail('invalid', 'Bu görev bir klasöre atanamaz.');
      if (b.role === 'inbox') fail('invalid', 'Gelen kutusunun görevi değiştirilemez.');
      for (const o of state.mailboxes) if (o.accountId === b.accountId && o.role === role) o.role = 'other';
      b.role = role;
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    mailbox_roles_reset: ({ accountId }) => {
      for (const b of state.mailboxes) if (b.accountId === accountId) b.role = autoRole.get(b.id) ?? 'other';
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    rules_list: () => structuredClone(state.rules),
    rule_save: ({ rule }) => {
      if (!rule.id && state.rules.length >= 200) fail('invalid', 'En çok 200 kural olabilir. Kullanmadıklarını sil.');
      const clean = checkRule(rule);
      if (clean.id) {
        const i = state.rules.findIndex((r) => r.id === clean.id);
        if (i < 0) fail('notFound', 'Kural bulunamadı; silinmiş olabilir.');
        state.rules[i] = clean;
      } else {
        clean.id = newId('kural');
        state.rules.push(clean);
      }
      return structuredClone(clean);
    },
    rule_delete: ({ id }) => {
      state.rules = state.rules.filter((r) => r.id !== id);
      return null;
    },
    rules_reorder: ({ ids }) => {
      const order = new Map(ids.map((id, i) => [id, i]));
      state.rules.sort((a, b) => (order.get(a.id) ?? 1e9) - (order.get(b.id) ?? 1e9));
      return null;
    },
    rule_run: ({ id }) => {
      const rule = state.rules.find((r) => r.id === id);
      if (!rule) fail('notFound', 'Kural bulunamadı; silinmiş olabilir.');
      let n = 0;
      for (const m of state.messages) {
        if (roleOf(m.mailboxId) !== 'inbox' || (rule.accountId && m.accountId !== rule.accountId)) continue;
        const hit = rule.match === 'all' ? rule.conditions.every((c) => matches(m, c)) : rule.conditions.some((c) => matches(m, c));
        if (!hit) continue;
        n++;
        for (const a of rule.actions) {
          if (a.kind === 'markRead') m.unread = false;
          else if (a.kind === 'star') m.starred = true;
          else if (a.kind === 'archive') m.mailboxId = boxFor(m.accountId, 'archive');
          else if (a.kind === 'trash') m.mailboxId = boxFor(m.accountId, 'trash');
          else if (a.kind === 'move') m.mailboxId = a.mailboxId;
        }
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return n;
    },
    outbox_list: () => [...scheduled.values()].map((e) => e.item).sort((a, b) => a.sendAt - b.sendAt),
    outbox_send_now: ({ id }) => {
      if (!scheduled.has(id)) fail('notFound', 'Bu gönderim artık giden kutusunda değil.');
      deliver(id);
      return null;
    },
    outbox_to_draft: ({ id }) => {
      const entry = scheduled.get(id);
      if (!entry) fail('notFound', 'Bu gönderim artık giden kutusunda değil.');
      clearTimeout(entry.timer);
      scheduled.delete(id);
      const draftId = nextId++;
      state.messages.push(draftMessage({ ...entry.msg, sendAt: null }, draftId));
      emit(EVENTS.changed, { mailboxIds: [] });
      return draftId;
    },
    unsubscribe: ({ id }) => {
      const u = find(id).unsubscribe;
      if (!u) fail('notFound', 'Bu postada abonelikten çıkma bağlantısı yok.');
      if (u.oneClick && u.url?.startsWith('https://')) return 'done';
      if (u.mailto) return 'done';
      if (u.url) return 'browser';
      fail('notFound', 'Bu postada abonelikten çıkma bağlantısı yok.');
    },
    vacation_get: ({ accountId }) => {
      const a = state.accounts.find((x) => x.id === accountId);
      if (!a) fail('notFound', 'Hesap bulunamadı.');
      if (a.kind !== 'jmap') return null;
      return structuredClone(vacations.get(accountId) ?? { enabled: false, fromDate: null, toDate: null, subject: '', body: '' });
    },
    vacation_set: ({ accountId, vacation }) => {
      const a = state.accounts.find((x) => x.id === accountId);
      if (!a) fail('notFound', 'Hesap bulunamadı.');
      if (a.kind !== 'jmap') fail('invalid', 'Bu hesabın sunucusu otomatik yanıtı desteklemiyor.');
      if (vacation.fromDate != null && vacation.toDate != null && vacation.toDate < vacation.fromDate) fail('invalid', 'Bitiş tarihi başlangıçtan önce olamaz.');
      const v = { ...vacation, subject: vacation.subject.replace(/\s+/g, ' ').trim().slice(0, 300), body: vacation.body.slice(0, 20_000) };
      vacations.set(accountId, v);
      return structuredClone(v);
    },
    quick_replies_set: ({ replies }) => {
      state.settings = { ...state.settings, quickReplies: cleanQuickReplies(replies) };
      emit(EVENTS.settings, structuredClone(state.settings));
      return structuredClone(state.settings);
    },
    cert_inspect: ({ host, port }) => {
      const pinned = state.settings.trustedCerts.some((c) => c.host === host && c.port === port);
      return {
      host,
      port,
      sha256: 'A1:B2:C3:D4:E5:F6:07:18:29:3A:4B:5C:6D:7E:8F:90:01:12:23:34:45:56:67:78:89:9A:AB:BC:CD:DE:EF:F0',
      subject: `CN=${host}`,
      issuer: `CN=${host}`,
      notBefore: now() - 30 * 86_400_000,
      notAfter: now() + 335 * 86_400_000,
      names: [host],
      problem: host.includes('kendinden') && !pinned ? 'kendinden imzalı' : '',
      };
    },
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
    attachments_pick: () => [],
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
    thread_get: ({ id }) => {
      const m = find(id);
      const keep = (x: MessageDetail) => x.id === id || !['trash', 'junk'].includes(roleOf(x.mailboxId));
      return state.messages
        .filter((x) => x.accountId === m.accountId && x.threadId === m.threadId && keep(x))
        .sort((a, b) => a.date - b.date || a.id - b.id)
        .slice(-100)
        .map(detail);
    },
    contact_get: ({ email }) => {
      const e = lower(email.trim());
      if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e)) fail('invalid', 'Adres eksik ya da bozuk; kişi bilgisi gösterilemez.');
      const involves = (m: MessageDetail) => lower(m.from.email) === e || [...m.to, ...m.cc].some((a) => lower(a.email) === e);
      const counted = (m: MessageDetail) => !['trash', 'junk', 'drafts'].includes(roleOf(m.mailboxId));
      const hits = state.messages.filter((m) => counted(m) && involves(m)).sort((a, b) => b.date - a.date || b.id - a.id);
      const named = state.messages.filter((m) => lower(m.from.email) === e && m.from.name).sort((a, b) => b.date - a.date);
      const threads = new Map<string, MessageDetail>();
      for (const m of hits) {
        const k = `${m.accountId}|${m.threadId}`;
        if (!threads.has(k)) threads.set(k, m);
      }
      return { email: e, name: named[0]?.from.name ?? null, total: hits.length, lastDate: hits[0]?.date ?? null, recent: [...threads.values()].slice(0, 5).map(summary) };
    },
    message_save_eml: ({ id }) => {
      const m = find(id);
      return `/tmp/mailhere/${m.subject.replace(/[\\/:*?"<>|]/g, '_').trim() || 'posta'}.eml`;
    },
    invite_reply: ({ id, response }) => {
      const m = find(id);
      if (!m.invite) fail('notFound', 'Bu postada takvim daveti yok.');
      if (m.invite.method !== 'request') fail('invalid', 'Bu etkinlik iptal edildi; yanıt gönderilmez.');
      if (!m.invite.organizer) fail('invalid', 'Davette düzenleyenin adresi yok; yanıt gönderilemez. Gönderene doğrudan yazabilirsin.');
      const account = state.accounts.find((a) => a.id === m.accountId);
      if (!account) fail('notFound', 'Postayı alan hesap bulunamadı.');
      const prefix = state.settings.language === 'en'
        ? { accepted: 'Accepted', tentative: 'Tentative', declined: 'Declined' }[response]
        : { accepted: 'Kabul edildi', tentative: 'Belki', declined: 'Katılmıyor' }[response];
      state.messages.push({
        id: nextId++, accountId: account.id, mailboxId: boxFor(account.id, 'sent'), threadId: m.threadId,
        from: { name: account.displayName, email: account.email }, to: [m.invite.organizer], subject: `${prefix}: ${m.invite.summary}`,
        snippet: `${account.displayName} daveti yanıtladı: ${m.invite.summary}`, date: now(), unread: false, starred: false,
        hasAttachments: false, category: 'people', snoozedUntil: null, cc: [], bcc: [], replyTo: [],
        messageIdHeader: null, references: [], bodyText: `${account.displayName} daveti yanıtladı: ${m.invite.summary}`, bodyHtml: null,
        hasRemoteContent: false, remoteAllowed: false, attachments: [], thread: [], unsubscribe: null, invite: null, followUp: null,
      });
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    invite_open: ({ id }) => {
      const m = find(id);
      if (!m.invite) fail('notFound', 'Bu postada takvim daveti yok.');
      return `/tmp/mailhere/${m.invite.summary || 'davet'}.ics`;
    },
    demo_notify: () => {
      const m = state.messages.find((x) => x.subject === 'Cumartesi kahvaltı?') ?? state.messages[0];
      if (m) {
        const a = state.accounts.find((x) => x.id === m.accountId);
        emit(EVENTS.notify, { message: summary(m), accountName: a?.name ?? '', accountColor: a?.color ?? '#4C6FD8' });
      }
      return null;
    },
    followup_set: ({ id, at }) => {
      const m = find(id);
      if (at == null) {
        delete state.followUps[id];
      } else {
        if (roleOf(m.mailboxId) !== 'sent') fail('invalid', 'Yanıt hatırlatması yalnız gönderilmiş postalara kurulabilir.');
        if (at <= now()) fail('invalid', 'Hatırlatma anı ileride olmalı.');
        state.followUps[id] = { at, createdAt: now() };
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    followups_list: () =>
      state.messages
        .filter((m) => inView(m, { kind: 'followups' }))
        .map((m): FollowUp => ({ id: m.id, accountId: m.accountId, at: state.followUps[m.id].at, createdAt: state.followUps[m.id].createdAt, subject: m.subject, to: m.to }))
        .sort((a, b) => a.at - b.at || a.id - b.id),
    screener_list: () => {
      const groups = new Map<string, ScreenedSender>();
      for (const m of [...state.messages].sort((a, b) => b.date - a.date || b.id - a.id)) {
        if (!isScreened(m.id)) continue;
        const key = knownKey(m.accountId, m.from.email);
        const g = groups.get(key);
        if (g) g.count++;
        else groups.set(key, { accountId: m.accountId, email: lower(m.from.email), name: m.from.name?.trim() || null, count: 1, latest: m.date, subject: m.subject });
      }
      return [...groups.values()].sort((a, b) => b.latest - a.latest);
    },
    screener_decide: ({ email, accountId, allow }) => {
      if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim())) fail('invalid', 'Gönderici adresi eksik ya da bozuk.');
      if (!state.accounts.some((a) => a.id === accountId)) fail('notFound', 'Hesap bulunamadı; kaldırılmış olabilir.');
      const key = knownKey(accountId, email);
      const ids = state.messages.filter((m) => isScreened(m.id) && knownKey(m.accountId, m.from.email) === key).map((m) => m.id);
      state.screened = state.screened.filter((id) => !ids.includes(id));
      if (allow) {
        if (!state.known.includes(key)) state.known.push(key);
      } else {
        for (const id of ids) find(id).mailboxId = boxFor(accountId, 'trash');
      }
      emit(EVENTS.changed, { mailboxIds: [] });
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
