// Ana pencerenin durumu: kutu, süzgeç, liste, seçim ve okuma.
// Eylemler iyimserdir: liste hemen güncellenir, arka uç ardından çağrılır.

import {
  call,
  EVENTS,
  on,
  openExternal,
  toAppError,
  type Account,
  type Address,
  type Category,
  type Counts,
  type ListOnly,
  type Mailbox,
  type MessageDetail,
  type MessageId,
  type MessageSummary,
  type MoveTarget,
  type Rule,
  type SyncStatus,
  type View,
} from '../api';
import { t, when } from '../i18n/index.svelte';
import { showToast } from '../ui/toast.svelte';
import { baseSubject, displayName, replyTargets } from '../util/format';
import { settings } from './settings.svelte';
import { loadTags } from './tags.svelte';

export type Screen = 'mail' | 'settings' | 'onboarding' | 'scheduled';

/** Ayarlar sayfasının bölümü; hesap sayfası `account:<kimlik>`. */
export type SettingsSection =
  | 'general'
  | 'notifications'
  | 'compose'
  | 'accounts'
  | 'rules'
  | 'tags'
  | 'devices'
  | 'privacy'
  | 'about'
  | `account:${string}`;

/** Hesap sayfasının sekmeleri. */
export type AccountTab = 'general' | 'signatures' | 'server' | 'folders' | 'vacation';

const PAGE = 100;

export const mail = $state({
  ready: false,
  screen: 'mail' as Screen,
  accounts: [] as Account[],
  mailboxes: [] as Mailbox[],
  counts: { inboxUnread: 0, snoozed: 0, drafts: 0, scheduled: 0, perAccountUnread: {}, followUps: 0, screener: 0 } as Counts,
  sync: {} as Record<string, SyncStatus>,

  view: { kind: 'inbox' } as View,
  accountId: null as string | null,
  category: null as Category | null,
  /** Listede hızlı süzgeç (okunmamış, yıldızlı, ekli). */
  only: null as ListOnly | null,
  settingsSection: 'general' as SettingsSection,
  /** Hesap sayfası açılınca gösterilecek sekme (tek seferlik; sayfa okuyup sıfırlar). */
  accountTab: null as AccountTab | null,
  /** Kurallar bölümü açılınca düzenleyicide gösterilecek taslak (postadan kural). */
  ruleDraft: null as Rule | null,

  items: [] as MessageSummary[],
  nextCursor: null as string | null,
  loading: false,

  selectedId: null as MessageId | null,
  /** Çoklu seçim: işaretli satırların kimlikleri (satır = konuşmanın en yeni iletisi). Mantık `selection.svelte.ts`. */
  checked: [] as MessageId[],
  detail: null as MessageDetail | null,
  detailLoading: false,
  detailError: null as string | null,

  paletteOpen: false,
  /** Etkin tam metin arama; null ise normal görünüm. */
  search: null as string | null,
});

/** Listede görünen satır: bir konuşmanın en yeni iletisi. */
export interface Row extends MessageSummary {
  /** Bu satırın kapsadığı tüm ileti kimlikleri (konuşma). */
  ids: MessageId[];
  count: number;
}

/** Aynı konuşmadaki iletileri en yenisinde toplar (arama sonuçları hariç). */
export function rowsOf(items: MessageSummary[], collapse = true): Row[] {
  if (!collapse) return items.map((m) => ({ ...m, ids: [m.id], count: 1 }));
  const byThread = new Map<string, Row>();
  const out: Row[] = [];
  for (const m of items) {
    const key = `${m.accountId}|${m.threadId}`;
    const row = byThread.get(key);
    if (row) {
      row.ids.push(m.id);
      row.count++;
      row.unread ||= m.unread;
      row.starred ||= m.starred;
      row.hasAttachments ||= m.hasAttachments;
    } else {
      const r = { ...m, ids: [m.id], count: 1 };
      byThread.set(key, r);
      out.push(r);
    }
  }
  return out;
}

