// Klavye kısayolları (docs/TASARIM.md §8). Saf eşleme; olay bağlama bileşende.

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
  | 'sync';

export interface KeyLike {
  key: string;
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
  '#': 'delete',
};

export function shortcutFor(e: KeyLike): Shortcut | null {
  const mod = !!(e.ctrlKey || e.metaKey);
  const key = e.key.length === 1 ? e.key.toLocaleLowerCase('en') : e.key;
  if (mod && !e.altKey && key === 'k') return 'palette';
  if (key === 'Escape') return 'escape';
  // Metin yazarken de çalışır; tarayıcının sayfa yenilemesinin yerine geçer.
  if (key === 'F5' && !mod && !e.altKey && !e.shiftKey) return 'sync';
  if (e.editing) return null;
  if (mod && !e.altKey && !e.shiftKey && key === 'z') return 'undo';
  if (mod && !e.altKey && !e.shiftKey && key === 'a') return 'selectAll';
  if (mod || e.altKey) return null;
  if (key === 'Delete') return 'delete';
  if (key === 'Enter') return 'open';
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
