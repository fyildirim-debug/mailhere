// Tarayıcıda ve testlerde Tauri yerine çalışan sahte arka uç.
// Davranışı Rust tarafıyla aynı sözleşmeyi izler (types.ts › Commands).

import { createDemoData } from './demo-data';
import {
  defaultSettings,
  type AuthKind,
  type MessageId,
  type OutboxItem,
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
  type HapticKind,
  type LockStatus,
  type Mailbox,
  type MailboxRole,
  type MessageDetail,
  type MessageSummary,
  type QrMatrix,
  type RemoteImage,
  type Rule,
  type RuleAction,
  type RuleCondition,
  type SavedSearch,
  type ScheduledItem,
  type ScreenedSender,
  type ServerConfig,
  type Settings,
  type Signature,
  type Tag,
  type Template,
  type TransferOffer,
  type TransferPreview,
  type TransferReport,
  type TransferStateKind,
  type Vacation,
  type View,
  LOCK_PASSWORD_MIN,
  QUICK_REPLIES_MAX,
  QUICK_REPLY_CHARS,
  SAVED_SEARCHES_MAX,
  SAVED_SEARCH_NAME_CHARS,
  SAVED_SEARCH_QUERY_CHARS,
  TAGS_MAX,
  TAG_COLORS,
  TAG_NAME_CHARS,
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
    tags: Tag[];
    /** Mobil kabuğun istediği titreşimler (yalnız kayıt; sahte arka uçta titreşim yok). */
    haptics: HapticKind[];
    /** Parola kilidi (testler kilitli açılışı buradan ve `persist` ile benzetir). */
    lock: MockLock;
  };
}

/**
 * Sahte parola kilidi. Gerçek arka uçta parola hiçbir yere yazılmaz; burada
 * yalnız tanıtım ve testler için düz metin tutulur (`persist` ile tarayıcı
 * deposunda, yeniden yükleme = yeniden başlatma: kilit açıksa kilitli açılır).
 */
export interface MockLock {
  enabled: boolean;
  locked: boolean;
  uiLocked: boolean;
  relockOnOpen: boolean;
  failures: number;
  /** Bekleme biter (ms); 0 ise yok. */
  retryAfter: number;
  password: string | null;
}

const LOCK_KEY = 'mailhere.mock.lock';
/** "Parolamı unuttum" yerel veriyi sildi: bir sonraki yükleme boş başlar (gerçek arka uçta veritabanı boştur). */
const WIPED_KEY = 'mailhere.mock.wiped';

function consumeWiped(persist: boolean): boolean {
  if (!persist) return false;
  try {
    const wiped = localStorage.getItem(WIPED_KEY) === '1';
    if (wiped) localStorage.removeItem(WIPED_KEY);
    return wiped;
  } catch {
    return false;
  }
}

/** Arka uçtaki `retry_delay` ile aynı: beş hatadan sonra 2, 4, 8 … sn, en çok 300. */
export function lockRetryDelayMs(failures: number): number {
  if (failures <= 5) return 0;
  return Math.min(2 ** Math.min(failures - 5, 20), 300) * 1000;
}

/** Arka uçtaki `allows` ile aynı: kilitliyken hangi komut geçer. */
export function lockAllows(cmd: string, coreOpen: boolean, uiLocked: boolean): boolean {
  if (cmd.startsWith('lock_')) return true;
  if (!coreOpen) return false;
  if (uiLocked) {
    return [
      'notify_hide', 'notify_layout', 'notify_focus', 'open_main', 'sync_now', 'sync_reconnect',
      'system_bars', 'haptic', 'notification_status', 'notification_permission', 'background_mode_get', 'battery_status',
    ].includes(cmd);
  }
  return true;
}

function loadLock(persist: boolean): MockLock {
  const fresh: MockLock = { enabled: false, locked: false, uiLocked: false, relockOnOpen: true, failures: 0, retryAfter: 0, password: null };
  if (!persist) return fresh;
  try {
    const raw = localStorage.getItem(LOCK_KEY);
    if (!raw) return fresh;
    const saved = JSON.parse(raw) as Partial<MockLock>;
    const enabled = !!saved.enabled && typeof saved.password === 'string';
    // Yeniden yükleme yeniden başlatma gibidir: kilit açıksa çekirdek kapalı başlar.
    return { ...fresh, enabled, locked: enabled, relockOnOpen: saved.relockOnOpen ?? true, password: enabled ? saved.password! : null };
  } catch {
    return fresh;
  }
}

const SETTINGS_KEY = 'mailhere.mock.settings';

function fail(code: AppError['code'], message: string): never {
  throw { code, message } satisfies AppError;
}

/** Öznitelik değerindeki karakter göndermelerini çözer (arka uçtaki `decode_entities` gibi). */
function decodeEntities(s: string): string {
  const named: Record<string, string> = { amp: '&', quot: '"', lt: '<', gt: '>', apos: "'", nbsp: '\u00a0' };
  return s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (whole, name: string) => {
    if (name[0] === '#') {
      const code = name[1] === 'x' || name[1] === 'X' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return Number.isFinite(code) ? String.fromCodePoint(code) : whole;
    }
    return named[name.toLowerCase()] ?? whole;
  });
}

/**
 * Tanıtımda "indirilen" görsel: kapak için uygulamanın renklerinde sade bir
 * SVG, izleme pikseli için 1×1 saydam PNG (gerçek arka uç sunucunun verdiği
 * baytları aynı biçimde `data:` adresi olarak döner).
 */
