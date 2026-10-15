// Komut paleti ve seçiciler için bulanık eşleşme. Saf işlevler.
//
// Puan: tam eşleşme > baştan > sözcük başından > içinde geçer > harfler sırayla
// (araya başka harf girebilir). Türkçe büyük/küçük harf ve aksanlar katlanır
// ("cop" "Çöp"e, "arsiv" "Arşiv"e uyar).

export function fold(s: string): string {
  return s
    .toLocaleLowerCase('tr')
    .replace(/ı/g, 'i')
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '');
}

/** Tek bir metnin aranan sözcükle eşleşme puanı; eşleşmezse 0. */
export function scoreText(text: string, needle: string): number {
  const h = fold(text);
  const n = fold(needle).trim();
  if (!n) return 1;
  if (h === n) return 100;
  if (h.startsWith(n)) return 80;
  const at = h.indexOf(n);
  if (at > 0 && /[\s\-/·:(]/.test(h[at - 1])) return 60;
  if (at >= 0) return 40;
  // Her sözcüğün başından (ör. "gk" → "gelen kutusu").
  const initials = h
    .split(/[\s\-/·:]+/)
    .filter(Boolean)
    .map((w) => w[0])
    .join('');
  if (n.length > 1 && initials.startsWith(n.replace(/\s+/g, ''))) return 30;
  // Harfler sırayla; sıkı düşenler daha yüksek.
  let i = 0;
  let gaps = 0;
  let last = -1;
  for (const ch of n.replace(/\s+/g, '')) {
    const j = h.indexOf(ch, i);
    if (j < 0) return 0;
    if (last >= 0 && j > last + 1) gaps++;
    last = j;
    i = j + 1;
  }
  return Math.max(5, 20 - gaps * 3);
}

/** Etiket ve eşanlamlılardan en iyi puan; aranan birden çok sözcükse her biri bir yerde geçmeli. */
export function score(texts: string[], needle: string): number {
  const n = needle.trim();
  if (!n) return 1;
  const whole = Math.max(0, ...texts.map((x) => scoreText(x, n)));
  if (whole) return whole;
  const words = n.split(/\s+/);
  if (words.length < 2) return 0;
  let sum = 0;
  for (const w of words) {
    const s = Math.max(0, ...texts.map((x) => scoreText(x, w)));
    if (!s) return 0;
    sum += s;
  }
  return Math.round(sum / words.length / 2);
}

/** Etikette aranan sözcüğün geçtiği aralık (vurgu için); yoksa null. */
export function matchRange(label: string, needle: string): [number, number] | null {
  const n = fold(needle).trim();
  if (!n) return null;
  // Katlama uzunluğu korur (tek harfli eşlemeler); dizinler etikete denk düşer.
  const h = fold(label);
  if (h.length !== label.length) return null;
  const at = h.indexOf(n);
  return at < 0 ? null : [at, at + n.length];
}