export function currentRows(): Row[] {
  return rowsOf(mail.items, mail.search == null);
}

/** Seçili iletiyi içeren satırın tüm kimlikleri. */
export function selectedIds(): MessageId[] {
  const id = mail.selectedId;
  if (id == null) return [];
  return currentRows().find((r) => r.ids.includes(id))?.ids ?? [id];
}

let listToken = 0;
let detailToken = 0;

function errorToast(e: unknown) {
  showToast({ text: t('toast.error', { message: toAppError(e).message }) });
}

// ------------------------------------------------------------ yükleme

export async function initMail() {
  on(EVENTS.changed, () => scheduleRefresh());
  on(EVENTS.sync, (s) => (mail.sync = { ...mail.sync, [s.accountId]: s }));
  on(EVENTS.open, (p) => void openById(p.id));
  void loadTags();
  await reloadAccounts();
  mail.ready = true;
}

export async function reloadAccounts() {
  const [accounts, mailboxes] = await Promise.all([call('accounts_list'), call('mailboxes_list')]);
  mail.accounts = accounts;
  mail.mailboxes = mailboxes;
  if (!accounts.length) {
    mail.screen = 'onboarding';
    mail.items = [];
    mail.detail = null;
    mail.selectedId = null;
    return;
  }
  await Promise.all([refreshList(), refreshCounts()]);
}

export async function refreshCounts() {
  try {
    mail.counts = await call('counts');
  } catch {
    /* sayaç yenilenemezse eski değer kalır */
  }
}

export async function refreshList(keepSelection = true) {
  const token = ++listToken;
  mail.loading = true;
  try {
    if (mail.search != null) {
      const items = await call('search', { q: mail.search, limit: 300 });
      if (token !== listToken) return;
      mail.items = items;
      mail.nextCursor = null;
      pruneChecked();
      if ((!keepSelection || mail.selectedId == null) && items.length) void select(items[0].id, { markRead: false });
      return;
    }
    const page = await call('messages_list', {
      query: { view: mail.view, accountId: mail.accountId, category: mail.category, only: mail.only, cursor: null, limit: PAGE },
    });
    if (token !== listToken) return;
    mail.items = page.items;
    mail.nextCursor = page.nextCursor;
    pruneChecked();
    // Seçili posta bu görünümde yoksa okuma alanı açık kalır; liste yalnız vurguyu kaybeder.
    if ((!keepSelection || mail.selectedId == null) && page.items.length) void select(page.items[0].id, { markRead: false });
  } catch (e) {
    errorToast(e);
  } finally {
    if (token === listToken) mail.loading = false;
  }
}

export async function loadMore() {
  if (!mail.nextCursor || mail.loading) return;
  const token = listToken;
  mail.loading = true;
  try {
    const page = await call('messages_list', {
      query: { view: mail.view, accountId: mail.accountId, category: mail.category, only: mail.only, cursor: mail.nextCursor, limit: PAGE },
    });
    if (token !== listToken) return;
    const known = new Set(mail.items.map((m) => m.id));
    mail.items = [...mail.items, ...page.items.filter((m) => !known.has(m.id))];
    mail.nextCursor = page.nextCursor;
  } catch (e) {
    errorToast(e);
  } finally {
    mail.loading = false;
  }
}

/** Liste yenilenince artık satır olmayan işaretler düşer; yoksa toplu işlem paneli boş kalırdı. */
function pruneChecked() {
  if (!mail.checked.length) return;
  const rows = new Set(currentRows().map((r) => r.id));
  if (mail.checked.some((id) => !rows.has(id))) mail.checked = mail.checked.filter((id) => rows.has(id));
}

let refreshTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleRefresh() {
  if (refreshTimer) clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => {
    void refreshList();
    void refreshCounts();
    void reloadMailboxes();
  }, 150);
}