function demoImage(url: string): string {
  if (/\.gif(\?|$)/i.test(url) || /\/p\.gif/i.test(url)) {
    return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';
  }
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 560 200"><rect width="560" height="200" fill="#e6e8fb"/>' +
    '<circle cx="118" cy="100" r="54" fill="#4655d4"/><rect x="214" y="58" width="252" height="14" rx="7" fill="#aab2fa"/>' +
    '<rect x="214" y="92" width="196" height="14" rx="7" fill="#c9cdfb"/><rect x="214" y="126" width="224" height="14" rx="7" fill="#c9cdfb"/></svg>';
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

function lower(s: string): string {
  return s.toLocaleLowerCase('tr');
}

function summary(m: MessageDetail, followUp: number | null = m.followUp): MessageSummary {
  const {
    id, accountId, mailboxId, threadId, from, to, subject, snippet, date, unread, starred, hasAttachments, category, snoozedUntil, pinned, tags,
  } = m;
  return { id, accountId, mailboxId, threadId, from, to, subject, snippet, date, unread, starred, hasAttachments, category, snoozedUntil, followUp, pinned, tags };
}

/** Arka uçtaki `normalize_tag_name` ile aynı: denetim/yön karakterleri atılır, boşluklar teke iner, en çok 40 karakter. */
export function cleanTagName(name: string): string {
  // eslint-disable-next-line no-control-regex
  const line = name.replace(/[\u0000-\u001f‎‏‪-‮⁦-⁩]/g, (c) => (/\s/.test(c) ? ' ' : '')).replace(/\s+/g, ' ');
  return [...line.trim()].slice(0, TAG_NAME_CHARS).join('').trim();
}

/** Arka uçtaki `normalize_saved_searches` ile aynı kurallar. */
export function cleanSavedSearches(list: SavedSearch[]): SavedSearch[] {
  const seen = new Set<string>();
  const out: SavedSearch[] = [];
  const clip = (s: string, max: number) => [...s.replace(/\s+/g, ' ').trim()].slice(0, max).join('').trim();
  for (const s of list) {
    const query = clip(s.query, SAVED_SEARCH_QUERY_CHARS);
    if (!query) continue;
    const name = clip(s.name, SAVED_SEARCH_NAME_CHARS) || [...query].slice(0, SAVED_SEARCH_NAME_CHARS).join('');
    const id = s.id.trim() && !seen.has(s.id.trim()) ? s.id.trim() : newId('arama');
    seen.add(id);
    out.push({ id, name, query });
    if (out.length === SAVED_SEARCHES_MAX) break;
  }
  return out;
}

/** Sabitlenenler önce gelen görünümler (arka uçtaki `pinned_first`). */
function pinnedFirst(view: View): boolean {
  return view.kind === 'inbox' || view.kind === 'mailbox';
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

// ------------------------------------------------------------ aktarım (QR)

/** Aktarımın bellekte beklediği süre (arka uçtaki `PREVIEW_TTL`). */
const PREVIEW_TTL_MS = 5 * 60_000;
/** Teklifin ömrü (arka uçtaki `OfferOptions::ttl`). */
const OFFER_TTL_MS = 180_000;

/** Telefona giden paketin sahte karşılığı (gizli değerin kendisi yok, yalnız var/yok). */
interface MockPackage {
  fromVersion: string;
  fromPlatform: string;
  createdAt: number;
  settings: Settings | null;
  accounts: { account: Account; hasSecret: boolean }[];
  rules: Rule[];
  tags: Tag[];
}

/** Küçük, kararlı sözde rastgele kaynak (xorshift32); testler tekrarlanabilir olsun. */
function prng(seed: number): () => number {
  let x = (seed >>> 0) || 0x9e3779b9;
  return () => {
    x ^= x << 13;
    x >>>= 0;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    return x;
  };
}

/** base64url (dolgusuz) sözde rastgele `bytes` bayt. */
function fakeB64(bytes: number, seed: number): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  const next = prng(seed);
  const chars = Math.ceil((bytes * 8) / 6);
  let out = '';
  for (let i = 0; i < chars; i++) out += alphabet[next() % 64];
  return out;
}

/** EC-M bayt kipinde sürüm 1–15'in kapasitesi (bayt); `mh-transfer` aynı düzeyi kullanır. */
const QR_M_BYTES = [14, 26, 42, 62, 84, 106, 122, 152, 180, 213, 251, 287, 331, 362, 412];

/** Gerçek kodlayıcının metne seçeceği modül sayısı (21 + 4·(sürüm − 1)). */
export function qrSizeFor(text: string): number {
  const bytes = new TextEncoder().encode(text).length;
  const version = QR_M_BYTES.findIndex((cap) => bytes <= cap) + 1 || QR_M_BYTES.length;
  return 17 + 4 * version;
}

/**
 * QR görünümlü modül matrisi: köşelerde üç bulucu desen, aralarında zamanlama
 * çizgileri, geri kalanı metinden türetilmiş sözde rastgele veri. Gerçek QR
 * değildir (okutulamaz); yalnız arayüzün çizimi için. Boyut gerçek kodlayıcının
 * seçeceği sürümle aynıdır, böylece yerleşim gerçek boyutla sınanır.
 */
export function mockQr(text: string, size = qrSizeFor(text)): QrMatrix {
  let seed = 7;
  for (const ch of text) seed = (seed * 31 + ch.charCodeAt(0)) >>> 0;
  const next = prng(seed);
  const grid: boolean[][] = Array.from({ length: size }, () => Array.from({ length: size }, () => next() % 2 === 1));
  const finder = (top: number, left: number) => {
    for (let r = -1; r <= 7; r++) {
      for (let c = -1; c <= 7; c++) {
        const y = top + r;
        const x = left + c;
        if (y < 0 || x < 0 || y >= size || x >= size) continue;
        const ring = r === 0 || r === 6 || c === 0 || c === 6;
        const core = r >= 2 && r <= 4 && c >= 2 && c <= 4;
        const inside = r >= 0 && r <= 6 && c >= 0 && c <= 6;
        grid[y][x] = inside && (ring || core);
      }
    }
  };
  finder(0, 0);
  finder(0, size - 7);
  finder(size - 7, 0);
  for (let i = 8; i < size - 8; i++) {
    grid[6][i] = i % 2 === 0;
    grid[i][6] = i % 2 === 0;
  }
  return { size, rows: grid.map((row) => row.map((on) => (on ? '1' : '0')).join('')) };
}

/** Kod metnini denetler (arka uçtaki `Code::parse` ile aynı mesajlar). */
function checkTransferCode(code: string): void {
  const text = code.trim();
  const unknown = () => fail('invalid', 'Aktarım kodu tanınmadı. Bilgisayardaki QR kodu yeniden okut ya da kodu eksiksiz yapıştır.');
  if (!/^mailhere:\/\/aktar\?/i.test(text)) unknown();
  const params = new Map<string, string>();
  for (const pair of text.slice('mailhere://aktar?'.length).split('&')) {
    if (!pair) continue;
    const i = pair.indexOf('=');
    if (i < 0) unknown();
    params.set(pair.slice(0, i), pair.slice(i + 1));
  }
  const v = Number(params.get('v'));
  if (!Number.isInteger(v) || v < 1) unknown();
  if (v > 1) fail('invalid', `Bu kod daha yeni bir Mailhere sürümüyle oluşturulmuş (v${v}). Önce bu cihazdaki Mailhere'yi güncelle.`);
  const hosts = (params.get('h') ?? '').split(',').filter(Boolean);
  const port = Number(params.get('p'));
  if (!hosts.length || !Number.isInteger(port) || port < 1 || port > 65535 || !params.get('t') || !params.get('k')) unknown();
  if (hosts.some((h) => /^(8\.8\.8\.8|1\.1\.1\.1)$/.test(h))) {
    fail('invalid', 'Aktarım kodu yerel ağ dışında bir adres içeriyor; güvenlik için kullanılmadı. Bilgisayarda yeni kod oluştur.');
  }
  // Ulaşılamayan bilgisayar (testler): adres 10.255.255.1 ise ağ hatası.
  if (hosts.includes('10.255.255.1')) {
    fail('network', "Bilgisayara bağlanılamadı. İki cihaz aynı Wi-Fi ağında mı, bilgisayarda aktarım ekranı hâlâ açık mı? Güvenlik duvarı Mailhere'ye izin vermeli.");
  }
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
  const empty = !!opts.empty || consumeWiped(!!opts.persist);
  const state = {
    accounts: empty ? [] : data.accounts,
    mailboxes: empty ? [] : data.mailboxes,
    messages: empty ? [] : data.messages,
    settings: loadSettings(!!opts.persist),
    rules: empty
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
    tags: empty ? [] : data.tags,
    haptics: [] as HapticKind[],
    lock: loadLock(!!opts.persist),
  };
  const scheduled = new Map<string, { item: ScheduledItem; msg: OutgoingMessage; timer: ReturnType<typeof setTimeout> }>();
  const vacations = new Map<string, Vacation>();
  // Sunucudan bulunan (otomatik) roller: elle atama sıfırlanınca geri gelir.
  const autoRole = new Map(state.mailboxes.map((b) => [b.id, b.role]));
  const handlers = new Map<string, Set<Handler>>();
  /** Android bildirim izni (yalnız mobil kabuk sorar). */
  let notifyPerm = 'prompt';
  /** Android bildirim sağlığı: pil kısıtlaması ve anlık bildirim (sürekli bağlantı; kabukta varsayılan açık). */
  let battery: CommandResult<'battery_status'> = 'optimized';
  let backgroundMode = true;
  /** Açılışta başlama (iki kabuk; kabukta varsayılan açık). */
  let autostart = true;
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
      case 'tag':
        return m.tags.includes(view.id) && role !== 'trash' && role !== 'junk';
      default:
        return role === view.kind;
    }
  }

  function tagOf(id: string): Tag {
    const tg = state.tags.find((x) => x.id === id);
    if (!tg) fail('notFound', 'Etiket bulunamadı; silinmiş olabilir.');
    return tg;
  }

  /** Etiketi denetler (arka uçtaki `normalize_tag`). */
  function checkTag(tag: Tag): Tag {
    const name = cleanTagName(tag.name);
    if (!name) fail('invalid', 'Etikete bir ad ver.');
    const color = TAG_COLORS.find((c) => c.toLowerCase() === tag.color.trim().toLowerCase());
    if (!color) fail('invalid', 'Etiket rengi paletten seçilmeli.');
    if (state.tags.some((x) => x.id !== tag.id && lower(x.name) === lower(name))) fail('invalid', 'Bu adda bir etiket zaten var.');
    return { id: tag.id.trim(), name, color };
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

  // Aktarım: açık teklifin zamanlayıcıları (bilgisayar) ve bekleyen paketler (telefon).
  let offerTimers: ReturnType<typeof setTimeout>[] | null = null;
  let offerSeq = 0;
  const previews = new Map<string, { pkg: MockPackage; at: number }>();

  function emitTransfer(state: TransferStateKind, message: string | null = null) {
    emit(EVENTS.transfer, { state, message });
  }

  /** Açık teklifi kapatır; `announce` ile `cancelled` olayı gider. */
  function closeOffer(announce: boolean): boolean {
    if (!offerTimers) return false;
    for (const t of offerTimers) clearTimeout(t);
    offerTimers = null;
    if (announce) emitTransfer('cancelled');
    return true;
  }

  /** Bilgisayardan gelmiş gibi örnek paket: biri bu cihazda olabilen iki hesap, kurallar, etiketler, ayarlar. */
  function samplePackage(): MockPackage {
    const work: Account = {
      id: 'demo-is', name: 'İş', email: 'furkan@ornek.com.tr', displayName: 'Furkan Yıldırım', color: '#4C6FD8', kind: 'imap',
      imap: { host: 'imap.ornek.com.tr', port: 993, security: 'tls', username: 'furkan@ornek.com.tr' },
      smtp: { host: 'smtp.ornek.com.tr', port: 465, security: 'tls', username: 'furkan@ornek.com.tr' },
      jmapUrl: null, createdAt: now() - 30 * 86_400_000, auth: 'password', notify: true,
      signatures: [{ id: 'is-tam', name: 'Tam', body: 'Furkan Yıldırım\nÜrün Geliştirme · Örnek A.Ş.' }], signatureNew: 'is-tam', signatureReply: null,
    };
    const google: Account = {
      id: 'aktarim-google', name: 'Google', email: 'furkan.yildirim@gmail.com', displayName: 'Furkan', color: '#D0566E', kind: 'imap',
      imap: { host: 'imap.gmail.com', port: 993, security: 'tls', username: 'furkan.yildirim@gmail.com' },
      smtp: { host: 'smtp.gmail.com', port: 465, security: 'tls', username: 'furkan.yildirim@gmail.com' },
      jmapUrl: null, createdAt: now() - 10 * 86_400_000, auth: 'google', notify: true, signatures: [], signatureNew: null, signatureReply: null,
    };
    return {
      fromVersion: '0.4.0',
      fromPlatform: 'linux',
      createdAt: now(),
      settings: {
        ...defaultSettings, theme: 'dark', density: 'compact', vip: ['elif@ornek.com.tr'], conversationView: false,
        // Cihaza özgü alanlar: uygulanırken yereldekiler kalır.
        cardSeconds: 30, cardPosition: 'topLeft', nativeNotifications: true, autoUpdate: false,
        notifyActions: ['markRead', 'archive'], swipeLeft: 'trash', swipeRight: 'pin', afterRemove: 'next',
      },
      accounts: [
        { account: work, hasSecret: true },
        { account: google, hasSecret: false },
      ],
      rules: [
        {
          id: 'aktarim-kural-fatura', name: 'Faturalar etiketlensin', enabled: true, accountId: null, match: 'any',
          conditions: [{ kind: 'subject', op: 'contains', value: 'fatura' }], actions: [{ kind: 'tag', tagId: 'aktarim-etiket-fatura' }], stop: false,
        },
        {
          id: 'aktarim-kural-toplanti', name: 'Toplantı davetleri yıldızlı', enabled: true, accountId: 'demo-is', match: 'all',
          conditions: [{ kind: 'subject', op: 'contains', value: 'toplantı' }], actions: [{ kind: 'star' }], stop: false,
        },
        {
          id: 'aktarim-kural-eski', name: 'Eski hesabın bültenleri', enabled: true, accountId: 'eski-hesap', match: 'all',
          conditions: [{ kind: 'category', value: 'newsletters' }], actions: [{ kind: 'archive' }], stop: false,
        },
      ],
      tags: [
        { id: 'aktarim-etiket-fatura', name: 'Fatura', color: '#D08A1E' },
        { id: 'aktarim-etiket-seyahat', name: 'Seyahat', color: '#2A9BA8' },
      ],
    };
  }

  function draftMessage(msg: OutgoingMessage, id: MessageId): MessageDetail {
    const account = state.accounts.find((a) => a.id === msg.accountId);
    if (!account) fail('notFound', 'Hesap bulunamadı.');
    return {
      id, accountId: account.id, mailboxId: boxFor(account.id, 'drafts'), threadId: `draft-${id}`,
      from: { name: account.displayName, email: account.email }, to: msg.to, subject: msg.subject,
      snippet: msg.bodyText.replace(/\s+/g, ' ').slice(0, 200), date: now(), unread: false, starred: false,
      hasAttachments: msg.attachments.length > 0, category: 'people', snoozedUntil: null, followUp: null, pinned: false, tags: [], cc: msg.cc, bcc: msg.bcc, replyTo: [],
      messageIdHeader: null, references: [], bodyText: msg.bodyText, bodyHtml: null, hasRemoteContent: false,
      remoteAllowed: false, attachments: [], thread: [], unsubscribe: null, invite: null, trackers: 0, cleanedLinks: 0, senderWarning: null, otp: null,
    };
  }

  /** Giden kutusuna koyar ("gönderimi geri al" süresi ya da zamanlanmış an). */
  function queueSend(msg: OutgoingMessage): OutboxItem {
    if (!msg.to.length) fail('invalid', 'En az bir alıcı ekle.');
    const id = `out-${nextId++}`;
    const sendAt = msg.sendAt ?? now() + state.settings.undoSendSeconds * 1000;
    if (msg.draftId != null) state.messages = state.messages.filter((m) => m.id !== msg.draftId);
    const item: ScheduledItem = { id, accountId: msg.accountId, sendAt, to: msg.to, subject: msg.subject, error: null };
    scheduled.set(id, { item, msg, timer: setTimeout(() => deliver(id), Math.max(0, sendAt - now())) });
    if (msg.sendAt) emit(EVENTS.changed, { mailboxIds: [] });
    return { id, sendAt };
  }

  function deliver(id: string) {
    const entry = scheduled.get(id);
    if (!entry) return;
    clearTimeout(entry.timer);
    // Tarayıcı çevrimdışıyken gönderim giden kutusunda bekler; bağlantı gelince gider
    // (gerçek arka uçta SMTP hatası alır ve yeniden dener).
    if (typeof navigator !== 'undefined' && navigator.onLine === false && typeof window !== 'undefined') {
      window.addEventListener('online', () => deliver(id), { once: true });
      return;
    }
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
        hasAttachments: msg.attachments.length > 0, category: 'people', snoozedUntil: null, followUp: null, pinned: false, tags: [], cc: msg.cc, bcc: [], replyTo: [],
        messageIdHeader: null, references: [], bodyText: msg.bodyText, bodyHtml: null, hasRemoteContent: false,
        remoteAllowed: false, attachments: [], thread: [], unsubscribe: null, invite: null, trackers: 0, cleanedLinks: 0, senderWarning: null, otp: null,
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
      if (a.kind === 'tag' && !state.tags.some((x) => x.id === a.tagId.trim())) {
        fail('invalid', 'Eklenecek etiket bulunamadı; silinmiş olabilir. Başka bir etiket seç.');
      }
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
      // Gelen kutusu ve klasörde sabitlenenler önce; imleç o zaman üç parçalı ("tarih:kimlik:sabit").
      const pinFirst = pinnedFirst(query.view);
      const rank = (m: MessageDetail) => (pinFirst && m.pinned ? 1 : 0);
      let items = state.messages
        .filter((m) => inView(m, query.view))
        .filter((m) => !query.accountId || m.accountId === query.accountId)
        .filter((m) => !query.category || m.category === query.category)
        .filter((m) => (query.only === 'unread' ? m.unread : query.only === 'starred' ? m.starred : query.only === 'attachments' ? m.hasAttachments : true))
        .sort((a, b) => rank(b) - rank(a) || b.date - a.date || b.id - a.id);
      // Yanıt bekleyenler: hatırlatma anına göre, imleçsiz (küçük liste).
      if (query.view.kind === 'followups') {
        items.sort((a, b) => followUpOf(a.id)! - followUpOf(b.id)! || a.id - b.id);
        return { items: items.map((m) => summary(m, followUpOf(m.id))), nextCursor: null };
      }
      if (query.cursor) {
        const [d, i, p = 0] = query.cursor.split(':').map(Number);
        const after = (m: MessageDetail) => m.date < d || (m.date === d && m.id < i);
        items = items.filter((m) => (pinFirst ? rank(m) < p || (rank(m) === p && after(m)) : after(m)));
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
        nextCursor: items.length > query.limit && last ? `${last.date}:${last.id}${pinFirst ? `:${rank(last)}` : ''}` : null,
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
      // Arka uçtaki işleçlerin sade karşılığı: from:, to:, konu:, has:ek, is:okunmamış, is:yıldızlı, in:, tag:, after:, before:.
      const tests: ((m: MessageDetail) => boolean)[] = [];
      let inBox = false;
      const free = q.replace(/(\S+?):("([^"]*)"|\S+)/g, (all, op: string, _v: string, quoted: string | undefined) => {
        const v = lower(quoted ?? _v);
        const o = lower(op);
        if (o === 'tag' || o === 'etiket') {
          // Tam ad varsa yalnız o; yoksa önek eşleşmesi.
          const exact = state.tags.filter((x) => lower(x.name) === v).map((x) => x.id);
          const ids = exact.length ? exact : state.tags.filter((x) => lower(x.name).startsWith(v)).map((x) => x.id);
          tests.push((m) => m.tags.some((id) => ids.includes(id)));
          return ' ';
        }
        if (o === 'from') tests.push((m) => lower(`${m.from.name ?? ''} ${m.from.email}`).includes(v));
        else if (o === 'to') tests.push((m) => [...m.to, ...m.cc].some((a) => lower(`${a.name ?? ''} ${a.email}`).includes(v)));
        else if (o === 'subject' || o === 'konu') tests.push((m) => lower(m.subject).includes(v));
        else if (o === 'has' && ['ek', 'attachment'].includes(v)) tests.push((m) => m.hasAttachments);
        else if (o === 'is' && ['okunmamış', 'okunmamis', 'unread'].includes(v)) tests.push((m) => m.unread);
        else if (o === 'is' && ['yıldızlı', 'yildizli', 'starred'].includes(v)) tests.push((m) => m.starred);
        else if (['after', 'sonra', 'before', 'önce', 'once'].includes(o) && /^\d{4}[-/]\d{2}[-/]\d{2}$/.test(v)) {
          // Yerel günün başlangıcı; after: o günü de kapsar, before: o günden öncesi.
          const [y, mo, d] = v.split(/[-/]/).map(Number);
          const at = new Date(y, mo - 1, d).getTime();
          if (o === 'after' || o === 'sonra') tests.push((m) => m.date >= at);
          else tests.push((m) => m.date < at);
        }
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
        .map((m) => summary(m, followUpOf(m.id)));
    },
    remote_allow: ({ id }) => {
      const m = find(id);
      m.remoteAllowed = true;
      const who = lower(m.from.email);
      if (!state.settings.remoteAllow.includes(who)) state.settings.remoteAllow.push(who);
      return detail(m);
    },
    remote_images: ({ id, urls }) => {
      const d = detail(find(id));
      if (!d.remoteAllowed) fail('invalid', 'Bu göndericinin uzak görsellerine izin verilmemiş. Görselleri görmek için önce “Bu göndericiye izin ver”e tıkla.');
      // Yalnız postanın temizlenmiş HTML'inde geçen http(s) görsel adresleri (çözülmüş).
      const known = new Set([...(d.bodyHtml ?? '').matchAll(/<img\b[^>]*\ssrc="(https?:[^"]*)"/gi)].map((m) => decodeEntities(m[1])));
      const picked = [...new Set(urls.map((u) => u.trim()))].filter((u) => known.has(u)).slice(0, 40);
      if (!picked.length) fail('invalid', 'İstenen görsel adresleri bu postada geçmiyor. Postayı kapatıp yeniden aç.');
      return picked.map((url): RemoteImage => ({ url, data: demoImage(url) }));
    },
    attachment_open: ({ id, index }) => {
      const a = find(id).attachments.find((x) => x.index === index);
      if (!a) fail('notFound', 'Ek bulunamadı.');
      return `/tmp/mailhere/${a.name}`;
    },
    send: ({ msg }) => queueSend(msg),
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
      state.settings = {
        ...settings,
        templates: cleanTemplates(settings.templates ?? []),
        quickReplies: cleanQuickReplies(settings.quickReplies),
        savedSearches: cleanSavedSearches(settings.savedSearches ?? []),
      };
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
    sync_reconnect: () => {
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
        // Hesap ve alıcılar arka uçtaki gibi `?from=hesap&to=Ad <adres>` sorgusuyla taşınır.
        const q = new URLSearchParams();
        if (prefill.accountId && /^[A-Za-z0-9._:-]{1,64}$/.test(prefill.accountId)) q.append('from', prefill.accountId);
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
    notify_focus: () => null,
    notify_action: ({ id, action }) => {
      const m = find(id);
      switch (action) {
        case 'markRead':
          m.unread = false;
          break;
        case 'archive':
        case 'trash':
          m.mailboxId = boxFor(m.accountId, action);
          m.snoozedUntil = null;
          break;
        case 'spam': {
          const junk = state.mailboxes.find((b) => b.accountId === m.accountId && b.role === 'junk');
          if (!junk) fail('notFound', 'Bu hesapta İstenmeyen klasörü yok. Postayı uygulamada aç ve bir klasöre taşı.');
          m.mailboxId = junk.id;
          m.snoozedUntil = null;
          break;
        }
        default:
          fail('invalid', 'Bu düğme bildirimden çalıştırılamaz; postayı uygulamada aç.');
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    notify_reply: ({ id, text }) => {
      const body = text.trim();
      if (!body) fail('invalid', 'Yanıt boş. Bir şey yaz ya da postayı uygulamada aç.');
      const m = find(id);
      const subject = m.subject.replace(/^(\s*(re|ynt|fwd?|ilt)\s*:\s*)+/i, '');
      const item = queueSend({
        accountId: m.accountId,
        to: m.replyTo.length ? m.replyTo : [m.from],
        cc: [],
        bcc: [],
        subject: `Re: ${subject}`,
        bodyText: body,
        inReplyTo: id,
        attachments: [],
        sendAt: null,
      });
      m.unread = false;
      emit(EVENTS.changed, { mailboxIds: [] });
      return item;
    },
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
          // Silinmiş etiket sessizce atlanır.
          else if (a.kind === 'tag' && state.tags.some((x) => x.id === a.tagId) && !m.tags.includes(a.tagId)) m.tags.push(a.tagId);
        }
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return n;
    },
    messages_by_sender: ({ accountId, email, view }) => {
      // Adres karşılaştırması ASCII büyük/küçük harf duyarsız (Türkçe I/ı kuralı uygulanmaz).
      const who = email.trim().toLowerCase();
      if (!who) return [];
      const scope = (m: MessageDetail) => {
        if (view?.kind === 'mailbox') return m.mailboxId === view.id;
        if (view?.kind === 'tag') return m.tags.includes(view.id) && roleOf(m.mailboxId) !== 'trash' && roleOf(m.mailboxId) !== 'junk';
        return roleOf(m.mailboxId) === 'inbox';
      };
      return state.messages
        .filter((m) => scope(m) && m.from.email.trim().toLowerCase() === who && (!accountId || m.accountId === accountId))
        .sort((a, b) => b.date - a.date || b.id - a.id)
        .map((m) => m.id);
    },
    messages_pin: ({ ids, pinned }) => {
      for (const id of ids) find(id).pinned = pinned;
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    tags_list: () => structuredClone(state.tags),
    tag_save: ({ tag }) => {
      const clean = checkTag(tag);
      if (clean.id) {
        const i = state.tags.findIndex((x) => x.id === clean.id);
        if (i < 0) fail('notFound', 'Etiket bulunamadı; silinmiş olabilir.');
        state.tags[i] = clean;
      } else {
        if (state.tags.length >= TAGS_MAX) fail('invalid', 'En çok 50 etiket olabilir. Kullanmadıklarını sil.');
        clean.id = newId('etiket');
        state.tags.push(clean);
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return structuredClone(clean);
    },
    tag_delete: ({ id }) => {
      state.tags = state.tags.filter((x) => x.id !== id);
      for (const m of state.messages) m.tags = m.tags.filter((x) => x !== id);
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
    },
    messages_tag: ({ ids, tagId, on }) => {
      const tg = tagOf(tagId.trim());
      for (const id of ids) {
        const m = find(id);
        if (on && !m.tags.includes(tg.id)) m.tags.push(tg.id);
        if (!on) m.tags = m.tags.filter((x) => x !== tg.id);
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return null;
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
      return {
        email: e,
        name: named[0]?.from.name ?? null,
        total: hits.length,
        lastDate: hits[0]?.date ?? null,
        recent: [...threads.values()].slice(0, 5).map((m) => summary(m, followUpOf(m.id))),
      };
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
        hasRemoteContent: false, remoteAllowed: false, attachments: [], thread: [], unsubscribe: null, invite: null, followUp: null, pinned: false, tags: [],
        trackers: 0, cleanedLinks: 0, senderWarning: null, otp: null,
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
        emit(EVENTS.notify, { message: summary(m), accountName: a?.name ?? '', accountColor: a?.color ?? '#4C6FD8', otp: m.otp });
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
    transfer_begin: ({ includeSecrets }) => {
      // Yeni teklif eskisini sessizce kapatır.
      closeOffer(false);
      const seed = (now() ^ (offerSeq++ * 0x9e37) ^ (includeSecrets ? 0x5a5a : 0)) >>> 0;
      // Gerçek bilgisayarlar çoğu zaman birkaç yerel adres duyurur (Wi-Fi, konteyner, örtü ağı, ULA).
      const hosts = ['192.168.1.20', '172.17.0.1', '100.101.102.103', 'fd12:3456::20'];
      const text = `mailhere://aktar?v=1&h=${hosts.join(',')}&p=51234&t=${fakeB64(16, seed)}&k=${fakeB64(32, seed + 1)}`;
      const offer: TransferOffer = { text, qr: mockQr(text), hosts, port: 51234, expiresAt: now() + OFFER_TTL_MS };
      // Bitiş: kalan zamanlayıcılar (ör. süre dolması) da kapanır.
      const finish = (state: TransferStateKind, message: string | null = null) => {
        closeOffer(false);
        emitTransfer(state, message);
      };
      offerTimers = [
        setTimeout(() => emitTransfer('waiting'), 0),
        setTimeout(() => emitTransfer('connected'), 1000),
        setTimeout(() => emitTransfer('delivered'), 2000),
        setTimeout(() => finish('applied'), 4000),
        setTimeout(() => finish('expired', 'Kodun süresi doldu; telefon bağlanmadı. Yeni kod oluşturup yeniden dene.'), OFFER_TTL_MS),
      ];
      return offer;
    },
    transfer_cancel: () => {
      closeOffer(true);
      return null;
    },
    transfer_receive: async ({ code }) => {
      checkTransferCode(code);
      // Ağdan alma süresi.
      await new Promise((r) => setTimeout(r, 300));
      const pkg = samplePackage();
      const previewId = newId('onizleme');
      // Süresi dolanlar düşer.
      for (const [id, p] of previews) if (now() - p.at >= PREVIEW_TTL_MS) previews.delete(id);
      previews.set(previewId, { pkg, at: now() });
      const preview: TransferPreview = {
        previewId,
        fromVersion: pkg.fromVersion,
        fromPlatform: pkg.fromPlatform,
        createdAt: pkg.createdAt,
        accounts: pkg.accounts.map(({ account, hasSecret }) => ({
          id: account.id, name: account.name, email: account.email, kind: account.kind, auth: account.auth,
          hasSecret, exists: state.accounts.some((a) => a.id === account.id),
        })),
        rules: pkg.rules.length,
        tags: pkg.tags.length,
        settings: pkg.settings != null,
      };
      return preview;
    },
    transfer_apply: ({ previewId, choice }) => {
      const entry = previews.get(previewId);
      previews.delete(previewId);
      if (!entry || now() - entry.at >= PREVIEW_TTL_MS) {
        fail('notFound', "Önizlemenin süresi doldu ya da paket zaten uygulandı. Bilgisayarda yeni kod oluşturup QR'ı yeniden okut.");
      }
      const { pkg } = entry;
      const report: TransferReport = { accountsAdded: 0, accountsUpdated: 0, accountsWithoutSecret: [], rules: 0, rulesSkipped: 0, tags: 0, settings: false };
      // 1) Ayarlar: cihaza özgü olanlar ve onaylı sertifikalar yerelde kalır.
      if (choice.settings && pkg.settings) {
        const local = state.settings;
        state.settings = {
          ...pkg.settings,
          cardPosition: local.cardPosition, cardSeconds: local.cardSeconds, nativeNotifications: local.nativeNotifications,
          autoUpdate: local.autoUpdate, trustedCerts: local.trustedCerts,
          notifyActions: local.notifyActions, swipeLeft: local.swipeLeft, swipeRight: local.swipeRight, afterRemove: local.afterRemove,
          quickReplies: cleanQuickReplies(pkg.settings.quickReplies), templates: cleanTemplates(pkg.settings.templates),
          savedSearches: cleanSavedSearches(pkg.settings.savedSearches),
        };
        report.settings = true;
        emit(EVENTS.settings, structuredClone(state.settings));
      }
      // 2) Etiketler: adı çakışan yereldekine eşlenir; seçilmediyse yalnız eşleme kurulur.
      const tagMap = new Map<string, string>();
      for (const t of pkg.tags) {
        const name = cleanTagName(t.name);
        if (!name) continue;
        const local = state.tags.find((x) => lower(x.name) === lower(name));
        if (local) {
          tagMap.set(t.id, local.id);
          continue;
        }
        if (!choice.tags || state.tags.length >= TAGS_MAX) continue;
        const color = TAG_COLORS.find((c) => c.toLowerCase() === t.color.trim().toLowerCase()) ?? TAG_COLORS[0];
        state.tags.push({ id: t.id, name, color });
        tagMap.set(t.id, t.id);
        report.tags++;
      }
      // 3) Hesaplar: kimlik korunur; varsa güncellenir, yoksa eklenir.
      for (const { account, hasSecret } of pkg.accounts) {
        if (!choice.accounts.includes(account.id)) continue;
        const i = state.accounts.findIndex((a) => a.id === account.id);
        if (i >= 0) {
          state.accounts[i] = { ...structuredClone(account), createdAt: state.accounts[i].createdAt };
          report.accountsUpdated++;
        } else {
          state.accounts.push(structuredClone(account));
          addMailboxes(account.id);
          report.accountsAdded++;
          if (!hasSecret) report.accountsWithoutSecret.push(account.email);
        }
        emit(EVENTS.sync, { accountId: account.id, state: 'online' });
      }
      // 4) Kurallar: hesabı bu cihazda olmayan atlanır; etiket kimlikleri yeniden yazılır.
      if (choice.rules) {
        for (const rule of pkg.rules) {
          if (rule.accountId && !state.accounts.some((a) => a.id === rule.accountId)) {
            report.rulesSkipped++;
            continue;
          }
          const actions = rule.actions.flatMap((a): RuleAction[] => {
            if (a.kind !== 'tag') return [a];
            const local = tagMap.get(a.tagId) ?? (state.tags.some((x) => x.id === a.tagId) ? a.tagId : null);
            return local ? [{ ...a, tagId: local }] : [];
          });
          if (!actions.length) {
            report.rulesSkipped++;
            continue;
          }
          const clean = { ...structuredClone(rule), actions };
          const i = state.rules.findIndex((r) => r.id === rule.id);
          if (i >= 0) state.rules[i] = clean;
          else if (state.rules.length < 200) state.rules.push(clean);
          else {
            report.rulesSkipped++;
            continue;
          }
          report.rules++;
        }
      }
      emit(EVENTS.changed, { mailboxIds: [] });
      return report;
    },
    // İki kabuk: açılışta başlama.
    autostart_get: () => autostart,
    autostart_set: ({ on }) => {
      autostart = on;
      return autostart;
    },
    // Yalnız mobil kabuk: bildirim sağlığı. Kısıtlamayı kaldırma isteği onaylanmış sayılır.
    battery_status: () => battery,
    battery_settings: () => {
      battery = 'unrestricted';
      return null;
    },
    background_mode_get: () => backgroundMode,
    background_mode_set: ({ on }) => {
      backgroundMode = on;
      return backgroundMode;
    },
    // Yalnız mobil kabuk: kamera yerine geçerli görünümlü örnek kod; bildirim izni verilmiş sayılır.
    qr_scan: () => `mailhere://aktar?v=1&h=192.168.1.20&p=51234&t=${fakeB64(16, 41)}&k=${fakeB64(32, 42)}`,
    // Bildirim izni: ilk istekte verilmiş sayılır; ayar açma bir şey yapmaz.
    notification_permission: () => {
      notifyPerm = 'granted';
      return notifyPerm;
    },
    notification_status: () => notifyPerm,
    notification_settings: () => null,
    system_bars: () => null,
    // Titreşim yok; testler istenen türleri sırasıyla görür.
    haptic: ({ kind }) => {
      state.haptics.push(kind);
      return null;
    },
    // Parola kilidi: davranış arka uçla aynı (uzunluk, yanlış parola sayacı,
    // üstel bekleme, sıfırlama); parola yalnız bu sahte arka uçta saklanır.
    lock_status: () => lockStatus(),
    lock_enable: ({ password }) => {
      checkPassword(password);
      if (state.lock.enabled) fail('invalid', 'Parola kilidi zaten açık. Parolayı değiştirmek için "Parolayı değiştir"i kullan.');
      state.lock = { ...state.lock, enabled: true, locked: false, uiLocked: false, relockOnOpen: true, failures: 0, retryAfter: 0, password };
      saveLock();
      emit(EVENTS.lock, lockStatus());
      return lockStatus();
    },
    lock_disable: ({ password }) => {
      requireEnabled();
      verifyPassword(password);
      state.lock = { ...state.lock, enabled: false, locked: false, uiLocked: false, relockOnOpen: true, password: null };
      saveLock();
      emit(EVENTS.lock, lockStatus());
      return lockStatus();
    },
    lock_change: ({ current, next }) => {
      requireEnabled();
      checkPassword(next);
      verifyPassword(current);
      state.lock.password = next;
      saveLock();
      return lockStatus();
    },
    lock_unlock: ({ password }) => {
      requireEnabled();
      verifyPassword(password);
      state.lock.locked = false;
      state.lock.uiLocked = false;
      emit(EVENTS.lock, lockStatus());
      return lockStatus();
    },
    lock_ui_lock: () => {
      if (state.lock.enabled && !state.lock.locked && !state.lock.uiLocked) {
        state.lock.uiLocked = true;
        emit(EVENTS.lock, lockStatus());
      }
      return lockStatus();
    },
    lock_set_relock: ({ on }) => {
      requireEnabled();
      state.lock.relockOnOpen = on;
      saveLock();
      emit(EVENTS.lock, lockStatus());
      return lockStatus();
    },
    lock_reset_local: () => {
      requireEnabled();
      // Yerel veri gider: hesaplar, kutular, postalar, kurallar, etiketler; ayarlar varsayılana döner.
      state.accounts = [];
      state.mailboxes = [];
      state.messages = [];
      state.rules = [];
      state.tags = [];
      state.followUps = {};
      state.screened = [];
      state.known = [];
      state.settings = { ...defaultSettings };
      state.lock = { enabled: false, locked: false, uiLocked: false, relockOnOpen: true, failures: 0, retryAfter: 0, password: null };
      saveLock();
      if (opts.persist) {
        try {
          localStorage.removeItem(SETTINGS_KEY);
          localStorage.setItem(WIPED_KEY, '1');
        } catch {
          /* depolama kapalı olabilir */
        }
      }
      emit(EVENTS.lock, lockStatus());
      emit(EVENTS.changed, { mailboxIds: [] });
      return lockStatus();
    },
  };

  function lockStatus(): LockStatus {
    const l = state.lock;
    return { enabled: l.enabled, locked: l.locked, uiLocked: l.uiLocked, relockOnOpen: l.relockOnOpen, retryAfterMs: Math.max(0, l.retryAfter - now()) };
  }

  function saveLock() {
    if (!opts.persist) return;
    try {
      const { enabled, relockOnOpen, password } = state.lock;
      if (enabled) localStorage.setItem(LOCK_KEY, JSON.stringify({ enabled, relockOnOpen, password }));
      else localStorage.removeItem(LOCK_KEY);
    } catch {
      /* özel pencerede depolama kapalı olabilir */
    }
  }

  function checkPassword(p: string) {
    if ([...p].length < LOCK_PASSWORD_MIN) fail('invalid', `Parola en az ${LOCK_PASSWORD_MIN} karakter olmalı.`);
    if (new TextEncoder().encode(p).length > 256) fail('invalid', 'Parola en çok 256 bayt olabilir.');
  }

  function requireEnabled() {
    if (!state.lock.enabled) fail('invalid', 'Parola kilidi açık değil.');
  }

  /** Yanlış parola sayılır; beş hatadan sonra bekleme kurulur (arka uçla aynı metinler). */
  function verifyPassword(p: string) {
    const left = state.lock.retryAfter - now();
    if (left > 0) fail('locked', `Çok sayıda yanlış deneme. ${Math.max(1, Math.ceil(left / 1000))} saniye sonra yeniden deneyebilirsin.`);
    if (p !== state.lock.password) {
      state.lock.failures += 1;
      const delay = lockRetryDelayMs(state.lock.failures);
      state.lock.retryAfter = delay ? now() + delay : 0;
      emit(EVENTS.lock, lockStatus());
      fail('auth', delay ? `Parola yanlış. Yeniden dene. Sonraki deneme için ${delay / 1000} saniye bekle.` : 'Parola yanlış. Yeniden dene.');
    }
    state.lock.failures = 0;
    state.lock.retryAfter = 0;
  }

  return {
    state,
    emit,
    async call(name, args) {
      // Gerçek arka uç gibi eşzamansız dön.
      await Promise.resolve();
      // Komut kapısı (arka uçtaki `lock::gate`): kilitliyken içerik komutları reddedilir.
      if (!lockAllows(name, !state.lock.locked, state.lock.uiLocked)) fail('locked', 'Mailhere kilitli. Devam etmek için parolanı gir.');
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
