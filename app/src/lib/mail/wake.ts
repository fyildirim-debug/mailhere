import { call } from '../api';

/** Uygulama arka planda en az bu kadar kaldıysa dönüşte bağlantılar yenilenir. */
export const AWAY_MS = 30_000;
/** Masaüstünde pencereye dönüşte en sık bu aralıkla eşitlenir. */
export const REFRESH_MS = 60_000;

/**
 * Ağ geri gelince ve (mobilde) uygulama uzun süre arka planda kaldıysa
 * eşitleme bağlantılarını baştan kurdurur: telefon arka plandayken IDLE
 * bağlantısı çoğunlukla ölür. Masaüstünde pencereye dönülünce (en çok
 * dakikada bir) tüm kutular eşitlenir: liste, gelen kutusu dışındaki
 * klasörlerde de düzenli eşitlemeyi beklemeden güncellenir. Bilgisayarın
 * uykusu Rust tarafında saat sıçramasıyla yakalanır. Söküm işlevi döner.
 */
export function watchWake(opts: { background: boolean }, now: () => number = Date.now): () => void {
  let hiddenAt: number | null = null;
  let refreshedAt = now();
  const quiet = () => {
    /* hesap durumu eşitleme olayıyla zaten gösterilir */
  };
  const reconnect = () => {
    refreshedAt = now();
    call('sync_reconnect').catch(quiet);
  };
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') {
      hiddenAt = now();
      return;
    }
    if (hiddenAt != null && now() - hiddenAt >= AWAY_MS) reconnect();
    hiddenAt = null;
  };
  const onFocus = () => {
    if (now() - refreshedAt < REFRESH_MS) return;
    refreshedAt = now();
    call('sync_now').catch(quiet);
  };
  window.addEventListener('online', reconnect);
  if (opts.background) document.addEventListener('visibilitychange', onVisibility);
  else window.addEventListener('focus', onFocus);
  return () => {
    window.removeEventListener('online', reconnect);
    window.removeEventListener('focus', onFocus);
    document.removeEventListener('visibilitychange', onVisibility);
  };
}