export async function reloadMailboxes() {
  try {
    mail.mailboxes = await call('mailboxes_list');
  } catch {
    /* yoksay */
  }
}

// ---------------------------------------------------------- gezinme

export function setView(view: View, accountId: string | null = null) {
  mail.search = null;
  mail.view = view;
  mail.accountId = accountId;
  mail.screen = 'mail';
  mail.selectedId = null;
  mail.checked = [];
  mail.detail = null;
  void refreshList(false);
}

/** Tüm postalarda arar; sonuçlar listede görünür. */
export function searchAll(q: string) {
  mail.search = q.trim() || null;
  mail.screen = 'mail';
  mail.selectedId = null;
  mail.checked = [];
  mail.detail = null;
  void refreshList(false);
}

export function setCategory(category: Category | null) {
  mail.category = category;
  mail.checked = [];
  void refreshList();
}

export function setOnly(only: ListOnly | null) {
  mail.only = mail.only === only ? null : only;
  mail.checked = [];
  void refreshList(false);
}

/** Ayarları belirli bir bölümde açar. */
export function openSettings(section: SettingsSection = mail.settingsSection) {
  mail.settingsSection = section;
  mail.screen = 'settings';
}

/** Hesabın "Sunucu ve parola" sekmesini açar (ör. aktarımdan parolasız gelen hesap). */
export function openAccountPassword(id: string) {
  mail.accountTab = 'server';
  openSettings(`account:${id}`);
}

/** Kurallar bölümünü bu taslakla açar. */
export function newRuleFrom(draft: Rule) {
  mail.ruleDraft = draft;
  openSettings('rules');
}

/** Gönderenin postaları bundan sonra çöpe gider ve bildirim göstermez. */
export async function blockSender(accountId: string, email: string) {
  try {
    const rule = await call('rule_save', {
      rule: {
        id: '',
        name: t('rule.blockedName', { email }),
        enabled: true,
        accountId,
        match: 'all',
        conditions: [{ kind: 'from', op: 'is', value: email }],
        actions: [{ kind: 'trash' }, { kind: 'mute' }],
        stop: true,
      },
    });
    showToast({
      text: t('toast.blocked', { email }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await call('rule_delete', { id: rule.id });
        },
      },
    });
  } catch (e) {
    errorToast(e);
  }
}

/** Hesabın yeni posta ya da yanıt imzası (yoksa null). */
export function signatureFor(account: Account | undefined, mode: 'new' | 'reply'): string | null {
  if (!account) return null;
  const id = mode === 'new' ? account.signatureNew : account.signatureReply;
  const body = account.signatures.find((s) => s.id === id)?.body.trimEnd();
  return body ? body : null;
}

export async function select(id: MessageId, opts: { markRead?: boolean } = {}) {
  const markRead = opts.markRead ?? true;
  mail.selectedId = id;
  const token = ++detailToken;
  mail.detailError = null;
  if (mail.detail?.id !== id) mail.detailLoading = true;
  try {
    const d = await call('message_get', { id });
    if (token !== detailToken) return;
    mail.detail = d;
    if (markRead && d.unread) await setUnread([id], false, { quiet: true });
  } catch (e) {
    if (token === detailToken) {
      mail.detail = null;
      mail.detailError = toAppError(e).message;
    }
  } finally {
    if (token === detailToken) mail.detailLoading = false;
  }
}

async function openById(id: MessageId) {
  mail.screen = 'mail';
  await select(id);
}

/** Klavyeyle satırlar arasında gezinir; okundu saymaz (Enter sayar). */
export function move(delta: 1 | -1) {
  const rows = currentRows();
  if (!rows.length) return;
  const i = rows.findIndex((r) => mail.selectedId != null && r.ids.includes(mail.selectedId));
  const next = rows[Math.min(Math.max(i + delta, 0), rows.length - 1)] ?? rows[0];
  if (next && next.id !== mail.selectedId) void select(next.id, { markRead: false });
}

