// Ana pencerenin yerleşimi: pencere genişliğine göre üç kip, sütun genişlikleri
// ve dar penceredeki katman / yığın durumu. Genişlikler kişisel tercihtir;
// yalnız bu bilgisayarda, tarayıcı deposunda tutulur.
//
//   geniş (≥ 1120)  kenar çubuğu · liste · okuma (kenar çubuğu Ctrl+B ile raya iner)
//   ray (860–1120)  56 px ikon rayı · liste · okuma; Ctrl+B tam kenar çubuğunu katman açar
//   yığın (< 860)   ray · liste YA DA okuma (posta açılınca okuma tam genişlik, Esc geri)

export type DeskMode = 'wide' | 'rail' | 'stack';

export const BREAK_WIDE = 1120;
export const BREAK_RAIL = 860;
export const RAIL_W = 56;
/** Okuma alanının liste genişletilirken altına inmeyeceği genişlik. */
export const READER_MIN = 440;

export interface Range {
  def: number;
  min: number;
  max: number;
}

export const SIDE: Range = { def: 248, min: 200, max: 360 };
export const LIST: Range = { def: 404, min: 320, max: 640 };
/** Klavyeyle ayırıcı adımı. */
export const STEP = 8;

const KEYS = { side: 'mh.desk.side', list: 'mh.desk.list', collapsed: 'mh.desk.collapsed' } as const;

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string | null) {
  try {
    if (value == null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* depo yoksa yalnız bu oturumda geçerli */
  }
}

export function clamp(v: number, r: Pick<Range, 'min' | 'max'>): number {
  return Math.round(Math.min(Math.max(v, r.min), Math.max(r.min, r.max)));
}

function stored(key: string, r: Range): number {
  const v = Number(read(key));
  return Number.isFinite(v) && v > 0 ? clamp(v, r) : r.def;
}

export const desk = $state({
  /** Etkin pencere genişliği (CSS px; tarayıcı ölçeğinde ölçek düşülür). */
  width: typeof window !== 'undefined' ? window.innerWidth : 1440,
  sideW: stored(KEYS.side, SIDE),
  listW: stored(KEYS.list, LIST),
  /** Geniş pencerede kullanıcı kenar çubuğunu raya indirdi mi (Ctrl+B). */
  collapsed: read(KEYS.collapsed) === '1',
  /** Ray kipinde tam kenar çubuğu katman olarak açık mı. */
  overlay: false,
  /** Yığın kipinde okuma alanı açık mı (liste yerine). */
  reading: false,
  /** Komut paleti bu metinle açılsın (ör. "Aramayı düzenle"); palet okuyunca sıfırlar. */
  paletteSeed: null as string | null,
});

export function modeFor(width: number): DeskMode {
  if (width >= BREAK_WIDE) return 'wide';
  if (width >= BREAK_RAIL) return 'rail';
  return 'stack';
}

export function deskMode(): DeskMode {
  return modeFor(desk.width);
}

/** Tam kenar çubuğu yerinde mi (yoksa ray). */
export function sideFull(): boolean {
  return deskMode() === 'wide' && !desk.collapsed;
}

/** Listenin çizilen genişliği: okuma alanına en az `READER_MIN` kalır. */
export function listWidthFor(width: number, side: number, list: number): number {
  return clamp(list, { min: LIST.min, max: Math.min(LIST.max, width - side - READER_MIN) });
}

export function listWidth(): number {
  return listWidthFor(desk.width, sideFull() ? desk.sideW : RAIL_W, desk.listW);
}

export function setSideWidth(v: number, persist = true) {
  desk.sideW = clamp(v, SIDE);
  if (persist) write(KEYS.side, String(desk.sideW));
}

export function setListWidth(v: number, persist = true) {
  // Çizilen sınırla kaydedilir: dar pencerede bırakılan genişlik geniş pencerede de anlamlı kalsın.
  desk.listW = clamp(v, LIST);
  if (persist) write(KEYS.list, String(desk.listW));
}

/** Ctrl+B: geniş pencerede kenar çubuğunu raya indirir / açar; dar pencerede katmanı açar / kapatır. */
export function toggleSidebar() {
  if (deskMode() === 'wide') {
    desk.collapsed = !desk.collapsed;
    desk.overlay = false;
    write(KEYS.collapsed, desk.collapsed ? '1' : null);
  } else {
    desk.overlay = !desk.overlay;
  }
}

/** Yığın kipinde okuma alanına geçer (posta açıldı). */
export function showReader() {
  desk.reading = true;
}

/** Yığın kipinde listeye döner. */
export function showList() {
  desk.reading = false;
}

/** Pencere genişliğini izler; `scale` tarayıcıdaki kök ölçektir (Tauri'de 1). */
export function watchWidth(scale: () => number): () => void {
  const update = () => {
    desk.width = window.innerWidth / (scale() || 1);
    // Geniş kipe dönünce katman anlamsızlaşır.
    if (deskMode() === 'wide' && desk.overlay) desk.overlay = false;
  };
  update();
  window.addEventListener('resize', update);
  return () => window.removeEventListener('resize', update);
}
