// Gelen kutusu kategori çiplerinin okunmamış sayıları. Sayaçlar (`counts`)
// kategori ayrımı taşımaz; mevcut liste komutu okunmamış süzgeciyle her
// kategori için yerel veritabanından sorulur (ağ yok). Sayı satır başınadır
// (konuşma bir satır), büyük başlığın altındaki sayı gibi; 100'de durur.

import { call, type AccountId, type Category } from '../../lib/api';
import { rowsOf } from '../../lib/stores/mail.svelte';

const KEYS: (Category | null)[] = [null, 'people', 'notifications', 'newsletters'];
/** Bu kadarından sonrası "99+" gösterilir; daha çok sayfa okunmaz. */
const LIMIT = 100;

function keyOf(c: Category | null): string {
  return c ?? 'all';
}

function createCategoryUnread() {
  const state = $state({ n: {} as Record<string, number> });
  let token = 0;

  return {
    /** Kategorinin okunmamış satır sayısı (`null` tümü); bilinmiyorsa 0. */
    of(c: Category | null): number {
      return state.n[keyOf(c)] ?? 0;
    },

    /** Dört sayı birlikte yenilenir; eski istek yenisini ezmez. */
    async refresh(accountId: AccountId | null) {
      const mine = ++token;
      try {
        const pages = await Promise.all(
          KEYS.map((category) =>
            call('messages_list', { query: { view: { kind: 'inbox' }, accountId, category, only: 'unread', cursor: null, limit: LIMIT } }),
          ),
        );
        if (mine !== token) return;
        const next: Record<string, number> = {};
        KEYS.forEach((c, i) => {
          const rows = rowsOf(pages[i].items).filter((r) => r.unread).length;
          next[keyOf(c)] = pages[i].nextCursor ? Math.max(rows, LIMIT) : rows;
        });
        state.n = next;
      } catch {
        /* sayı alınamazsa eski değer kalır; çip sayısız da çalışır */
      }
    },
  };
}

export const categoryUnread = createCategoryUnread();
