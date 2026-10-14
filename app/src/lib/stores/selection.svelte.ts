// Listede çoklu seçim: Ctrl/Cmd+tık tek tek, Shift+tık aralık, Ctrl A görünen
// tümü, Esc bırakır; Shift+J/K klavyeyle genişletir. İşaretli satırlar
// `mail.checked` içindedir (satır = konuşmanın en yeni iletisi); toplu işlem
// satırların kapsadığı tüm ileti kimliklerine uygulanır.

import type { MessageId } from '../api';
import { currentRows, mail, select, type Row } from './mail.svelte';

/** Son tıklanan satır; Shift+tık aralığın başlangıcı buradan alınır. */
let anchor: MessageId | null = null;

/** İşaretli satırlar, liste sırasıyla. */
export function checkedRows(): Row[] {
  if (!mail.checked.length) return [];
  const set = new Set(mail.checked);
  return currentRows().filter((r) => set.has(r.id));
}

/** İşaretli satırların kapsadığı tüm ileti kimlikleri (konuşmalar dahil). */
export function checkedIds(): MessageId[] {
  return checkedRows().flatMap((r) => r.ids);
}

/** Toplu işlem paneli görünsün mü (iki ya da daha çok satır)? */
export function multi(): boolean {
  return mail.checked.length > 1;
}

export function isChecked(rowId: MessageId): boolean {
  return mail.checked.includes(rowId);
}

export function clearChecked() {
  mail.checked = [];
  anchor = null;
}

/** Ctrl/Cmd+tık: satırı işaretler ya da işareti kaldırır. */
export function toggleCheck(rowId: MessageId) {
  mail.checked = isChecked(rowId) ? mail.checked.filter((id) => id !== rowId) : [...mail.checked, rowId];
  anchor = rowId;
}

/** Shift+tık: son tıklanan (yoksa okunan) satırdan bu satıra kadar işaretler. */
export function checkRange(rowId: MessageId) {
  const rows = currentRows();
  // Çapa görünüm değişince listede kalmamışsa yok sayılır; okunan satırdan başlanır.
  const from =
    (anchor != null && rows.some((r) => r.id === anchor) ? anchor : null) ??
    rows.find((r) => mail.selectedId != null && r.ids.includes(mail.selectedId))?.id ??
    rowId;
  const a = rows.findIndex((r) => r.id === from);
  const b = rows.findIndex((r) => r.id === rowId);
  if (a < 0 || b < 0) {
    toggleCheck(rowId);
    return;
  }
  const [lo, hi] = a < b ? [a, b] : [b, a];
  const range = rows.slice(lo, hi + 1).map((r) => r.id);
  mail.checked = [...new Set([...mail.checked, ...range])];
}

/** Ctrl A: görünen tüm satırlar. */
export function checkAll() {
  mail.checked = currentRows().map((r) => r.id);
}

/**
 * Shift+J / Shift+K: okunan satırı ve komşusunu işaretler, odağı komşuya
 * taşır (okundu saymaz). Listenin ucunda durur.
 */
export function extendChecked(delta: 1 | -1) {
  const rows = currentRows();
  if (!rows.length) return;
  const i = rows.findIndex((r) => mail.selectedId != null && r.ids.includes(mail.selectedId));
  const cur = rows[Math.max(i, 0)];
  const next = rows[Math.min(Math.max(i + delta, 0), rows.length - 1)];
  mail.checked = [...new Set([...mail.checked, cur.id, next.id])];
  anchor = next.id;
  if (next.id !== mail.selectedId) void select(next.id, { markRead: false });
}

/** Düz tık: çoklu seçim biter, satır okunur. */
export function openRow(rowId: MessageId) {
  clearChecked();
  anchor = rowId;
  void select(rowId);
}

/** Satır tıklaması: değiştirici tuşa göre işaretler ya da açar. */
export function clickRow(e: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean }, rowId: MessageId) {
  if (e.ctrlKey || e.metaKey) toggleCheck(rowId);
  else if (e.shiftKey) checkRange(rowId);
  else openRow(rowId);
}
