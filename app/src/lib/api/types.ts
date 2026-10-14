// Arka uçla sözleşme. `crates/mh-core/src/lib.rs` ile birebir aynı tutulur:
// bir alan değişirse iki dosya aynı committe güncellenir.

export type AccountId = string;
export type MessageId = number;
/** Unix zaman damgası, milisaniye. */
export type Millis = number;

// ---------------------------------------------------------------- hesaplar

export type Security = 'tls' | 'starttls' | 'none';

export interface ServerConfig {
  host: string;
  port: number;
  security: Security;
  username: string;
}

export type AccountKind = 'imap' | 'jmap';
export type AuthKind = 'password' | 'google' | 'microsoft';

export interface Account {
  id: AccountId;
  name: string;
  email: string;
  displayName: string;
  color: string;
  kind: AccountKind;
  imap: ServerConfig | null;
  smtp: ServerConfig | null;
  jmapUrl: string | null;
  /** Hesabın imzaları (düz metin). */
  signatures: Signature[];
  /** Yeni postada kullanılan imzanın kimliği; null: imzasız. */
  signatureNew: string | null;
  /** Yanıt ve iletmede kullanılan imzanın kimliği; null: imzasız. */
  signatureReply: string | null;
  createdAt: Millis;
  auth: AuthKind;
  /** Bu hesabın yeni postaları için kart ve bildirim gösterilir. */
  notify: boolean;
}

export interface NewAccount {
  name: string;
  email: string;
  displayName: string;
  color: string | null;
  imap: ServerConfig;
  smtp: ServerConfig;
  password: string;
}

export interface NewJmapAccount {
  name: string;
  email: string;
  displayName: string;
  color: string | null;
  sessionUrl: string;
  username: string;
  password: string;
}

export interface OAuthGrant {
  grantId: string;
  provider: AuthKind;
  email: string;
  displayName: string | null;
}

export interface NewOAuthAccount {
  grantId: string;
  name: string;
  displayName: string;
  color: string | null;
}

export interface Signature {
  /** Yeni imzada boş bırakılır; arka uç kimlik verir. */
  id: string;
  name: string;
  body: string;
}

export interface AccountUpdate {
  name: string;
  displayName: string;
  color: string;
  notify: boolean;
  signatures: Signature[];
  signatureNew: string | null;
  signatureReply: string | null;
}

/** Parolalı IMAP hesabının sunucu ayarları. */
export interface ServerUpdate {
  imap: ServerConfig;
  smtp: ServerConfig;
  /**
   * Sunucu adı, kapı ya da kullanıcı adı değişiyorsa zorunlu: kayıtlı parola
   * yeni bir sunucuya kendiliğinden gönderilmez. Boşsa kayıtlı parola denenir.
   */
  password: string | null;
}

export type DiscoverySource = 'autoconfig' | 'srv' | 'guess' | 'builtin';

export interface DiscoveredConfig {
  domain: string;
  imap: ServerConfig;
  smtp: ServerConfig;
  source: DiscoverySource;
  /** Doğrulanmamış kaynaktan (DNS SRV, tahmin) gelen sunucu e-posta alan adının dışında. */
  otherDomain?: boolean;
}

// ------------------------------------------------------------------- kutular

export type MailboxRole = 'inbox' | 'sent' | 'drafts' | 'archive' | 'trash' | 'junk' | 'other';

/** Bir klasöre elle atanabilen görev. */
export type SpecialRole = Exclude<MailboxRole, 'inbox' | 'other'>;

export interface Mailbox {
  id: string;
  accountId: AccountId;
  path: string;
  name: string;
  role: MailboxRole;
  unread: number;
  total: number;
  /** Üst klasörün kimliği; kökteyse (ya da üstü seçilemeyen bir düğümse) null. */
  parentId: string | null;
  /** Kenar çubuğunda gizli. Gizli özel klasör eşitlenmez; rol klasörü eşitlenmeye devam eder. */
  hidden: boolean;
}

// -------------------------------------------------------------------- postalar

export type Category = 'people' | 'notifications' | 'newsletters';

export interface Address {
  name: string | null;
  email: string;
}

export interface MessageSummary {
  id: MessageId;
  accountId: AccountId;
  mailboxId: string;
  threadId: string;
  from: Address;
  to: Address[];
  subject: string;
  snippet: string;
  date: Millis;
  unread: boolean;
  starred: boolean;
  hasAttachments: boolean;
  category: Category;
  snoozedUntil: Millis | null;
  /** Kullanıcı listede en üste sabitledi. Yalnız yerelde tutulur; sunucuya yazılmaz. */
  pinned: boolean;
  /** Postanın etiketlerinin kimlikleri (`Tag.id`). Yalnız yereldir. */
  tags: string[];
}

