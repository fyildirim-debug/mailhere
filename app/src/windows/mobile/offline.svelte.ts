// Bağlantı durumu (liste başlığının altındaki satır): tarayıcı çevrimdışıysa
// (`navigator.onLine`) ya da tüm hesaplar eşitleyiciden "çevrimdışı" durumu
// bildirdiyse çevrimdışı sayılır. Çevrimdışıyken giden kutusunda zamanı
// gelmiş ama gidemeyen gönderimler sayılır ("N gönderim bekliyor"); sayı
// mevcut `outbox_list` komutundan, açılışta, giden kutusu olayında ve
// çevrimdışıyken 5 sn'de bir okunur.

import { call, EVENTS, on } from '../../lib/api';
import { mail } from '../../lib/stores/mail.svelte';

const POLL = 5000;

function createConnection() {
  const state = $state({ browserOffline: false, pending: 0, reduced: false });

  /** Hesapların hepsi çevrimdışı bildirdi mi (hesap yoksa hayır). */
  function syncOffline(): boolean {
    const states = Object.values(mail.sync).filter((s) => mail.accounts.some((a) => a.id === s.accountId));
    return states.length > 0 && states.every((s) => s.state === 'offline');
  }

  function offline(): boolean {
    return state.browserOffline || syncOffline();
  }

  let timer: ReturnType<typeof setInterval> | null = null;

  async function count() {
    try {
      const now = Date.now();
      state.pending = (await call('outbox_list')).filter((x) => x.sendAt <= now).length;
    } catch {
      /* sayı alınamazsa satır yalnız durumu söyler */
    }
  }

  return {
    get offline(): boolean {
      return offline();
    },
    /** Giden kutusunda zamanı gelmiş, bekleyen gönderim sayısı (yalnız çevrimdışıyken anlamlı). */
    get pending(): number {
      return state.pending;
    },
    /** Azaltılmış hareket: satır sönmeden kalkar. */
    get reduced(): boolean {
      return state.reduced;
    },

    /** Dinlemeyi başlatır; dönen işlev bırakır. */
    watch(): () => void {
      if (typeof window === 'undefined') return () => {};
      const media = window.matchMedia?.('(prefers-reduced-motion: reduce)');
      state.reduced = media?.matches ?? false;
      const update = () => {
        state.browserOffline = navigator.onLine === false;
        if (timer) clearInterval(timer);
        timer = null;
        if (offline()) {
          void count();
          timer = setInterval(() => void count(), POLL);
        } else state.pending = 0;
      };
      update();
      window.addEventListener('online', update);
      window.addEventListener('offline', update);
      const offOutbox = on(EVENTS.outbox, () => {
        if (offline()) void count();
      });
      const offSync = on(EVENTS.sync, () => queueMicrotask(update));
      return () => {
        window.removeEventListener('online', update);
        window.removeEventListener('offline', update);
        offOutbox();
        offSync();
        if (timer) clearInterval(timer);
        timer = null;
      };
    },
  };
}

export const connection = createConnection();
