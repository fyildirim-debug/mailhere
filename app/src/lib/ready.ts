// İlk ekran hazır sinyali. Android kabuğu WebView'e yalnız uygulamanın kendi
// adresine açık `mhAcilis` kanalını ekler ve sistem açılış ekranını bu sinyale
// dek tutar (büyük ikon → boş zemin → arayüz sıçraması olmaz). Kanal yoksa
// (masaüstü, tarayıcı, testler) hiçbir şey yapmaz. Yalnız bir kez gider.

interface ReadyChannel {
  postMessage(message: string): void;
}

let sent = false;

/** İlk anlamlı ekran DOM'da: kilit ekranı, ilk açılış ya da posta listesi. */
export function signalFirstScreen(): void {
  if (sent) return;
  sent = true;
  const channel = (globalThis as { mhAcilis?: ReadyChannel }).mhAcilis;
  // Svelte DOM'u eşzamanlı yazar; sinyal görevin sonunda gider ki ilk karede
  // ekran tam olsun. requestAnimationFrame kullanılmaz: açılış ekranı dururken
  // WebView kare çizmez, rAF hiç gelmeyebilir.
  if (channel) setTimeout(() => channel.postMessage('hazir'), 0);
}

/** Yalnız testler için. */
export function resetFirstScreenForTest(): void {
  sent = false;
}