// ---------------------------------------------------------- eylemler

export function patchLocal(ids: MessageId[], patch: Partial<MessageSummary>) {
  const set = new Set(ids);
  mail.items = mail.items.map((m) => (set.has(m.id) ? { ...m, ...patch } : m));
  if (mail.detail && set.has(mail.detail.id)) mail.detail = { ...mail.detail, ...patch };
}

/** Postaları listeden çıkarır; seçili olan çıkarsa sıradakine geçer. Çoklu seçimden de düşer. */
export function takeOut(ids: MessageId[]) {
  const set = new Set(ids);
  const before = currentRows();
  const idx = before.findIndex((r) => r.ids.some((x) => set.has(x)));
  mail.items = mail.items.filter((m) => !set.has(m.id));
  if (mail.checked.length) mail.checked = mail.checked.filter((id) => !set.has(id));
  if (mail.selectedId != null && set.has(mail.selectedId)) {
    const keep = (r: Row) => !r.ids.every((x) => set.has(x));
    const rest = before.slice(idx + 1).find(keep) ?? [...before.slice(0, Math.max(idx, 0))].reverse().find(keep);
    mail.selectedId = null;
    mail.detail = null;
    if (rest) void select(rest.ids.find((x) => !set.has(x)) ?? rest.id, { markRead: false });
  }
}

export async function setUnread(ids: MessageId[], unread: boolean, opts: { quiet?: boolean } = {}) {
  patchLocal(ids, { unread });
  try {
    await call('messages_flag', { ids, unread, starred: null });
    void refreshCounts();
  } catch (e) {
    patchLocal(ids, { unread: !unread });
    if (!opts.quiet) errorToast(e);
  }
}

/**
 * Yıldızı çevirir; sonucu döndürür. Listede ve okuma alanında olmayan ileti
 * (konuşmadaki eski ileti) için `current` verilmelidir, yoksa hep yıldızlanır.
 */
export async function toggleStar(id: MessageId, current?: boolean): Promise<boolean> {
  const m = mail.items.find((x) => x.id === id) ?? (mail.detail?.id === id ? mail.detail : null);
  const starred = !(current ?? m?.starred ?? false);
  patchLocal([id], { starred });
  try {
    await call('messages_flag', { ids: [id], unread: null, starred });
    if (mail.view.kind === 'starred' && !starred) takeOut([id]);
    return starred;
  } catch (e) {
    patchLocal([id], { starred: !starred });
    errorToast(e);
    return !starred;
  }
}

export async function moveTo(ids: MessageId[], target: MoveTarget) {
  if (!ids.length) return;
  const selected = mail.selectedId;
  takeOut(ids);
  try {
    await call('messages_move', { ids, target });
    void refreshCounts();
    showToast({
      text: t(target === 'archive' ? 'toast.archived' : 'toast.deleted'),
      action: {
        label: t('act.undo'),
        run: async () => {
          await call('messages_move', { ids, target: 'inbox' });
          await refreshList();
          if (selected != null) void select(selected, { markRead: false });
          void refreshCounts();
        },
      },
    });
  } catch (e) {
    errorToast(e);
    void refreshList();
  }
}

