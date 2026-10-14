// Mobil gezinme yığını: liste → okuma / yazma / ayarlar / arama. Çekmece ve alt
// sayfa gibi katmanlar yığının üstünde tek bir "katman" olarak durur.
//
// Tarayıcı geçmişiyle bire bir: her itme ve her katman açılışı bir
// `history.pushState` girdisidir; girdinin durumu `{ mh: derinlik }` taşır.
// Geri tuşu (`popstate`) hedef derinliği getirir, yığın ona indirgenir: önce
// açık katman kapanır, sonra ekranlar çıkar. Kökte (derinlik 0) geri tuşu
// bizim değildir; Android'de uygulama kapanır.

import type { Address, ComposeMode, MessageId } from '../../lib/api';
import type { SettingsSection } from '../../lib/stores/mail.svelte';

export type MobileScreen =
  | { name: 'list' }
  | { name: 'read'; id: MessageId }
  /** Yazma: `to` ve `from` yalnız yeni postada (kişi kartından, `mh:navigate` rotasından) dolu gelir. */
  | { name: 'compose'; mode: ComposeMode; id: MessageId | null; to: Address[]; from: string | null }
  | { name: 'settings'; section: SettingsSection | null }
  | { name: 'search' };

export type Layer = 'drawer' | 'sheet';

/** `window.history`nin kullanılan kısmı; testlerde sahtesi verilir. */
export interface HistoryLike {
  pushState(data: unknown, unused: string): void;
  replaceState(data: unknown, unused: string): void;
  back(): void;
  go(delta: number): void;
}

interface Mark {
  mh: number;
}

function depthOf(data: unknown): number {
  const mh = (data as Partial<Mark> | null)?.mh;
  return typeof mh === 'number' && mh >= 0 ? Math.floor(mh) : 0;
}

export function createNav(history: HistoryLike) {
  const state = $state({ stack: [{ name: 'list' }] as MobileScreen[], layer: null as Layer | null });

  /** Bizim olan geçmiş girdisi sayısı: ekranlar (kök hariç) + açık katman. */
  function depth(): number {
    return state.stack.length - 1 + (state.layer ? 1 : 0);
  }

  function mark() {
    history.replaceState({ mh: depth() } satisfies Mark, '');
  }

  mark();

  /** Yığını hedef derinliğe indirir: önce katman, sonra ekranlar. */
  function reconcile(target: number) {
    while (depth() > target) {
      if (state.layer) state.layer = null;
      else state.stack.pop();
    }
    // İleri tuşu: çıkan ekran geri yüklenmez, girdi yeniden işaretlenir.
    if (depth() < target) mark();
  }

  return {
    get stack() {
      return state.stack;
    },
    get layer() {
      return state.layer;
    },
    get current(): MobileScreen {
      return state.stack[state.stack.length - 1];
    },
    depth,

    /** Yeni ekran iter; açık katman kapanır ve geçmiş girdisini ekrana devreder. */
    push(screen: MobileScreen) {
      if (state.layer) {
        state.layer = null;
        state.stack.push(screen);
        mark();
        return;
      }
      state.stack.push(screen);
      history.pushState({ mh: depth() } satisfies Mark, '');
    },

    /** Üstteki ekranı değiştirir; geçmiş girdisi aynı kalır. */
    replace(screen: MobileScreen) {
      state.stack[state.stack.length - 1] = screen;
    },

    /** Çekmece ya da alt sayfa açar (tek katman; başka katman açıksa yerine geçer). */
    open(layer: Layer) {
      if (state.layer === layer) return;
      if (state.layer) {
        state.layer = layer;
        return;
      }
      state.layer = layer;
      history.pushState({ mh: depth() } satisfies Mark, '');
    },

    /** Katmanı kapatır; geçmiş girdisi geri alınır. */
    close() {
      if (!state.layer) return;
      state.layer = null;
      history.back();
    },

    /**
     * Geri: açık katman ya da üstteki ekran kapanır. Kökte yapacak bir şey
     * yoktur (false); Android bunu uygulamadan çıkış sayar.
     */
    back(): boolean {
      if (depth() === 0) return false;
      history.back();
      return true;
    },

    /**
     * Üstteki ekrandan çıkar; açık katman (alt sayfa) varsa onunla birlikte,
     * tek geçmiş adımında. Bir alt sayfadan seçilen eylem ekranı kapatırken
     * (arşivle, taşı) iki ayrı `back` yerine bu kullanılır: katman hemen
     * kapanır, iki girdi birden geri alınır. Kökte false.
     */
    pop(): boolean {
      if (state.stack.length <= 1) return false;
      const steps = 1 + (state.layer ? 1 : 0);
      state.layer = null;
      history.go(-steps);
      return true;
    },

    /** `popstate`: tarayıcının getirdiği girdiye göre yığın indirgenir. */
    onPopState(data: unknown) {
      reconcile(depthOf(data));
    },

    /** Köke döner (ör. hesap kalmayınca); geçmiş girdileri geride kalır ama işaretsizdir. */
    reset() {
      state.stack = [{ name: 'list' }];
      state.layer = null;
      mark();
    },
  };
}

export type Nav = ReturnType<typeof createNav>;

const browserHistory: HistoryLike =
  typeof window !== 'undefined'
    ? window.history
    : { pushState() {}, replaceState() {}, back() {}, go() {} };

/** Uygulamanın tek gezinme yığını. */
export const nav = createNav(browserHistory);
