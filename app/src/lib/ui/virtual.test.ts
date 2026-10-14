import { describe, expect, it } from 'vitest';
import { buildLayout, firstVisible, scrollToReveal, visibleEntries, type LayoutEntry } from './virtual';

interface R {
  id: number;
  g: string;
}

const rows: R[] = [
  { id: 1, g: 'a' },
  { id: 2, g: 'a' },
  { id: 3, g: 'b' },
  { id: 4, g: 'a' },
];

const layout = buildLayout(rows, { rowH: 70, groupH: 30, groupOf: (r) => r.g, groupLabel: (g) => g.toUpperCase(), rowKey: (r) => `m-${r.id}` });

describe('sanal liste yerleşimi', () => {
  it('ardışık satırlar aynı gruptaysa tek başlık; grup değişince konuma bağlı yeni başlık', () => {
    expect(layout.entries.map((e) => e.key)).toEqual(['g-0', 'm-1', 'm-2', 'g-2', 'm-3', 'g-3', 'm-4']);
    expect(layout.entries.filter((e) => e.kind === 'group').map((e) => e.label)).toEqual(['A', 'B', 'A']);
    expect(layout.entries.map((e) => e.top)).toEqual([0, 30, 100, 170, 200, 270, 300]);
    // 3 başlık + 4 satır + 16 px alt boşluk.
    expect(layout.total).toBe(3 * 30 + 4 * 70 + 16);
  });

  it('boş liste boş yerleşim verir', () => {
    const empty = buildLayout([] as R[], { rowH: 70, groupH: 30, tail: 0, groupOf: (r) => r.g, groupLabel: (g) => g, rowKey: (r) => `${r.id}` });
    expect(empty.entries).toEqual([]);
    expect(empty.total).toBe(0);
  });

  it('ikili arama konumu kapsayan ilk girdiyi bulur', () => {
    const e = layout.entries;
    expect(firstVisible(e, -100)).toBe(0);
    expect(firstVisible(e, 0)).toBe(0);
    expect(firstVisible(e, 31)).toBe(1);
    expect(firstVisible(e, 100)).toBe(1); // 30+70 = 100: satır 1 tam biter, alt sınır dahil
    expect(firstVisible(e, 101)).toBe(2);
    expect(firstVisible(e, 9999)).toBe(e.length - 1);
  });

  it('görünür dilim pencereyi ve taşma payını kapsar', () => {
    const keys = (list: LayoutEntry[]) => list.map((e) => e.key);
    // Tepesi pencerenin dışında kalan girdi (m-2, top 100) alınmaz.
    expect(keys(visibleEntries(layout.entries, 0, 100, 0))).toEqual(['g-0', 'm-1']);
    expect(keys(visibleEntries(layout.entries, 0, 100, 100))).toEqual(['g-0', 'm-1', 'm-2', 'g-2']);
    expect(keys(visibleEntries(layout.entries, 280, 50, 0))).toEqual(['g-3', 'm-4']);
    expect(visibleEntries([], 0, 100, 100)).toEqual([]);
  });

  it('seçili satır görünmüyorsa başlığıyla birlikte kaydırma hedefi verir', () => {
    // Satır 2 (dizin 2, top 200, h 70) ve üstündeki başlık (top 170).
    expect(scrollToReveal(layout, 2, 300, 100)).toBe(170);
    expect(scrollToReveal(layout, 2, 0, 100)).toBe(170);
    expect(scrollToReveal(layout, 2, 170, 100)).toBeNull();
    // Dizin 1'in üstünde başlık yok: kendi tepesine kaydırılır.
    expect(scrollToReveal(layout, 1, 200, 100)).toBe(100);
    expect(scrollToReveal(layout, 9, 0, 100)).toBeNull();
  });
});