/** Kullanıcının yerel, renkli etiketi. Sunucuya yazılmaz; hesaplar arası ortaktır. */
export interface Tag {
  /** Yeni etikette boş; arka uç kimlik verir. */
  id: string;
  /** En çok TAG_NAME_CHARS karakter; etiketler arasında tek. */
  name: string;
  /** TAG_COLORS paletinden bir renk (#RRGGBB). */
  color: string;
}

/** Etiket renk paleti (hesap renkleri gibi kullanıcı verisi; jeton değil). Arka uçtaki `TAG_COLORS` ile birebir aynı. */
export const TAG_COLORS = ['#4C6FD8', '#2E9A61', '#D08A1E', '#9A5BD6', '#D0566E', '#2A9BA8', '#B3882A', '#6B7280'] as const;
export const TAG_NAME_CHARS = 40;
export const TAGS_MAX = 50;

export interface Attachment {
  index: number;
  name: string;
  mime: string;
  size: number;
}

export interface ThreadEntry {
  id: MessageId;
  from: Address;
  date: Millis;
  snippet: string;
}

export interface MessageDetail extends MessageSummary {
  cc: Address[];
  /** Yalnız taslaklarda dolu (gönderilmiş postada Bcc başlığı yoktur). */
  bcc: Address[];
  replyTo: Address[];
  messageIdHeader: string | null;
  references: string[];
  bodyText: string | null;
  bodyHtml: string | null;
  hasRemoteContent: boolean;
  remoteAllowed: boolean;
  attachments: Attachment[];
  thread: ThreadEntry[];
  /** List-Unsubscribe başlığı varsa abonelikten çıkma yolları. */
  unsubscribe: Unsubscribe | null;
}

export interface Unsubscribe {
  /** https (ya da http) adresi. */
  url: string | null;
  /** mailto adresi. */
  mailto: string | null;
  /** RFC 8058: List-Unsubscribe-Post ile tek tıkla çıkılabilir. */
  oneClick: boolean;
}

/** `done`: arka uç çıkışı yaptı; `browser`: arayüz `url`yi tarayıcıda açmalı. */
export type UnsubscribeResult = 'done' | 'browser';

export type View =
  | { kind: 'inbox' }
  | { kind: 'snoozed' }
  | { kind: 'starred' }
  | { kind: 'drafts' }
  | { kind: 'sent' }
  | { kind: 'archive' }
  | { kind: 'trash' }
  | { kind: 'mailbox'; id: string }
  /** Bu etiketi taşıyan postalar (çöp ve istenmeyen hariç). */
  | { kind: 'tag'; id: string };

/** Listede hızlı süzgeç. */
export type ListOnly = 'unread' | 'starred' | 'attachments';

export interface ListQuery {
  view: View;
  accountId: AccountId | null;
  category: Category | null;
  only?: ListOnly | null;
  cursor: string | null;
  limit: number;
}

export interface Page<T> {
  items: T[];
  nextCursor: string | null;
}

export interface Counts {
  inboxUnread: number;
  snoozed: number;
  drafts: number;
  /** Giden kutusunda bekleyen (planlanmış) gönderimler. */
  scheduled: number;
  perAccountUnread: Record<AccountId, number>;
}

export type MoveTarget = 'inbox' | 'archive' | 'trash';

// ------------------------------------------------------------------- gönderim

export interface OutgoingMessage {
  accountId: AccountId;
  to: Address[];
  cc: Address[];
  bcc: Address[];
  subject: string;
  bodyText: string;
  inReplyTo: MessageId | null;
  attachments: string[];
  sendAt: Millis | null;
  /** İletilen posta: ekleri bu postadan kopyalanır. */
  forwardFrom?: MessageId | null;
  /** Gönderilince silinecek ya da kaydederken yerine yazılacak taslak. */
  draftId?: MessageId | null;
}

export interface OutboxItem {
  id: string;
  sendAt: Millis;
}

/** Giden kutusundaki planlanmış gönderim. */
export interface ScheduledItem {
  id: string;
  accountId: AccountId;
  sendAt: Millis;
  to: Address[];
  subject: string;
  /** Son denemenin hatası (gönderilemediyse). */
  error: string | null;
}

// ------------------------------------------------------------------ kurallar

export type RuleText = 'from' | 'to' | 'subject' | 'body';
export type RuleOp = 'contains' | 'notContains' | 'is';

