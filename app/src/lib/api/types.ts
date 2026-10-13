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
  signature: string | null;
  createdAt: Millis;
  auth: AuthKind;
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

export interface AccountUpdate {
  name: string;
  displayName: string;
  color: string;
  signature: string | null;
}

export type DiscoverySource = 'autoconfig' | 'srv' | 'guess' | 'builtin';

export interface DiscoveredConfig {
  domain: string;
  imap: ServerConfig;
  smtp: ServerConfig;
  source: DiscoverySource;
}

// ------------------------------------------------------------------- kutular

export type MailboxRole = 'inbox' | 'sent' | 'drafts' | 'archive' | 'trash' | 'junk' | 'other';

export interface Mailbox {
  id: string;
  accountId: AccountId;
  path: string;
  name: string;
  role: MailboxRole;
  unread: number;
  total: number;
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
}

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
  replyTo: Address[];
  messageIdHeader: string | null;
  references: string[];
  bodyText: string | null;
  bodyHtml: string | null;
  hasRemoteContent: boolean;
  remoteAllowed: boolean;
  attachments: Attachment[];
  thread: ThreadEntry[];
}

export type View =
  | { kind: 'inbox' }
  | { kind: 'snoozed' }
  | { kind: 'starred' }
  | { kind: 'drafts' }
  | { kind: 'sent' }
  | { kind: 'archive' }
  | { kind: 'trash' }
  | { kind: 'mailbox'; id: string };

export interface ListQuery {
  view: View;
  accountId: AccountId | null;
  category: Category | null;
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
}

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

  // Sertifika onayı
  cert_inspect: [{ host: string; port: number; security: Security }, CertInfo];
  cert_trust: [{ host: string; port: number; sha256: string }, null];

  // Taslaklar
  /** Taslağı kaydeder (msg.draftId varsa yerine yazar); taslağın kimliğini döner. */
  draft_save: [{ msg: OutgoingMessage }, MessageId];
  draft_delete: [{ id: MessageId }, null];

  // Güncelleme
  update_check: [Record<string, never>, UpdateInfo | null];
  /** İndirir, kurar ve uygulamayı yeniden başlatır. */
  update_install: [Record<string, never>, null];
}

export type CommandName = keyof Commands;
export type CommandArgs<K extends CommandName> = Commands[K][0];
export type CommandResult<K extends CommandName> = Commands[K][1];
