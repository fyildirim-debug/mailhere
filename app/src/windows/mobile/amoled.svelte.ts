// AMOLED siyah: koyu temada zeminleri tam siyaha çeken, yalnız mobil kabukta
// sunulan kişisel tercih. Arka uç ayarlarına girmez; tarayıcı deposunda, yalnız
// bu telefonda tutulur (aktarımla taşınmaz). Kök öğeye `data-amoled` yazılır;
// jeton geçersiz kılmaları `tokens.css`te `:root[data-theme='dark'][data-amoled]`
// altındadır, bu yüzden açık temada öznitelik dursa da hiçbir şey değişmez.
//
// Android'de uygulama kenardan kenara çizilir ve sistem çubukları saydamdır:
// çubukların arkası uygulama çubuğunun ve alt şeridin zeminidir, onlar da
// siyaha döner. Çubuk simgeleri (`system_bars`) koyu temadaki gibi açık kalır.

const KEY = 'mailhere.mob.amoled';

export function loadAmoled(): boolean {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch {
    return false;
  }
}

export function saveAmoled(on: boolean) {
  try {
    if (on) localStorage.setItem(KEY, '1');
    else localStorage.removeItem(KEY);
  } catch {
    /* depolama yoksa tercih yalnız bu oturumda geçerli */
  }
}

function apply(on: boolean) {
  if (typeof document !== 'undefined') document.documentElement.toggleAttribute('data-amoled', on);
}

export const amoled = $state({ on: loadAmoled() });

// Modül mobil kabukla birlikte yüklenir: ilk ekran (kilit ekranı dahil) doğru zeminle gelir.
apply(amoled.on);

export function setAmoled(on: boolean) {
  amoled.on = on;
  saveAmoled(on);
  apply(on);
}
