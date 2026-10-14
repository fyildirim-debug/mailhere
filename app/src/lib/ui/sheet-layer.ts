// Alt sayfanın (Sheet.svelte) gezinme bağlantısı.
//
// Mobil kabuk açılışta kendi yığınını bağlar (MobileApp → nav.open('sheet') /
// nav.close()): böylece her alt sayfa bir geçmiş girdisi olur ve Android geri
// tuşu önce onu kapatır. Bağlı değilse (masaüstü, testler) alt sayfa geçmişe
// dokunmaz. Aynı anda açık alt sayfalar (biri kapanırken öbürü açılıyor) tek
// katman girdisini paylaşır; sayaç sıfıra inince girdi geri alınır.

export interface SheetLayer {
  /** Katman açık mı; geri tuşu kapatınca false olur (okuma reaktiftir). */
  readonly active: boolean;
  show(): void;
  hide(): void;
}

let layer: SheetLayer | null = null;
let open = 0;

export function bindSheetLayer(l: SheetLayer | null) {
  layer = l;
  open = 0;
}

/** Katman bağlı mı ve açık mı (alt sayfa, geri tuşuyla kapanınca bunu görür). */
export function sheetLayerActive(): boolean {
  return layer ? layer.active : true;
}

/** Alt sayfa açıldı: ilk açılan katman girdisini üretir. */
export function sheetOpened() {
  open++;
  if (layer && !layer.active) layer.show();
}

/**
 * Alt sayfa kapandı: aynı anda başka alt sayfa açık kalmadıysa katman girdisi
 * geri alınır. Karar bir mikro görev sonra verilir; aynı akışta kapanan ve
 * açılan alt sayfalar (menüden alt menüye) girdiyi devralır.
 */
export function sheetClosed() {
  open = Math.max(0, open - 1);
  queueMicrotask(() => {
    if (open === 0 && layer?.active) layer.hide();
  });
}
