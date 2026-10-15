// Dokunsal geri bildirim: Android kabuğunun `haptic` komutu (kaydırma eşiği,
// uzun basış onayı, ret). Masaüstü kabukta ve eski Android kabuğunda komut
// yoktur; çağrı sessizce geçer. Sonuç beklenmez: titreşim hareketi geciktirmez.

import { call, type HapticKind } from '../../lib/api';

export function haptic(kind: HapticKind) {
  try {
    void call('haptic', { kind }).catch(() => {
      /* komut yok (masaüstü, tarayıcı) ya da titreşim kapalı */
    });
  } catch {
    /* arka uç hazır değil */
  }
}
