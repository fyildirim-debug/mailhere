import { call } from '../api';

/** Uygulama arka planda en az bu kadar kaldıysa dönüşte bağlantılar yenilenir. */
export const AWAY_MS = 30_000;

/**
 * Ağ geri gelince ve (mobilde) uygulama uzun süre arka planda kaldıysa
 * eşitleme bağlantılarını baştan kurdurur: telefon arka plandayken IDLE
 * bağlantısı çoğunlukla ölür. Bilgisayarın uykusu Rust tarafında saat
 * sıçramasıyla yakalanır. Söküm işlevi döner.
 */
export function watchWake(opts: { background: boolean }, now: () => number = Date.now): () => void {
  let hiddenAt: number | null = null;
  const reconnect = () => {
    call('sync_reconnect').catch(() => {
      /* hesap durumu eşitleme olayıyla zaten gösterilir */
    });
  };
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') {
      hiddenAt = now();
      return;
    }
    if (hiddenAt != null && now() - hiddenAt >= AWAY_MS) reconnect();
    hiddenAt = null;
  };
  window.addEventListener('online', reconnect);
  if (opts.background) document.addEventListener('visibilitychange', onVisibility);
  return () => {
    window.removeEventListener('online', reconnect);
    document.removeEventListener('visibilitychange', onVisibility);
  };
}
