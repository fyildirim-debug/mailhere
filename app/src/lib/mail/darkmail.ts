// Koyu temada posta: açık zeminleri koyuya, koyu yazıları açığa eşler.
// Çerçeve betiksizdir; eşlemeyi ana pencere yapar (aynı köken). Saf DOM
// işlemi; ağ ya da yeni içerik eklenmez.
//
// Yöntem renk eşlemedir, `filter: invert()` değil: ton korunur, görsel
// süzgeçten geçmez, zaten koyu tasarlanmış alanlar (başlık bandı, düğme) ve
// içlerindeki açık yazı olduğu gibi kalır, yazı uygulamanın mürekkep
// jetonlarına oturur. İki aşama: önce açık varsayılanlarla hesaplanmış renkler
// okunur (yazma yok, yerleşim bozulmaz), sonra tek seferde yazılır. Yazılan
// her şey geri alınabilir ("Özgün renkler").

/** En çok bu kadar öğe incelenir (dev bültenlerde donmasın). */
export const MAX_ELEMENTS = 6000;

/** Bu parlaklığın üstündeki zemin "açık" sayılır ve koyulaştırılır. */
const LIGHT_BG = 0.4;
/** Bu karşıtlığın altındaki yazı zeminle gizlenmiştir (ön başlık hilesi); gizli kalır. */
const HIDDEN = 1.4;
/** Bu doygunluğun altı gri sayılır ve mürekkep jetonlarına oturur. */
const GRAY = 0.18;

export interface DarkTokens {
  ink: string;
  ink2: string;
  paper: string;
  line: string;
  lineStrong: string;
  /** Açık varsayılanlardaki bağlantı rengi; bu renkteki bağlantıya dokunulmaz, çerçeve stili koyu jetonu verir. */
  lightLink: string;
}

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

