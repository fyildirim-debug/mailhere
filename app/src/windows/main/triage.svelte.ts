// Odak (tek tek işleme) kipi: açıldığı andaki listenin satırları sırayla tam
// genişlikte okunur; her eylemden sonra sıradaki işlenmemiş posta gelir.
// İlerleme "işlenen + 1 / toplam"; hepsi bitince boş durum. Geri alma mevcut
// toast'la çalışır: geri alınan posta yeniden gösterilir ve işlenmemiş sayılır.

import type { Mailbox, MessageId } from '../../lib/api';
import { currentRows, mail, moveTo, moveToFolder, select, selectedIds, snooze, type Row } from '../../lib/stores/mail.svelte';
import { clearChecked } from '../../lib/stores/selection.svelte';
import { afterToastAction, lastToastId } from '../../lib/ui/toast.svelte';

export const triage = $state({
  on: false,
  /** Açılıştaki satırlar (satırın en yeni iletisinin kimliği), sırasıyla. */
  queue: [] as MessageId[],
  /** İşlenen ya da atlanan satırlar. */
  handled: [] as MessageId[],
  /** Kipin göstermek istediği satır; geri almayı otomatik seçimden ayırmak için. */
  cursor: null as MessageId | null,
});

/** Okunan iletiyi içeren satır. */
function rowOf(id: MessageId | null, rows: Row[] = currentRows()): Row | undefined {
  return id == null ? undefined : rows.find((r) => r.ids.includes(id));
}

export function currentRowId(): MessageId | null {
  return rowOf(mail.selectedId)?.id ?? null;
}

export function finished(): boolean {
  return triage.on && triage.cursor == null;
}

/** İlerleme: şu anki postanın sırası (bitince toplam) ve toplam. */
export function progress(): { n: number; total: number } {
  const total = triage.queue.length;
  return { n: Math.min(triage.handled.length + (finished() ? 0 : 1), total), total };
}

function show(row: Row | undefined) {
  triage.cursor = row?.id ?? null;
  if (row) void select(row.id);
  else {
    // Bitti: takeOut'un seçtiği satır gösterilmez; geri alınca yeniden seçilir.
    mail.selectedId = null;
  }
}

/** Sıradaki işlenmemiş satır: önce bulunulan yerden sonra, sonra baştan. */
function nextAfter(from: MessageId | null): Row | undefined {
  const rows = currentRows();
  const live = new Map(rows.map((r) => [r.id, r]));
  const open = triage.queue.filter((id) => live.has(id) && !triage.handled.includes(id));
  const at = from == null ? -1 : triage.queue.indexOf(from);
  const after = open.find((id) => triage.queue.indexOf(id) > at) ?? open[0];
  return after == null ? undefined : live.get(after);
}

export function startTriage() {
  const rows = currentRows();
  clearChecked();
  triage.queue = rows.map((r) => r.id);
  triage.handled = [];
  triage.on = true;
  // Okunan posta varsa ondan başlanır.
  show(rowOf(mail.selectedId, rows) ?? rows[0]);
}

export function stopTriage() {
  triage.on = false;
  triage.queue = [];
  triage.handled = [];
  const cur = triage.cursor;
  triage.cursor = null;
  // Bitmişse listenin başı seçilir; okuma alanı boş kalmasın.
  if (cur == null && mail.selectedId == null) {
    const first = currentRows()[0];
    if (first) void select(first.id, { markRead: false });
  }
}

/** Satırı işlenmiş sayar ve sıradakine geçer. */
function advance(from: MessageId) {
  if (!triage.handled.includes(from)) triage.handled = [...triage.handled, from];
  show(nextAfter(from));
}

/** J: atla. */
export function skip() {
  const cur = triage.cursor;
  if (cur != null) advance(cur);
}

/** K: bir önceki satıra döner (işlenmemiş sayılır). */
export function back() {
  const rows = currentRows();
  const live = new Set(rows.map((r) => r.id));
  const prev = [...triage.handled].reverse().find((id) => live.has(id));
  if (prev == null) return;
  triage.handled = triage.handled.filter((id) => id !== prev);
  show(rowOf(prev, rows));
}

/** Geri alınan satır yeniden işlenmemiş sayılır ve gösterilir (geri alma onu zaten seçer). */
function restore(row: MessageId) {
  if (!triage.on) return;
  triage.handled = triage.handled.filter((id) => id !== row);
  triage.cursor = row;
}

/**
 * Postayı listeden çıkaran eylem (arşivle, sil, ertele, taşı): eylem önce
 * çalışır (iyimser), sonra sıradaki posta seçilir. Eylemin geri alma
 * toast'ı postayı kipe de geri getirir.
 */
function leave(run: (ids: MessageId[]) => Promise<void>): Promise<void> {
  const cur = triage.cursor;
  if (cur == null) return Promise.resolve();
  const ids = selectedIds();
  const before = lastToastId();
  const done = run(ids);
  advance(cur);
  return done.then(() => {
    const id = lastToastId();
    if (id !== before) afterToastAction(id, () => restore(cur));
  });
}

export const archiveCurrent = () => leave((ids) => moveTo(ids, 'archive'));
export const deleteCurrent = () => leave((ids) => moveTo(ids, 'trash'));
export const snoozeCurrent = (until: number) => leave((ids) => snooze(ids, until));
export const moveCurrent = (box: Mailbox, label: string) => leave((ids) => moveToFolder(ids, box, label));