export type RuleCondition =
  | { kind: RuleText; op: RuleOp; value: string }
  | { kind: 'hasAttachment' }
  | { kind: 'category'; value: Category };

export type RuleAction =
  | { kind: 'move'; mailboxId: string }
  | { kind: 'archive' }
  | { kind: 'trash' }
  | { kind: 'markRead' }
  | { kind: 'star' }
  /** Kart ve bildirim gösterilmez. */
  | { kind: 'mute' }
  | { kind: 'forward'; to: string }
  /** Postaya yerel etiket ekler. */
  | { kind: 'tag'; tagId: string };

/** Gelen kutusuna yeni düşen postaya sırayla uygulanan kural. */
export interface Rule {
  /** Yeni kuralda boş; arka uç kimlik verir. */
  id: string;
  name: string;
  enabled: boolean;
  /** null: tüm hesaplar (klasöre taşıma için hesap zorunlu). */
  accountId: AccountId | null;
  match: 'all' | 'any';
  conditions: RuleCondition[];
  actions: RuleAction[];
  /** Eşleşirse sonraki kurallar denenmez. */
  stop: boolean;
}

// -------------------------------------------------------------- otomatik yanıt

/** Sunucu tarafı otomatik yanıt (JMAP VacationResponse). */
export interface Vacation {
  enabled: boolean;
  fromDate: Millis | null;
  toDate: Millis | null;
  subject: string;
  body: string;
}

/** Yazma penceresine eklenen hazır metin. */
export interface Template {
  /** Yeni şablonda boş; arka uç kimlik verir. */
  id: string;
  name: string;
  subject: string;
  body: string;
}

export type ComposeMode = 'new' | 'reply' | 'replyAll' | 'forward' | 'draft';

export interface ComposePrefill {
  mode: ComposeMode;
  messageId: MessageId | null;
}

// ------------------------------------------------------------------ eşitleme

export type SyncState =
  | { state: 'online' }
  | { state: 'syncing'; done: number; total: number }
  | { state: 'offline' }
  | { state: 'error'; message: string }
  | { state: 'untrusted'; host: string; port: number };

export type SyncStatus = { accountId: AccountId } & SyncState;

// -------------------------------------------------------------------- ayarlar

export type Theme = 'system' | 'light' | 'dark';
export type Language = 'tr' | 'en';
export type NotifyMode = 'people' | 'all' | 'vip' | 'off';
export type CardPosition = 'bottomRight' | 'topRight' | 'bottomLeft' | 'topLeft';
export type Density = 'comfortable' | 'compact';

export interface Settings {
  theme: Theme;
  language: Language;
  notify: NotifyMode;
  vip: string[];
  quietStart: string | null;
  quietEnd: string | null;
  cardSeconds: number;
  cardPosition: CardPosition;
  nativeNotifications: boolean;
  undoSendSeconds: number;
  density: Density;
  remoteAllow: string[];
  autoUpdate: boolean;
  trustedCerts: TrustedCert[];
  /** Hızlı yanıt düğmeleri; null ise dile göre varsayılanlar. */
  quickReplies: string[] | null;
  /** Koyu temada kendi zemini olmayan postalar koyu zeminde gösterilir. */
  darkMail: boolean;
  templates: Template[];
  /** Kenar çubuğundan tek tıkla çalıştırılan aramalar (en çok SAVED_SEARCHES_MAX). */
  savedSearches: SavedSearch[];
}

/** Kayıtlı arama: ad ve arama sorgusu (işleçler dahil). */
export interface SavedSearch {
  /** Yeni kayıtta boş; arka uç kimlik verir. */
  id: string;
  name: string;
  query: string;
}

/** Kayıtlı arama sınırları (arka uçla aynı). */
export const SAVED_SEARCHES_MAX = 30;
export const SAVED_SEARCH_NAME_CHARS = 60;
export const SAVED_SEARCH_QUERY_CHARS = 300;

/** Hızlı yanıt sınırları (arka uçla aynı). */
export const QUICK_REPLIES_MAX = 8;
export const QUICK_REPLY_CHARS = 120;

export interface TrustedCert {
  host: string;
  port: number;
  sha256: string;
}

export interface CertInfo {
  host: string;
  port: number;
  sha256: string;
  subject: string;
  issuer: string;
  notBefore: Millis | null;
  notAfter: Millis | null;
  names: string[];
  /** Neden güvenilmedi (Türkçe, kısa). */
  problem: string;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
  date: string | null;
}

