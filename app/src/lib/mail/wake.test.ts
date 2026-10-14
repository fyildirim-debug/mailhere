import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const calls: string[] = [];
vi.mock('../api', () => ({
  call: (name: string) => {
    calls.push(name);
    return Promise.resolve(null);
  },
}));

const { AWAY_MS, watchWake } = await import('./wake');

/** Olay dinleyicilerini tutan en küçük hedef. */
function target() {
  const handlers = new Map<string, Set<() => void>>();
  return {
    visibilityState: 'visible' as 'visible' | 'hidden',
    addEventListener(type: string, fn: () => void) {
      if (!handlers.has(type)) handlers.set(type, new Set());
      handlers.get(type)!.add(fn);
    },
    removeEventListener(type: string, fn: () => void) {
      handlers.get(type)?.delete(fn);
    },
    fire(type: string) {
      for (const fn of handlers.get(type) ?? []) fn();
    },
    count(type: string) {
      return handlers.get(type)?.size ?? 0;
    },
  };
}

describe('arka plandan dönüş', () => {
  let win: ReturnType<typeof target>;
  let doc: ReturnType<typeof target>;
  let clock = 0;
  const now = () => clock;

  beforeEach(() => {
    calls.length = 0;
    clock = 1_000_000;
    win = target();
    doc = target();
    vi.stubGlobal('window', win);
    vi.stubGlobal('document', doc);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function hide() {
    doc.visibilityState = 'hidden';
    doc.fire('visibilitychange');
  }

  function show() {
    doc.visibilityState = 'visible';
    doc.fire('visibilitychange');
  }

  it('uzun arka plandan dönünce bağlantılar yenilenir, kısa ayrılışta yenilenmez', () => {
    watchWake({ background: true }, now);
    hide();
    clock += AWAY_MS - 1;
    show();
    expect(calls).toEqual([]);
    hide();
    clock += AWAY_MS;
    show();
    expect(calls).toEqual(['sync_reconnect']);
    // Gizlenmeden gelen görünürlük olayı yenilemez.
    show();
    expect(calls).toEqual(['sync_reconnect']);
  });

  it('ağ geri gelince yenilenir; masaüstünde görünürlük izlenmez', () => {
    const off = watchWake({ background: false }, now);
    expect(doc.count('visibilitychange')).toBe(0);
    win.fire('online');
    expect(calls).toEqual(['sync_reconnect']);
    off();
    expect(win.count('online')).toBe(0);
  });
});