/** Postaları bir klasöre taşır; geri alınca eski klasörlerine döner. */
export async function moveToFolder(ids: MessageId[], mailbox: Mailbox, label: string) {
  if (!ids.length) return;
  const selected = mail.selectedId;
  const from = new Map(mail.items.filter((m) => ids.includes(m.id)).map((m) => [m.id, m.mailboxId]));
  if (mail.detail && ids.includes(mail.detail.id)) from.set(mail.detail.id, mail.detail.mailboxId);
  takeOut(ids);
  try {
    await call('messages_move_to', { ids, mailboxId: mailbox.id });
    void refreshCounts();
    void reloadMailboxes();
    showToast({
      text: t('toast.movedTo', { folder: label }),
      action: {
        label: t('act.undo'),
        run: async () => {
          // Her posta geldiği klasöre döner.
          const back = new Map<string, MessageId[]>();
          for (const id of ids) {
            const b = from.get(id);
            if (b) back.set(b, [...(back.get(b) ?? []), id]);
          }
          for (const [box, list] of back) await call('messages_move_to', { ids: list, mailboxId: box });
          await refreshList();
          if (selected != null) void select(selected, { markRead: false });
          void refreshCounts();
        },
      },
    });
  } catch (e) {
    errorToast(e);
    void refreshList();
  }
}

/** Bültenden çıkar: arka uç yapabiliyorsa yapar, yoksa bağlantıyı tarayıcıda açar. */
export async function unsubscribe(d: MessageDetail) {
  if (!d.unsubscribe) return;
  try {
    const r = await call('unsubscribe', { id: d.id });
    if (r === 'browser' && d.unsubscribe.url) {
      await openExternal(d.unsubscribe.url);
      showToast({ text: t('toast.unsubBrowser') });
    } else {
      showToast({
        text: t('toast.unsubscribed', { name: displayName(d.from) }),
        action: { label: t('read.archiveToo'), run: () => moveTo([d.id], 'archive') },
      });
    }
  } catch (e) {
    errorToast(e);
  }
}

export async function snooze(ids: MessageId[], until: number) {
  if (!ids.length) return;
  const selected = mail.selectedId;
  takeOut(ids);
  try {
    await call('messages_snooze', { ids, until });
    void refreshCounts();
    showToast({
      text: t('toast.snoozed', { when: when(until) }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await call('messages_snooze', { ids, until: null });
          await refreshList();
          if (selected != null) void select(selected, { markRead: false });
          void refreshCounts();
        },
      },
    });
  } catch (e) {
    errorToast(e);
    void refreshList();
  }
}

export async function markAllRead() {
  const ids = mail.items.filter((m) => m.unread).map((m) => m.id);
  if (!ids.length) return;
  await setUnread(ids, false);
  showToast({
    text: t('toast.markedRead'),
    action: { label: t('act.undo'), run: () => setUnread(ids, true) },
  });
}

/** Uzak içeriğe izin verir; güncel iletiyi döndürür (hata olursa null). */
export async function allowRemote(id: MessageId): Promise<MessageDetail | null> {
  try {
    const d = await call('remote_allow', { id });
    if (mail.detail?.id === id) mail.detail = d;
    showToast({ text: t('toast.remoteAllowed', { name: displayName(d.from) }) });
    return d;
  } catch (e) {
    errorToast(e);
    return null;
  }
}

/** Alıntılı yanıt metni oluşturur. */
export function quoteFor(d: MessageDetail, text: string): string {
  const quoted = (d.bodyText ?? d.snippet)
    .split('\n')
    .map((l) => `> ${l}`)
    .join('\n');
  return `${text.trim()}\n\n${t('compose.wrote', { date: when(d.date), name: displayName(d.from) })}\n${quoted}\n`;
}

/**
 * Hızlı yanıtın alıcıları: Reply-To ya da gönderen, hesabın kendi adresi
 * çıkarılmış. Kendi gönderdiğimiz postada (Gönderilenler, "Yanıt bekleniyor")
 * kimse kalmazsa postanın alıcılarına gider. Yazma penceresindeki `prefill`
 * ile aynı kural; en kötü durumda yine yanıt adresleri.
 */
export function quickReplyTargets(
  d: Pick<MessageDetail, 'accountId' | 'from' | 'replyTo' | 'to'>,
  account: Account | null | undefined = mail.accounts.find((a) => a.id === d.accountId),
): Address[] {
  const me = account?.email.toLowerCase();
  const notMe = (a: Address) => a.email.toLowerCase() !== me;
  const targets = replyTargets(d);
  const others = targets.filter(notMe);
  if (others.length) return others;
  const to = d.to.filter(notMe);
  return to.length ? to : targets;
}