export const defaultSettings: Settings = {
  theme: 'system',
  language: 'tr',
  notify: 'people',
  vip: [],
  quietStart: '22:00',
  quietEnd: '08:00',
  cardSeconds: 8,
  cardPosition: 'bottomRight',
  nativeNotifications: false,
  undoSendSeconds: 10,
  density: 'comfortable',
  remoteAllow: [],
  autoUpdate: true,
  trustedCerts: [],
  quickReplies: null,
  darkMail: true,
  templates: [],
  savedSearches: [],
};

// ------------------------------------------------------------------- olaylar

export const EVENTS = {
  notify: 'mh:notify',
  sync: 'mh:sync',
  changed: 'mh:changed',
  outbox: 'mh:outbox',
  settings: 'mh:settings',
  open: 'mh:open',
  update: 'mh:update',
} as const;

export interface NotifyEvent {
  message: MessageSummary;
  accountName: string;
  accountColor: string;
}

/** Ana pencere bu postayı açmalı (karttan ya da hızlı okumadan). */
export interface OpenEvent {
  id: MessageId;
}

export interface ChangedEvent {
  mailboxIds: string[];
}

export interface OutboxEvent {
  id: string;
  ok: boolean;
  error: string | null;
}

export interface EventPayloads {
  'mh:notify': NotifyEvent;
  'mh:sync': SyncStatus;
  'mh:changed': ChangedEvent;
  'mh:outbox': OutboxEvent;
  'mh:settings': Settings;
  'mh:open': OpenEvent;
  'mh:update': UpdateInfo;
}

// -------------------------------------------------------------------- hatalar

export type ErrorCode =
  | 'notFound'
  | 'auth'
  | 'network'
  | 'tls'
  | 'untrusted'
  | 'discovery'
  | 'keyring'
  | 'storage'
  | 'invalid'
  | 'internal';

export interface AppError {
  code: ErrorCode;
  message: string;
}

// ------------------------------------------------------------------- komutlar

/** Tauri komutları: ad → [argümanlar, dönüş]. */
export interface Commands {
  accounts_list: [Record<string, never>, Account[]];
  account_discover: [{ email: string }, DiscoveredConfig];
  account_add: [{ input: NewAccount }, Account];
  account_remove: [{ id: AccountId }, null];
  mailboxes_list: [Record<string, never>, Mailbox[]];
  counts: [Record<string, never>, Counts];
  messages_list: [{ query: ListQuery }, Page<MessageSummary>];
  message_get: [{ id: MessageId }, MessageDetail];
  messages_flag: [{ ids: MessageId[]; unread: boolean | null; starred: boolean | null }, null];
  messages_move: [{ ids: MessageId[]; target: MoveTarget }, null];
  /** Postaları kendi hesabının herhangi bir klasörüne taşır. */
  messages_move_to: [{ ids: MessageId[]; mailboxId: string }, null];
  messages_snooze: [{ ids: MessageId[]; until: Millis | null }, null];
  search: [{ q: string; limit: number }, MessageSummary[]];
  remote_allow: [{ id: MessageId }, MessageDetail];
  /** Eki geçici klasöre yazar, varsayılan uygulamayla açar; dosya yolunu döner. */
  attachment_open: [{ id: MessageId; index: number }, string];
  send: [{ msg: OutgoingMessage }, OutboxItem];
  send_cancel: [{ id: string }, boolean];
  settings_get: [Record<string, never>, Settings];
  settings_set: [{ settings: Settings }, Settings];
  sync_now: [Record<string, never>, null];
  open_main: [{ id: MessageId | null }, null];
  open_quick: [{ id: MessageId }, null];
  open_compose: [{ prefill: ComposePrefill }, null];
  notify_layout: [{ height: number }, null];
  notify_hide: [Record<string, never>, null];
  /** Bildirim penceresi açılırken kuyrukta bekleyen kartlar (açılış yarışında kaybolmasınlar). */
  notify_pending: [Record<string, never>, NotifyEvent[]];
  notify_snooze: [{ minutes: number }, null];
  demo_notify: [Record<string, never>, null];

  // JMAP (Stalwart vb.)
  /** Adresin alan adında JMAP oturum adresini arar (/.well-known/jmap). */
  jmap_discover: [{ email: string }, string];
  account_add_jmap: [{ input: NewJmapAccount }, Account];

  // Google / Microsoft OAuth 2
  /** Bu derlemede istemci kimliği tanımlı sağlayıcılar. */
  oauth_providers: [Record<string, never>, AuthKind[]];
  /** Tarayıcıda girişi başlatır, dönüşü bekler; jetonlar arka uçta kalır. */
  oauth_sign_in: [{ provider: AuthKind; email: string | null }, OAuthGrant];
  account_add_oauth: [{ input: NewOAuthAccount }, Account];