function parseAny(color: string): Rgb | null {
  const m = color.trim().match(/^#([0-9a-f]{6})$/i);
  if (m) {
    const n = parseInt(m[1], 16);
    return { r: n >> 16, g: (n >> 8) & 255, b: n & 255, a: 1 };
  }
  return parse(color);
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

function hsl(h: number, s: number, l: number): string {
  return `hsl(${Math.round(h)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%)`;
}

function same(a: Rgb | null, b: Rgb | null): boolean {
  return !!a && !!b && a.r === b.r && a.g === b.g && a.b === b.b && Math.abs(a.a - b.a) < 0.01;
}

/** Görünür bir zemini var mı (saydam değil)? */
function opaque(c: Rgb | null): c is Rgb {
  return !!c && c.a > 0.05;
}

function isLight(c: Rgb): boolean {
  return luminance(c) >= LIGHT_BG;
}

/**
 * Açık zemini koyuya eşler. Beyaz tam olarak uygulamanın kağıdına oturur;
 * açık griler kağıttan biraz açık, orta tonlar daha açık kalır (hiyerarşi
 * korunur). Ton korunur; pastel zeminlerin doygunluğu kısılır. Koyu ve orta
 * zeminler (başlık bandı, düğme) olduğu gibi döner.
 */
function mapBackground(c: Rgb, paper: Rgb): string | null {
  if (!isLight(c)) return null;
  const t = Math.min(1, (luminance(c) - LIGHT_BG) / (1 - LIGHT_BG));
  const [h, s] = toHsl(c);
  const [ph, ps, pl] = toHsl(paper);
  const light = pl + (1 - t) * (0.3 - pl);
  return s < 0.1 ? hsl(ph, ps, light) : hsl(h, s * (1 - 0.5 * t), light);
}

/**
 * Açık zemindeki yazıyı koyu zemine eşler. Griler mürekkep jetonlarına oturur;
 * renkli yazı aynı tonda açık hale gelir.
 */
function mapText(c: Rgb, paper: Rgb, t: DarkTokens): string {
  const [h, s, l] = toHsl(c);
  if (s < GRAY) {
    if (l <= 0.3) return t.ink;
    if (l <= 0.62) return t.ink2;
    const [ph, ps] = toHsl(paper);
    return hsl(ph, ps, 0.55);
  }
  return hsl(h, Math.min(s, 0.85), 0.74);
}

/** Açık zemindeki kenar çizgisini koyu zemine eşler. */
function mapBorder(c: Rgb, t: DarkTokens): string {
  const [h, s] = toHsl(c);
  if (s < GRAY) return isLight(c) ? t.line : t.lineStrong;
  return hsl(h, Math.min(s, 0.7), 0.45);
}

const SIDES = ['top', 'right', 'bottom', 'left'] as const;

interface Rec {
  el: HTMLElement;
  /** Öğenin kendi (saydam olmayan) zemini. */
  bg: Rgb | null;
  color: Rgb | null;
  /** Yazı rengi ana öğeden miras: ana öğe yazılırsa buraya da iner. */
  inherits: boolean;
  /** Rengi çerçevenin kendi stilinden gelen bağlantı; dokunulmaz. */
  defaultLink: boolean;
  /** Öğenin özgün etkin zemini: en yakın boyalı ata ya da kağıt (kendi zemini hariç). */
  effective: Rgb;
  borders: [string, Rgb][];
}

interface Write {
  el: HTMLElement;
  prop: string;
  value: string;
}

/**
 * Belgeyi koyu temaya eşler; yazılanları geri alan işlev döner. Belge açık
 * varsayılanlarla (beyaz kağıt, koyu mürekkep) yerleşmiş olmalıdır; okuma
 * ondan yapılır, sonra çağıran koyu sınıfı ekler.
 */
export function adaptDark(doc: Document, tokens: DarkTokens): () => void {
  const view = doc.defaultView;
  const body = doc.body;
  if (!view || !body) return () => {};
  const paper = parseAny(tokens.paper) ?? { r: 25, g: 28, b: 34, a: 1 };
  const lightLink = parseAny(tokens.lightLink);
  const bodyStyle = view.getComputedStyle(body);
  const bodyBg = parse(bodyStyle.backgroundColor);
  const canvas: Rgb = opaque(bodyBg) ? bodyBg : { r: 255, g: 255, b: 255, a: 1 };
  const bodyColor = parse(bodyStyle.color);

  // 1. aşama: yalnız okuma.
  const all = body.querySelectorAll<HTMLElement>('*');
  const n = Math.min(all.length, MAX_ELEMENTS);
  const recs = new Map<Element, Rec>();
  const list: Rec[] = [];
  for (let i = 0; i < n; i++) {
    const el = all[i];
    const cs = view.getComputedStyle(el);
    const bgRaw = parse(cs.backgroundColor);
    const bg = opaque(bgRaw) ? bgRaw : null;
    const parent = el.parentElement ? recs.get(el.parentElement) : undefined;
    const parentColor = parent ? parent.color : bodyColor;
    const parentEffective = parent ? (parent.bg ?? parent.effective) : canvas;
    const color = parse(cs.color);
    // Kendi rengi (satır içi stil ya da `color` özniteliği) olan öğeye ana
    // öğenin yazılan rengi inmez; o ayrı yazılır.
    const ownColor = el.style.color !== '' || el.hasAttribute('color');
    const borders: [string, Rgb][] = [];
    if (cs.borderStyle !== 'none') {
      for (const side of SIDES) {
        const style = cs.getPropertyValue(`border-${side}-style`);
        if (style === 'none' || style === 'hidden') continue;
        if (parseFloat(cs.getPropertyValue(`border-${side}-width`)) <= 0) continue;
        const bc = parse(cs.getPropertyValue(`border-${side}-color`));
        if (opaque(bc)) borders.push([side, bc]);
      }
    }
    const rec: Rec = {
      el,
      bg,
      color,
      inherits: !ownColor && !bg && same(color, parentColor),
      // Rengi çerçeve stilinden gelen bağlantı: koyu sınıf ona `--accent-text` verir.
      defaultLink: el.tagName === 'A' && !ownColor && same(color, lightLink),
      effective: parentEffective,
      borders,
    };
    recs.set(el, rec);
    list.push(rec);
  }

  // 2. aşama: yazma. Her öğe için zemin, yazı ve kenar.
  const writes: Write[] = [];
  const mappedBg = new Map<Element, string>();
  const canvasMapped = mapBackground(canvas, paper) ?? tokens.paper;
  const put = (el: HTMLElement, prop: string, value: string) => writes.push({ el, prop, value });
  for (const rec of list) {
    const { el, bg, color } = rec;
    if (bg) {
      const m = mapBackground(bg, paper);
      if (m) {
        mappedBg.set(el, m);
        put(el, 'background-color', m);
      }
    }
    // Yazı ve kenar yalnız özgün zemini açık olan bölgede değişir.
    const region = bg ?? rec.effective;
    if (!isLight(region)) continue;
    if (color && !rec.inherits && !rec.defaultLink) {
      if (contrast(color, region) < HIDDEN) {
        // Zeminle gizlenmiş yazı (ön başlık hilesi) koyuda da gizli kalsın.
        const host = bg ? el : nearestPainted(el, recs);
        put(el, 'color', (host && mappedBg.get(host)) ?? canvasMapped);
      } else {
        put(el, 'color', mapText(color, paper, tokens));
      }
    }
    for (const [side, bc] of rec.borders) put(el, `border-${side}-color`, mapBorder(bc, tokens));
  }

  const undo: { el: HTMLElement; prop: string; value: string; priority: string }[] = [];
  for (const w of writes) {
    undo.push({ el: w.el, prop: w.prop, value: w.el.style.getPropertyValue(w.prop), priority: w.el.style.getPropertyPriority(w.prop) });
    w.el.style.setProperty(w.prop, w.value, 'important');
  }
  return () => {
    for (let i = undo.length - 1; i >= 0; i--) {
      const u = undo[i];
      if (u.value) u.el.style.setProperty(u.prop, u.value, u.priority);
      else u.el.style.removeProperty(u.prop);
    }
  };
}

/** Öğenin zemini çizen en yakın atası (incelenenler içinde). */
function nearestPainted(el: HTMLElement, recs: Map<Element, Rec>): HTMLElement | null {
  let p = el.parentElement;
  while (p) {
    const r = recs.get(p);
    if (!r) return null;
    if (r.bg) return r.el;
    p = p.parentElement;
  }
  return null;
}
