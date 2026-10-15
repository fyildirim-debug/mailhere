// Android bildirim sağlığı: pil kısıtlaması ve isteğe bağlı "Anlık bildirim"
// (sürekli bağlantı). Masaüstünde komutlar yoktur; durum `null` (desteklenmiyor)
// sayılır ve Ayarlar › Bildirimler'de bu satırlar gizlenir.

import { call, type CommandResult } from './api';

export type BatteryStatus = CommandResult<'battery_status'>;

/** Pil kısıtlaması; komut yoksa ya da okunamazsa `null`. */
export async function batteryStatus(): Promise<BatteryStatus | null> {
  try {
    return await call('battery_status');
  } catch {
    return null;
  }
}

/**
 * Kısıtlamayı kaldırma sayfasını açar (Android'in onay penceresi ya da
 * uygulamanın pil ayarı); kullanıcı dönünce son durumu döner.
 */
export async function liftBatteryRestriction(): Promise<BatteryStatus | null> {
  await call('battery_settings');
  return batteryStatus();
}

/** Anlık bildirim açık mı; komut yoksa `null`. */
export async function backgroundMode(): Promise<boolean | null> {
  try {
    return await call('background_mode_get');
  } catch {
    return null;
  }
}

/** Anlık bildirimi açar ya da kapatır; son durumu döner (başlatılamazsa hata). */
export function setBackgroundMode(on: boolean): Promise<boolean> {
  return call('background_mode_set', { on });
}
