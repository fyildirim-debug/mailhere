import type { Locator } from '@playwright/test';

// Karşıtlık ve tık hedefi ölçümü (e2e yardımcıları): hesaplanmış renklerden
// WCAG oranı. Zemin, öğeden köke doğru atalardaki arka plan renkleri
// saydamlıklarıyla üst üste harmanlanarak bulunur; yarı saydam metin rengi de
// bu zeminin üstüne harmanlanır.

/** Öğenin metin rengi ile arkasındaki zemin arasındaki WCAG karşıtlık oranı. */
export function contrast(loc: Locator): Promise<number> {
  return loc.evaluate((el) => {
    type Rgba = [number, number, number, number];
    /** `rgb()`, `rgba()` ve `color(srgb …)` biçimlerini 0–255 kanal + alfa olarak okur. */
    const parse = (css: string): Rgba => {
      const n = (css.match(/-?[\d.]+(e-?\d+)?%?/g) ?? []).map((v) => (v.endsWith('%') ? parseFloat(v) / 100 : parseFloat(v)));
      if (css.startsWith('color(srgb')) return [n[0] * 255, n[1] * 255, n[2] * 255, n[3] ?? 1];
      if (css === 'transparent') return [0, 0, 0, 0];
      return [n[0], n[1], n[2], n[3] ?? 1];
    };
    const over = (top: Rgba, under: Rgba): Rgba => {
      const a = top[3];
      return [top[0] * a + under[0] * (1 - a), top[1] * a + under[1] * (1 - a), top[2] * a + under[2] * (1 - a), 1];
    };
    const layers: Rgba[] = [];
    for (let n: Element | null = el; n; n = n.parentElement) {
      const bg = parse(getComputedStyle(n).backgroundColor);
      if (bg[3] > 0) layers.push(bg);
      if (bg[3] >= 1) break;
    }
    let ground: Rgba = [255, 255, 255, 1];
    for (let i = layers.length - 1; i >= 0; i--) ground = over(layers[i], ground);
    const ink = over(parse(getComputedStyle(el).color), ground);
    const lum = (c: Rgba) =>
      c
        .slice(0, 3)
        .map((v) => {
          const s = v / 255;
          return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
        })
        .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
    const [hi, lo] = [lum(ink), lum(ground)].sort((a, b) => b - a);
    return (hi + 0.05) / (lo + 0.05);
  });
}

/**
 * Öğenin kutusunun `dy` px üstünden (negatifse altından) tıklanınca aynı öğe mi
 * vurulur: görünmez tık kenarını (`::before`) doğrular.
 */
export function hitsAbove(loc: Locator, dy: number): Promise<boolean> {
  return loc.evaluate((el, d) => {
    const r = el.getBoundingClientRect();
    const y = d > 0 ? r.top - d : r.bottom - d;
    const hit = document.elementFromPoint(r.left + r.width / 2, y);
    return hit != null && (hit === el || el.contains(hit));
  }, dy);
}
