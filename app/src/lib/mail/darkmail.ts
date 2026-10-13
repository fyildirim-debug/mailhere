// Koyu temada posta: kendi zemini olmayan postanın okunmayan koyu yazı
// renklerini koyu zemine uyarlar. Çerçeve betiksizdir; uyarlamayı ana pencere
// yapar (aynı köken). Saf DOM işlemi; ağ ya da yeni içerik eklenmez.

/** En çok bu kadar öğe incelenir (dev bültenlerde donmasın). */
const MAX_ELEMENTS = 6000;

interface Rgb {
  r: number;
  g: number;
  b: number;
  a: number;
}

function parse(color: string): Rgb | null {
  const m = color.match(/rgba?\(([^)]+)\)/);
  if (!m) return null;
  const p = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
  if (p.length < 3 || p.some((x) => Number.isNaN(x))) return null;
  return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
}

function luminance({ r, g, b }: Rgb): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

export function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

function toHsl({ r, g, b }: Rgb): [number, number, number] {
  const [R, G, B] = [r / 255, g / 255, b / 255];
  const max = Math.max(R, G, B);
  const min = Math.min(R, G, B);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === R ? (G - B) / d + (G < B ? 6 : 0) : max === G ? (B - R) / d + 2 : (R - G) / d + 4;
  return [h * 60, s, l];
}

/** Görünür bir zemini var mı (saydam değil)? */
function opaque(color: string): boolean {
  const c = parse(color);
  return !!c && c.a > 0.05;
}

/**
 * Posta kendi zeminini çiziyor mu? Geniş bir öğede zemin rengi ya da görseli
 * (ya da `bgcolor`) varsa evet: bülten ve faturalar kendi tasarımında kalır.
 */
export function hasOwnBackground(doc: Document): boolean {
  const body = doc.body;
  if (!body) return false;
  const view = doc.defaultView;
  if (!view) return false;
  const wide = Math.max(200, body.clientWidth * 0.5);
  const all = body.querySelectorAll<HTMLElement>('*');
  const n = Math.min(all.length, MAX_ELEMENTS);
  for (let i = 0; i < n; i++) {
    const el = all[i];
    const cs = view.getComputedStyle(el);
    const painted = opaque(cs.backgroundColor) || (cs.backgroundImage && cs.backgroundImage !== 'none') || el.hasAttribute('bgcolor');
    if (!painted) continue;
    const r = el.getBoundingClientRect();
    if (r.width >= wide && r.height >= 40) return true;
  }
  return false;
}

/**
 * Koyu zemindeki okunmayan yazı renklerini açar. Gri tonlar uygulamanın yazı
 * rengine, renkli tonlar aynı renk tonunda açık hale gelir. Kendi küçük zemini
 * olan öğelerin (vurgulu satır, düğme) içine dokunulmaz.
 */
export function adaptDark(doc: Document, colors: { ink: string; ink2: string; paper: string }) {
  const view = doc.defaultView;
  const body = doc.body;
  if (!view || !body) return;
  const paper = parse(view.getComputedStyle(body).backgroundColor) ?? { r: 25, g: 28, b: 34, a: 1 };
  const all = body.querySelectorAll<HTMLElement>('*');
  const n = Math.min(all.length, MAX_ELEMENTS);
  const keep = new Set<Element>();
  for (let i = 0; i < n; i++) {
    const el = all[i];
    if (keep.has(el.parentElement as Element)) {
      keep.add(el);
      continue;
    }
    const cs = view.getComputedStyle(el);
    if (opaque(cs.backgroundColor) || (cs.backgroundImage && cs.backgroundImage !== 'none')) {
      keep.add(el);
      continue;
    }
    const c = parse(cs.color);
    if (!c || contrast(c, paper) >= 4.5) continue;
    const [h, s, l] = toHsl(c);
    if (s < 0.18) {
      // Gri: koyu gri ana yazıya, orta gri (imza, alıntı) ikincil yazıya.
      el.style.setProperty('color', l > 0.35 ? colors.ink2 : colors.ink, 'important');
    } else {
      el.style.setProperty('color', `hsl(${Math.round(h)} ${Math.round(Math.min(s, 0.85) * 100)}% 74%)`, 'important');
    }
  }
}
