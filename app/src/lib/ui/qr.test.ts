import { describe, expect, it } from 'vitest';
import { mockQr } from '../api/mock';
import { QR_QUIET, qrCells } from './qr';

// QR çizimi: boyut sessiz bölgeyle büyür, her '1' tam bir hücre olur, '0' hücre olmaz.

describe('qrCells', () => {
  it("viewBox kenarı modül + 2×sessiz bölge; '1' sayısı kadar hücre", () => {
    const qr = mockQr('mailhere://aktar?v=1&h=192.168.1.20&p=51234&t=abc&k=def');
    const d = qrCells(qr);
    expect(QR_QUIET).toBe(4);
    expect(d.side).toBe(qr.size + 8);
    const ones = qr.rows.reduce((n, row) => n + [...row].filter((c) => c === '1').length, 0);
    expect(d.cells).toHaveLength(ones);
    // Bulucu desenin sol üst köşesi sessiz bölge kadar kayar.
    expect(d.cells[0]).toEqual({ x: 4, y: 4 });
    // Hiçbir hücre sessiz bölgeye taşmaz.
    for (const c of d.cells) {
      expect(c.x).toBeGreaterThanOrEqual(4);
      expect(c.y).toBeGreaterThanOrEqual(4);
      expect(c.x).toBeLessThan(d.side - 4);
      expect(c.y).toBeLessThan(d.side - 4);
    }
  });

  it('sessiz bölge ayarlanabilir; boş ve eksik satırlar hücre üretmez', () => {
    const d = qrCells({ size: 3, rows: ['101', '010'] }, 1);
    expect(d.side).toBe(5);
    expect(d.cells).toEqual([
      { x: 1, y: 1 },
      { x: 3, y: 1 },
      { x: 2, y: 2 },
    ]);
    expect(qrCells({ size: 2, rows: ['00', '00'] }).cells).toEqual([]);
  });
});