/** Hızlı yanıtı giden kutusuna koyar; geri alınırsa metni geri verir. */
export async function sendQuickReply(d: MessageDetail, text: string, restore: (text: string) => void): Promise<boolean> {
  const account = mail.accounts.find((a) => a.id === d.accountId);
  const to = quickReplyTargets(d, account);
  try {
    const item = await call('send', {
      msg: {
        accountId: d.accountId,
        to,
        cc: [],
        bcc: [],
        subject: `Re: ${baseSubject(d.subject)}`,
        bodyText: quoteFor(d, signatureFor(account, 'reply') ? `${text}\n\n${signatureFor(account, 'reply')}` : text),
        inReplyTo: d.id,
        attachments: [],
        sendAt: null,
      },
    });
    const seconds = Math.max(1, Math.round((item.sendAt - Date.now()) / 1000));
    showToast({
      text: t('toast.queued', { n: seconds }),
      duration: Math.max(4000, item.sendAt - Date.now()),
      action: {
        label: t('act.undo'),
        run: async () => {
          if (await call('send_cancel', { id: item.id })) restore(text);
        },
      },
    });
    return true;
  } catch (e) {
    showToast({ text: t('toast.sendFailed', { message: toAppError(e).message }) });
    return false;
  }
}

/** Gönderilmiş postaya yanıt hatırlatması kurar (`at`) ya da kaldırır (null). */
export async function setFollowUp(id: MessageId, at: number | null) {
  try {
    await call('followup_set', { id, at });
    patchLocal([id], { followUp: at });
    // Kaldırılan hatırlatma "Yanıt bekleyenler"den ve gelen kutusunun üstünden çıkar.
    if (at == null && (mail.view.kind === 'followups' || mail.view.kind === 'inbox')) takeOut([id]);
    void refreshCounts();
    showToast({ text: at == null ? t('follow.cleared') : t('follow.setToast', { when: when(at) }) });
  } catch (e) {
    errorToast(e);
  }
}

/** Elekten karar: kabul edilen gönderici gelen kutusuna döner; engellenen çöpe gider ve kural kurulur. */
export async function screenerDecide(accountId: string, email: string, allow: boolean) {
  try {
    await call('screener_decide', { email, accountId, allow });
    if (allow) showToast({ text: t('screen.allowed', { email }) });
    else await blockSender(accountId, email);
    void refreshList(false);
    void refreshCounts();
  } catch (e) {
    errorToast(e);
  }
}

export function syncLabel(): { text: string; tone: 'ok' | 'busy' | 'warn'; untrusted?: { host: string; port: number } } {
  const n = mail.accounts.length;
  if (!n) return { text: t('sync.noAccounts'), tone: 'warn' };
  const states = Object.values(mail.sync).filter((s) => mail.accounts.some((a) => a.id === s.accountId));
  const bad = states.find((s) => s.state === 'untrusted');
  if (bad && bad.state === 'untrusted') {
    return { text: t('sync.untrusted', { host: bad.host }), tone: 'warn', untrusted: { host: bad.host, port: bad.port } };
  }
  const err = states.find((s) => s.state === 'error');
  if (err) return { text: t('sync.error'), tone: 'warn' };
  const busy = states.find((s) => s.state === 'syncing');
  if (busy && busy.state === 'syncing') {
    return { text: busy.total ? t('sync.syncingProgress', { done: busy.done, total: busy.total }) : t('sync.syncing'), tone: 'busy' };
  }
  if (states.length && states.every((s) => s.state === 'offline')) return { text: t('sync.offline'), tone: 'warn' };
  return { text: t('sync.online', { n }), tone: 'ok' };
}

export function density() {
  return settings.value.density;
}
