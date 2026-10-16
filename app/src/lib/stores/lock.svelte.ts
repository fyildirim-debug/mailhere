// Parola kilidi: durum arka uçtan okunur (`lock_status`), değişimler `mh:lock`
// olayıyla gelir. Kilitliyken (çekirdek kapalı ya da yalnız arayüz kilitli)
// pencereler posta yerine kilit ekranını gösterir; hiçbir posta verisi DOM'a
// girmez. Kilit kapalıyken (varsayılan) hiçbir akışta parola sorulmaz.

import { call, EVENTS, on, toAppError, type LockStatus } from '../api';

/** Telefon arka planda en az bu kadar kaldıysa dönüşte arayüz yeniden kilitlenir. */
export const RELOCK_AFTER_MS = 60_000;

export const lock = $state({
  /** `null`: henüz sorulmadı (ekran açılmaz, posta da yüklenmez). */
  status: null as LockStatus | null,
});

let started: Promise<void> | null = null;

/**
 * Durumu bir kez okur ve olayı dinler. Çekirdek kapalıyken de çalışır
 * (`lock_*` komutları kilitliyken izinlidir). Komut yoksa (eski kabuk) kilit
 * kapalı sayılır.
 */
export function initLock(): Promise<void> {
  started ??= (async () => {
    on(EVENTS.lock, (s) => (lock.status = s));
    try {
      lock.status = await call('lock_status');
    } catch (e) {
      // Kilit komutu olmayan bir kabuk: kilit yok.
      console.warn('kilit durumu okunamadı', toAppError(e).message);
      lock.status = { enabled: false, locked: false, uiLocked: false, relockOnOpen: true, retryAfterMs: 0 };
    }
  })();
  return started;
}

/** Kilit ekranı gösterilmeli mi (çekirdek kapalı ya da arayüz kilitli). */
export function isLocked(s: LockStatus | null = lock.status): boolean {
  return !!s && (s.locked || s.uiLocked);
}

/** Posta yüklenebilir: durum biliniyor ve kilitli değil. */
export function isOpen(s: LockStatus | null = lock.status): boolean {
  return !!s && !s.locked && !s.uiLocked;
}

export async function unlock(password: string): Promise<LockStatus> {
  const s = await call('lock_unlock', { password });
  lock.status = s;
  return s;
}

/** Yalnız arayüzü kilitler; çekirdek ve bildirimler sürer. */
export async function lockUi(): Promise<void> {
  try {
    lock.status = await call('lock_ui_lock');
  } catch (e) {
    console.warn('arayüz kilitlenemedi', toAppError(e).message);
  }
}

/**
 * Telefon: uygulama arka plana gidip `RELOCK_AFTER_MS`den uzun kaldıysa
 * dönüşte arayüz kilitlenir (kilit açık ve "dönünce sor" seçiliyse). Söküm
 * işlevi döner. Masaüstünde pencere gizlenince Rust tarafı kilitler.
 */
export function watchRelock(now: () => number = Date.now): () => void {
  let hiddenAt: number | null = null;
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') {
      hiddenAt = now();
      return;
    }
    const away = hiddenAt != null ? now() - hiddenAt : 0;
    hiddenAt = null;
    const s = lock.status;
    if (s?.enabled && s.relockOnOpen && !s.locked && !s.uiLocked && away >= RELOCK_AFTER_MS) void lockUi();
  };
  document.addEventListener('visibilitychange', onVisibility);
  return () => document.removeEventListener('visibilitychange', onVisibility);
}

/** Arka uçtan gelen hata "kilitli" mi (kilit ekranı açıkken beklenen, gösterilmez). */
export function isLockedError(e: unknown): boolean {
  return toAppError(e).code === 'locked';
}
