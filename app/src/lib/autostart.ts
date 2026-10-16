// Açılışta başlama: masaüstünde oturum açılınca tepside (pencere açmadan),
// telefonda açılışta arka planda başlar. Komutlar iki kabukta aynı adla,
// kabuğa özgüdür; komut yoksa (Android kabuğunun masaüstü geliştirme
// ikilisi) durum `null` (desteklenmiyor) sayılır ve anahtar gizlenir.

import { call } from './api';

/** Açılışta başlama açık mı; komut yoksa ya da okunamazsa `null`. */
export async function autostart(): Promise<boolean | null> {
  try {
    return await call('autostart_get');
  } catch {
    return null;
  }
}

/** Açar ya da kapatır; son durumu döner (yazılamazsa hata). */
export function setAutostart(on: boolean): Promise<boolean> {
  return call('autostart_set', { on });
}
