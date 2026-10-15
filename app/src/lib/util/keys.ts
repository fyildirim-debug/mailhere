// Klavye kısayolları (docs/TASARIM.md §8). Saf eşleme; olay bağlama bileşende.

import type { MessageKey } from '../i18n/tr';

export type Shortcut =
  | 'compose'
  | 'reply'
  | 'replyAll'
  | 'forward'
  | 'archive'
  | 'snooze'
  | 'star'
  | 'unread'
  | 'delete'
  | 'next'
  | 'prev'
  | 'open'
  | 'palette'
  | 'undo'
  | 'escape'
  /** P: sabitle / sabitlemeyi kaldır. */
  | 'pin'
  /** Ctrl A: görünen tüm satırları seç. */
  | 'selectAll'
  /** Shift J / Shift ↓: seçimi sonraki satıra genişlet. */
  | 'extendNext'
  /** Shift K / Shift ↑: seçimi önceki satıra genişlet. */
  | 'extendPrev'
  /** F5: postaları hemen al (sayfa yenilenmez). */
  | 'sync'
  /** Shift O: tek tek işleme (odak) kipi. */
  | 'triage'
  /** V: klasöre taşı seçicisi. */
  | 'movePicker'
  /** L: etiket seçicisi. */
  | 'tagPicker'
  /** ?: kısayol sayfası. */
  | 'help'
  /** /: arama (komut paleti). */
  | 'search'
  /** G: iki tuşlu gezinmenin ilk tuşu (G sonra I …). */
  | 'go'
  /** Ctrl B: kenar çubuğunu daralt / katman olarak aç. */
  | 'sidebar'
  /** Ctrl + / Ctrl - / Ctrl 0: arayüz ölçeği. */
  | 'zoomIn'
  | 'zoomOut'
  | 'zoomReset';

export interface KeyLike {
  key: string;
  /** Fiziksel tuş (sayısal tuş takımı ve düzene bağlı + / - için). */
  code?: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  /** Odak bir metin alanında mı? */
  editing?: boolean;
}

const SINGLE: Record<string, Shortcut> = {
  c: 'compose',
  r: 'reply',
  a: 'replyAll',
  f: 'forward',
  e: 'archive',
  h: 'snooze',
  s: 'star',
  u: 'unread',
  j: 'next',
  k: 'prev',
  p: 'pin',
  v: 'movePicker',
  l: 'tagPicker',
  g: 'go',
  '#': 'delete',
};

export function shortcutFor(e: KeyLike): Shortcut | null {
  const mod = !!(e.ctrlKey || e.metaKey);
  const key = e.key.length === 1 ? e.key.toLocaleLowerCase('en') : e.key;
  if (mod && !e.altKey && key === 'k') return 'palette';
  if (key === 'Escape') return 'escape';
  // Metin yazarken de çalışır; tarayıcının sayfa yenilemesinin yerine geçer.
  if (key === 'F5' && !mod && !e.altKey && !e.shiftKey) return 'sync';
  // Ölçek tarayıcıdaki gibi her yerde çalışır (düzenden bağımsız: = / + ve sayısal tuşlar).
  if (mod && !e.altKey) {
    if (key === '+' || key === '=' || e.code === 'NumpadAdd') return 'zoomIn';
    if (key === '-' || key === '_' || e.code === 'NumpadSubtract') return 'zoomOut';
    if (key === '0' || e.code === 'Numpad0') return 'zoomReset';
  }
  if (e.editing) return null;
  if (mod && !e.altKey && !e.shiftKey && key === 'z') return 'undo';
  if (mod && !e.altKey && !e.shiftKey && key === 'a') return 'selectAll';
  if (mod && !e.altKey && !e.shiftKey && key === 'b') return 'sidebar';
  if (mod || e.altKey) return null;
  // Düzene göre Shift isteyen işaretler: karakterin kendisine bakılır.
  if (key === '?') return 'help';
  if (key === '/') return 'search';
  if (key === 'Delete') return 'delete';
  if (key === 'Enter') return 'open';
  if (e.shiftKey && key === 'o') return 'triage';
  // Shift ile gezinme seçimi genişletir.
  if (e.shiftKey && (key === 'j' || key === 'ArrowDown')) return 'extendNext';
  if (e.shiftKey && (key === 'k' || key === 'ArrowUp')) return 'extendPrev';
  if (key === 'ArrowDown') return 'next';
  if (key === 'ArrowUp') return 'prev';
  return SINGLE[key] ?? null;
}

export function isEditing(target: EventTarget | null): boolean {
  if (!target || typeof (target as HTMLElement).closest !== 'function') return false;
  const el = target as HTMLElement;
  return !!el.closest('input, textarea, select, [contenteditable="true"]');
}

// ------------------------------------------------------------ iki tuşlu gezinme

