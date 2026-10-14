// Sanal liste yerleşimi: grup başlıkları ve satırlar için sabit yükseklikli
// konum hesabı, görünür dilim ve seçili satırı görünür tutma. Saf işlevler;
// masaüstü (MessageList) ve mobil (MobileList) listeleri birlikte kullanır.

export type LayoutEntry =
  | { kind: 'group'; key: string; label: string; top: number; h: number }
  | { kind: 'row'; key: string; index: number; top: number; h: number };

export interface Layout {
  entries: LayoutEntry[];
  /** Kaydırılan alanın toplam yüksekliği (alt boşluk dahil). */
  total: number;
}

export interface LayoutOptions<T, G extends string = string> {
  rowH: number;
  groupH: number;
  /** Listenin altında bırakılan boşluk. */
  tail?: number;
  /** Satırın grup anahtarı; ardışık satırlarda değişince başlık eklenir. */
  groupOf: (row: T, index: number) => G;
  groupLabel: (group: G) => string;
  rowKey: (row: T) => string;
}

/**
 * Satırları sırayla yerleştirir. Liste tarihe göre sıralı olmayabilir
 * (arama, hatırlatma); başlık anahtarı bu yüzden konuma bağlıdır (`g-<index>`).
 */
export function buildLayout<T, G extends string = string>(rows: T[], opts: LayoutOptions<T, G>): Layout {
  const entries: LayoutEntry[] = [];
  let top = 0;
  let last: G | null = null;
  rows.forEach((row, index) => {
    const g = opts.groupOf(row, index);
    if (g !== last) {
      entries.push({ kind: 'group', key: `g-${index}`, label: opts.groupLabel(g), top, h: opts.groupH });
      top += opts.groupH;
      last = g;
    }
    entries.push({ kind: 'row', key: opts.rowKey(row), index, top, h: opts.rowH });
    top += opts.rowH;
  });
  return { entries, total: top + (opts.tail ?? 16) };
}

/** `y` konumunu kapsayan ya da ondan sonraki ilk girdinin dizini (ikili arama). */
export function firstVisible(entries: LayoutEntry[], y: number): number {
  let lo = 0;
  let hi = entries.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (entries[mid].top + entries[mid].h < y) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** Görünür pencereye (ve iki yanındaki `overscan` payına) düşen girdiler. */
export function visibleEntries(entries: LayoutEntry[], scrollTop: number, viewport: number, overscan: number): LayoutEntry[] {
  if (!entries.length) return [];
  const start = firstVisible(entries, scrollTop - overscan);
  const end = scrollTop + viewport + overscan;
  const out: LayoutEntry[] = [];
  for (let i = start; i < entries.length && entries[i].top < end; i++) out.push(entries[i]);
  return out;
}

/**
 * Satırı (varsa üstündeki grup başlığıyla birlikte) görünür tutmak için
 * gereken kaydırma konumu; zaten görünüyorsa null.
 */
export function scrollToReveal(layout: Layout, rowIndex: number, scrollTop: number, viewport: number): number | null {
  const i = layout.entries.findIndex((x) => x.kind === 'row' && x.index === rowIndex);
  if (i < 0) return null;
  const e = layout.entries[i];
  const above = layout.entries[i - 1];
  const top = above?.kind === 'group' ? above.top : e.top;
  if (top < scrollTop) return top;
  if (e.top + e.h > scrollTop + viewport) return e.top + e.h - viewport;
  return null;
}
