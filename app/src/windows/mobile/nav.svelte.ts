// Mobil gezinme yığını: liste → okuma / yazma / ayarlar / planlananlar / arama.
// Çekmece, alt sayfa ve okuma ekranında yerinde açılan yanıt alanı yığının
// üstünde "katman" olarak durur.
//
// Tarayıcı geçmişiyle bire bir: her itme ve her katman açılışı bir
// `history.pushState` girdisidir; girdinin durumu `{ mh: derinlik }` taşır.
// Geri tuşu (`popstate`) hedef derinliği getirir, yığın ona indirgenir: önce
// açık katmanlar (üstten), sonra ekranlar çıkar. Kökte (derinlik 0) geri
// tuşu bizim değildir; Android'de uygulama kapanır.
//
// Katmanlar: çekmece ve alt sayfa birbirinin yerine geçer (tek girdi). Yanıt
// alanı ekranın parçasıdır: üstüne alt sayfa açılabilir (kişi kartı, menü);
// geri tuşu önce alt sayfayı, sonra yanıt alanını kapatır.

import type { Address, ComposeMode, MessageId } from '../../lib/api';
import type { SettingsSection } from '../../lib/stores/mail.svelte';

export type MobileScreen =
  | { name: 'list' }
  | { name: 'read'; id: MessageId }
  /**
   * Yazma: `to` ve `from` yalnız yeni postada (kişi kartından, `mh:navigate`
   * rotasından) dolu gelir; `body` okuma ekranının yanıt alanından "tam ekran
   * yaz" ile taşınan metindir.
   */
  | { name: 'compose'; mode: ComposeMode; id: MessageId | null; to: Address[]; from: string | null; body?: string }
  /** Ayarlar: `section` boşsa bölüm listesi; her bölüm ayrı ekran olarak itilir. */
  | { name: 'settings'; section: SettingsSection | null }
  /** Planlanmış gönderimler (giden kutusu); çekmeceden açılır. */
  | { name: 'scheduled' }
  | { name: 'search' };

export type Layer = 'drawer' | 'sheet' | 'reply';

/** Ekranın parçası olan katmanlar: üstlerine açılan katman yerlerine geçmez. */
const INLINE: readonly Layer[] = ['reply'];

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
  const state = $state({ stack: [{ name: 'list' }] as MobileScreen[], layers: [] as Layer[] });

  /** Bizim olan geçmiş girdisi sayısı: ekranlar (kök hariç) + açık katmanlar. */
  function depth(): number {
    return state.stack.length - 1 + state.layers.length;
  }

  function mark() {
    history.replaceState({ mh: depth() } satisfies Mark, '');
  }

  mark();

  function top(): Layer | null {
    return state.layers.length ? state.layers[state.layers.length - 1] : null;
  }

  function close() {
    if (!state.layers.length) return;
    state.layers.pop();
    history.back();
  }

  /** Yığını hedef derinliğe indirir: önce katmanlar, sonra ekranlar. */
  function reconcile(target: number) {
    while (depth() > target) {
      if (state.layers.length) state.layers.pop();
      else state.stack.pop();
    }
    // İleri tuşu: çıkan ekran geri yüklenmez, girdi yeniden işaretlenir.
    if (depth() < target) mark();
  }

  return {
    get stack() {
      return state.stack;
    },
    /** En üstteki katman (yoksa null). */
    get layer(): Layer | null {
      return top();
    },
    /** Katman açık mı (üstünde başka katman olsa da). */
    has(layer: Layer): boolean {
      return state.layers.includes(layer);
    },
    get current(): MobileScreen {
      return state.stack[state.stack.length - 1];
    },
    depth,

    /**
     * Yeni ekran iter; açık katmanlar kapanır ve en alttakinin geçmiş girdisini
     * ekrana devreder (fazlası tek adımda geri alınır).
     */
    push(screen: MobileScreen) {
      const open = state.layers.length;
      if (open) {
        state.layers = [];
        state.stack.push(screen);
        if (open > 1) history.go(-(open - 1));
        else mark();
        return;
      }
      state.stack.push(screen);
      history.pushState({ mh: depth() } satisfies Mark, '');
    },

    /** Üstteki ekranı değiştirir; geçmiş girdisi aynı kalır. */
    replace(screen: MobileScreen) {
      state.stack[state.stack.length - 1] = screen;
    },

    /**
     * Katman açar. Çekmece ya da alt sayfa açıkken başka biri açılırsa yerine
     * geçer (tek girdi); yanıt alanının üstüne yeni girdiyle eklenir.
     */
    open(layer: Layer) {
      const now = top();
      if (now === layer) return;
      if (now && !INLINE.includes(now)) {
        state.layers[state.layers.length - 1] = layer;
        return;
      }
      state.layers.push(layer);
      history.pushState({ mh: depth() } satisfies Mark, '');
    },

    /** En üstteki katmanı kapatır; geçmiş girdisi geri alınır. */
    close,

    /** Yalnız en üstteki bu katmansa kapatır (ör. yanıt alanının kapat düğmesi). */
    closeLayer(layer: Layer) {
      if (top() === layer) close();
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
     * Üstteki ekrandan çıkar; açık katmanlar (alt sayfa, yanıt alanı) varsa
     * onlarla birlikte, tek geçmiş adımında. Bir alt sayfadan seçilen eylem
     * ekranı kapatırken (arşivle, taşı) ayrı ayrı `back` yerine bu kullanılır:
     * katmanlar hemen kapanır, girdiler birden geri alınır. Kökte false.
     */
    pop(): boolean {
      if (state.stack.length <= 1) return false;
      const steps = 1 + state.layers.length;
      state.layers = [];
      history.go(-steps);
      return true;
    },

    /**
     * Köke döner (ör. aktarım bitince "Gelen kutusuna git"): bizim tüm geçmiş
     * girdileri tek seferde geri alınır, yığın `popstate` ile boşalır. Kökteyken
     * bir şey yapmaz.
     */
    home() {
      const d = depth();
      if (d > 0) history.go(-d);
    },

    /** `popstate`: tarayıcının getirdiği girdiye göre yığın indirgenir. */
    onPopState(data: unknown) {
      reconcile(depthOf(data));
    },

    /**
     * Köke döner (ör. hesap kalmayınca). Bizim geçmiş girdilerimiz de geri
     * alınır; yoksa Android geri tuşu her basışta ölü bir girdiye düşerdi.
     */
    reset() {
      const d = depth();
      state.stack = [{ name: 'list' }];
      state.layers = [];
      if (d > 0) history.go(-d);
      else mark();
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
