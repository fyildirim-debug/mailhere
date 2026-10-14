// Android 13+ bildirim izni. Masaüstünde komutlar yoktur; durum "unsupported"
// sayılır ve hiçbir şey sorulmaz.

import { call } from './api';

export type NotifyPermission = 'granted' | 'denied' | 'prompt' | 'promptWithRationale' | 'unsupported';

const ASKED = 'mailhere.notifyAsked';

/** İzin durumu; sormadan okunur. */
export async function notifyPermission(): Promise<NotifyPermission> {
  try {
    return (await call('notification_status')) as NotifyPermission;
  } catch {
    return 'unsupported';
  }
}

/** Sistem iznini ister (karar daha önce kesinleştiyse Android sormaz). */
export async function requestNotifyPermission(): Promise<NotifyPermission> {
  try {
    return (await call('notification_permission')) as NotifyPermission;
  } catch {
    return notifyPermission();
  }
}

function asked(): boolean {
  try {
    return localStorage.getItem(ASKED) === '1';
  } catch {
    return false;
  }
}

function markAsked() {
  try {
    localStorage.setItem(ASKED, '1');
  } catch {
    /* depolama kapalıysa her açılışta denetlenir; Android yine yalnız kararsızken sorar */
  }
}

/**
 * İlk hesap varken izni bir kez ister: kullanıcı hesabı elle ekledi ya da
 * bilgisayardan aldı fark etmez. Sonra yalnız Ayarlar › Bildirimler'den sorulur;
 * her açılışta sormak rahatsız eder ve Android ikinci retten sonra kalıcı reddeder.
 */
let asking: Promise<void> | null = null;

export function askNotificationsOnce(): Promise<void> {
  // Aynı anda gelen çağrılar tek isteğe bağlanır; iki sistem penceresi açılmaz.
  asking ??= (async () => {
    if (asked()) return;
    const state = await notifyPermission();
    if (state === 'unsupported') return;
    markAsked();
    if (state === 'prompt' || state === 'promptWithRationale') await requestNotifyPermission();
  })();
  return asking;
}
