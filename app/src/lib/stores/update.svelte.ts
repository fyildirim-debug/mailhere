// İmzalı güncelleme: arka uç denetler ve `mh:update` yayar; arayüz kurmayı önerir.
// İmzayı arka uç doğrular; burada yalnız durum tutulur.

import { call, EVENTS, on, toAppError, type UpdateInfo } from '../api';
import { settings } from './settings.svelte';

export const update = $state({
  info: null as UpdateInfo | null,
  /** Kullanıcı "Sonra" dedi; bu oturumda şerit gizli. */
  dismissed: false,
  checking: false,
  installing: false,
  /** Elle denetimin sonucu: null = denetlenmedi, true = güncel. */
  upToDate: null as boolean | null,
  error: null as string | null,
});

let started = false;

/** Ana pencerede bir kez çağrılır. */
export function initUpdate() {
  if (started) return;
  started = true;
  on(EVENTS.update, (info) => {
    update.info = info;
    update.dismissed = false;
  });
  // Açılışı yavaşlatmamak için biraz bekle; kapalıysa hiç denetleme.
  setTimeout(() => {
    if (settings.value.autoUpdate && !update.info) void checkUpdate(true);
  }, 8000);
}

export async function checkUpdate(quiet = false) {
  if (update.checking) return;
  update.checking = true;
  update.error = null;
  try {
    const info = await call('update_check');
    update.info = info;
    update.upToDate = info == null;
    if (info) update.dismissed = false;
  } catch (e) {
    if (!quiet) update.error = toAppError(e).message;
  } finally {
    update.checking = false;
  }
}

export async function installUpdate() {
  if (update.installing || !update.info) return;
  update.installing = true;
  update.error = null;
  try {
    // Başarılıysa uygulama yeniden başlar; buraya dönülmez.
    await call('update_install');
  } catch (e) {
    update.error = toAppError(e).message;
  } finally {
    update.installing = false;
  }
}