/** G'den sonra gidilen görünümler. */
export type GoTarget = 'inbox' | 'starred' | 'sent' | 'drafts' | 'archive' | 'snoozed' | 'trash';

/**
 * G sonra: I gelen kutusu, S yıldızlılar, T gönderilenler (sent), D taslaklar,
 * A arşiv, H ertelenenler (H ertele), # çöp (# sil). İkinci tuş tek tuşlarla
 * çakışmaz: G'den sonra yalnız bu harita okunur.
 */
export const GO_KEYS: { key: string; target: GoTarget; label: MessageKey }[] = [
  { key: 'I', target: 'inbox', label: 'nav.inbox' },
  { key: 'S', target: 'starred', label: 'nav.starred' },
  { key: 'T', target: 'sent', label: 'nav.sent' },
  { key: 'D', target: 'drafts', label: 'nav.drafts' },
  { key: 'A', target: 'archive', label: 'nav.archive' },
  { key: 'H', target: 'snoozed', label: 'nav.snoozed' },
  { key: '#', target: 'trash', label: 'nav.trash' },
];

/** G'den sonraki tuşun hedefi; tanınmazsa null (zincir biter). */
export function goTarget(e: KeyLike): GoTarget | null {
  if (e.ctrlKey || e.metaKey || e.altKey) return null;
  const key = e.key.length === 1 ? e.key.toLocaleUpperCase('en') : e.key;
  return GO_KEYS.find((g) => g.key === key)?.target ?? null;
}

/** G zincirinin ne kadar bekleyeceği (ms). */
export const GO_TIMEOUT = 1500;

// ------------------------------------------------------------ gösterilen adlar

/** Kısayolun arayüzde yazılışı (kısayol çipi, palet, ipucu). */
export const KEY_LABEL: Partial<Record<Shortcut, string>> = {
  compose: 'C',
  reply: 'R',
  replyAll: 'A',
  forward: 'F',
  archive: 'E',
  snooze: 'H',
  star: 'S',
  unread: 'U',
  delete: '#',
  pin: 'P',
  next: 'J',
  prev: 'K',
  open: 'Enter',
  palette: 'Ctrl K',
  undo: 'Ctrl Z',
  sync: 'F5',
  triage: 'Shift O',
  movePicker: 'V',
  tagPicker: 'L',
  help: '?',
  search: '/',
  sidebar: 'Ctrl B',
  zoomIn: 'Ctrl +',
  zoomOut: 'Ctrl -',
  zoomReset: 'Ctrl 0',
};

/** Bir G hedefinin yazılışı, ör. "G I". */
export function goLabel(target: GoTarget): string {
  return `G ${GO_KEYS.find((g) => g.target === target)?.key ?? ''}`;
}

export interface KeyRow {
  keys: string;
  label: MessageKey;
}

/** Kısayol sayfası ve Hakkında › Kısayollar: tek kaynak. */
export const KEY_GROUPS: { title: MessageKey; rows: KeyRow[] }[] = [
  {
    title: 'keys.group.actions',
    rows: [
      { keys: 'C', label: 'keys.compose' },
      { keys: 'R', label: 'keys.reply' },
      { keys: 'A', label: 'keys.replyAll' },
      { keys: 'F', label: 'keys.forward' },
      { keys: 'E', label: 'keys.archive' },
      { keys: '# / Del', label: 'keys.delete' },
      { keys: 'H', label: 'keys.snooze' },
      { keys: 'S', label: 'keys.star' },
      { keys: 'U', label: 'keys.unread' },
      { keys: 'P', label: 'keys.pin' },
      { keys: 'V', label: 'keys.movePicker' },
      { keys: 'L', label: 'keys.tagPicker' },
      { keys: 'Ctrl Z', label: 'keys.undo' },
    ],
  },
  {
    title: 'keys.group.navigate',
    rows: [
      { keys: 'J / K', label: 'keys.move' },
      { keys: 'Enter', label: 'keys.open' },
      { keys: 'Shift J / K', label: 'keys.extend' },
      { keys: 'Ctrl A', label: 'keys.selectAll' },
      ...GO_KEYS.map((g) => ({ keys: `G ${g.key}`, label: `keys.go.${g.target}` as MessageKey })),
      { keys: 'Esc', label: 'keys.escape' },
    ],
  },
  {
    title: 'keys.group.app',
    rows: [
      { keys: 'Ctrl K', label: 'keys.palette' },
      { keys: '/', label: 'keys.search' },
      { keys: 'Shift O', label: 'keys.triage' },
      { keys: 'Ctrl B', label: 'keys.sidebar' },
      { keys: 'Ctrl + / -', label: 'keys.zoom' },
      { keys: 'Ctrl 0', label: 'keys.zoomReset' },
      { keys: 'F5', label: 'keys.sync' },
      { keys: 'Ctrl ,', label: 'keys.settings' },
      { keys: '?', label: 'keys.help' },
    ],
  },
];