  account_update: [{ id: AccountId; update: AccountUpdate }, Account];
  /** Parolalı IMAP hesabının sunucularını değiştirir; önce bağlantıyı dener. */
  account_update_server: [{ id: AccountId; update: ServerUpdate }, Account];
  /** Parolayı (ya da uygulama parolasını) değiştirir; önce girişi dener. */
  account_set_password: [{ id: AccountId; password: string }, null];
  /** Yalnız hızlı yanıtları yazar (hızlı okuma penceresi ayarların geri kalanına erişemez). */
  quick_replies_set: [{ replies: string[] | null }, Settings];

  // Kurallar
  rules_list: [Record<string, never>, Rule[]];
  /** Kaydeder (id boşsa yeni); doğrulanmış kuralı döner. */
  rule_save: [{ rule: Rule }, Rule];
  rule_delete: [{ id: string }, null];
  rules_reorder: [{ ids: string[] }, null];
  /** Kuralı gelen kutusundaki mevcut postalara uygular (iletme hariç); eşleşen posta sayısı. */
  rule_run: [{ id: string }, number];

  // Planlanan gönderimler
  outbox_list: [Record<string, never>, ScheduledItem[]];
  outbox_send_now: [{ id: string }, null];
  /** Gönderimi iptal edip taslağa çevirir; taslağın kimliği. */
  outbox_to_draft: [{ id: string }, MessageId];

  // Abonelik ve otomatik yanıt
  unsubscribe: [{ id: MessageId }, UnsubscribeResult];
  /** Desteklenmiyorsa (IMAP) null. */
  vacation_get: [{ accountId: AccountId }, Vacation | null];
  vacation_set: [{ accountId: AccountId; vacation: Vacation }, Vacation];

  // Klasörler
  /** Kenar çubuğunda gösterir ya da gizler (gelen kutusu gizlenemez). */
  mailbox_set_hidden: [{ id: string; hidden: boolean }, null];
  /** Klasörü Gönderilenler, Taslaklar, Arşiv, Çöp ya da Gereksiz yapar; rolün önceki sahibi sıradan klasöre döner. */
  mailbox_set_role: [{ id: string; role: SpecialRole }, null];
  /** Hesabın elle atanan klasör görevlerini kaldırır; roller yeniden sunucudan bulunur. */
  mailbox_roles_reset: [{ accountId: AccountId }, null];

  // Sertifika onayı
  cert_inspect: [{ host: string; port: number; security: Security }, CertInfo];
  cert_trust: [{ host: string; port: number; sha256: string }, null];

  // Taslaklar
  /** Taslağı kaydeder (msg.draftId varsa yerine yazar); taslağın kimliğini döner. */
  draft_save: [{ msg: OutgoingMessage }, MessageId];
  draft_delete: [{ id: MessageId }, null];
  /** İşletim sisteminin dosya penceresini açar; seçilen dosyaların yolları. */
  attachments_pick: [Record<string, never>, string[]];

  // Güncelleme
  update_check: [Record<string, never>, UpdateInfo | null];
  /** İndirir, kurar ve uygulamayı yeniden başlatır. */
  update_install: [Record<string, never>, null];

  // Liste: göndericiye göre seçim, sabitleme, etiketler (hepsi yerel)
  /**
   * Gönderenin kapsamdaki postaları, yeniden eskiye. Kapsam: `view` klasör ya da
   * etiket görünümüyse o, yoksa gelen kutusu; `accountId` verilirse yalnız o hesap.
   */
  messages_by_sender: [{ accountId: AccountId | null; email: string; view: View | null }, MessageId[]];
  /** Sabitler ya da sabitlemeyi kaldırır; sunucuya yazılmaz. */
  messages_pin: [{ ids: MessageId[]; pinned: boolean }, null];
  tags_list: [Record<string, never>, Tag[]];
  /** Kaydeder (id boşsa yeni); sadeleştirilmiş etiketi döner. */
  tag_save: [{ tag: Tag }, Tag];
  /** Etiketi siler; postalardan da kalkar. */
  tag_delete: [{ id: string }, null];
  /** Etiketi postalara ekler (`on`) ya da kaldırır. */
  messages_tag: [{ ids: MessageId[]; tagId: string; on: boolean }, null];
}

export type CommandName = keyof Commands;
export type CommandArgs<K extends CommandName> = Commands[K][0];
export type CommandResult<K extends CommandName> = Commands[K][1];
