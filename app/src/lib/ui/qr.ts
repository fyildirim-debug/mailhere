// QR matrisini çizime hazırlar: her koyu modül ('1') bir hücre, çevresinde
// sessiz bölge. Çizim QrCode.svelte'de modül başına bir `rect` ile yapılır;
// `{@html}` kullanılmaz, kod metni DOM'a yazılmaz.

import type { QrMatrix } from '../api/types';

/** Standart sessiz bölge: her kenarda 4 modül. */
export const QR_QUIET = 4;

export interface QrCell {
  x: number;
  y: number;
}

export interface QrDrawing {
  /** viewBox kenarı: modül sayısı + iki yanda sessiz bölge. */
  side: number;
  /** Koyu modüller, sessiz bölge kadar kaydırılmış. */
  cells: QrCell[];
}

export function qrCells(qr: QrMatrix, quiet = QR_QUIET): QrDrawing {
  const cells: QrCell[] = [];
  for (let y = 0; y < qr.size; y++) {
    const row = qr.rows[y] ?? '';
    for (let x = 0; x < qr.size; x++) {
      if (row[x] === '1') cells.push({ x: x + quiet, y: y + quiet });
    }
  }
  return { side: qr.size + quiet * 2, cells };
}
