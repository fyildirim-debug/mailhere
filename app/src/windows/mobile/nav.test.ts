import { beforeEach, describe, expect, it } from 'vitest';
import { createNav, type HistoryLike, type Nav } from './nav.svelte';

/** Sahte tarayıcı geçmişi: girdiler ve imleç; `back` popstate'i senkron verir. */
function fakeHistory() {
  const entries: unknown[] = [null];
  let index = 0;
  let listener: ((data: unknown) => void) | null = null;
  const h: HistoryLike & { entries: unknown[]; index: number; bind(fn: (data: unknown) => void): void; forward(): void } = {
    entries,
    index,
    bind(fn) {
      listener = fn;
    },
    pushState(data) {
      entries.splice(index + 1);
      entries.push(data);
      index++;
      h.index = index;
    },
    replaceState(data) {
      entries[index] = data;
    },
    back() {
      h.go(-1);
    },
    go(delta) {
      const target = index + delta;
      if (target < 0 || target > entries.length - 1) return;
      index = target;
      h.index = index;
      listener?.(entries[index]);
    },
    forward() {
      if (index >= entries.length - 1) return;
      index++;
      h.index = index;
      listener?.(entries[index]);
    },
  };
  return h;
}

describe('mobil gezinme yığını', () => {
  let h: ReturnType<typeof fakeHistory>;
  let nav: Nav;

  beforeEach(() => {
    h = fakeHistory();
    nav = createNav(h);
    h.bind((d) => nav.onPopState(d));
  });

  it('kökte derinlik 0 olarak işaretlenir', () => {
    expect(nav.current).toEqual({ name: 'list' });
    expect(nav.depth()).toBe(0);
    expect(h.entries).toEqual([{ mh: 0 }]);
  });

  it('her itme bir geçmiş girdisi, geri tuşu ekranı çıkarır', () => {
    nav.push({ name: 'read', id: 7 });
    nav.push({ name: 'search' });
    expect(nav.stack.map((s) => s.name)).toEqual(['list', 'read', 'search']);
    expect(h.entries).toEqual([{ mh: 0 }, { mh: 1 }, { mh: 2 }]);
    expect(nav.back()).toBe(true);
    expect(nav.current).toEqual({ name: 'read', id: 7 });
    expect(nav.back()).toBe(true);
    expect(nav.current).toEqual({ name: 'list' });
    // Kökte geri bize ait değil.
    expect(nav.back()).toBe(false);
    expect(h.index).toBe(0);
  });

  it('açık çekmece geri tuşunda ekrandan önce kapanır', () => {
    nav.push({ name: 'read', id: 1 });
    nav.open('drawer');
    expect(nav.layer).toBe('drawer');
    expect(nav.depth()).toBe(2);
    nav.back();
    expect(nav.layer).toBeNull();
    expect(nav.current).toEqual({ name: 'read', id: 1 });
    nav.back();
    expect(nav.current).toEqual({ name: 'list' });
  });

  it('katman kapatma geçmiş girdisini geri alır; aynı katman iki kez girdi üretmez', () => {
    nav.open('drawer');
    nav.open('drawer');
    expect(h.entries.length).toBe(2);
    nav.close();
    expect(nav.layer).toBeNull();
    expect(h.index).toBe(0);
    nav.close();
    expect(h.index).toBe(0);
  });

  it('çekmece açıkken itilen ekran çekmeceyi kapatır ve girdisini devralır', () => {
    nav.open('drawer');
    nav.push({ name: 'settings', section: null });
    expect(nav.layer).toBeNull();
    expect(nav.current).toEqual({ name: 'settings', section: null });
    expect(h.entries).toEqual([{ mh: 0 }, { mh: 1 }]);
    nav.back();
    expect(nav.current).toEqual({ name: 'list' });
    expect(nav.layer).toBeNull();
  });

  it('pop: açık alt sayfayla birlikte ekran tek adımda çıkar', () => {
    nav.push({ name: 'read', id: 3 });
    nav.open('sheet');
    expect(h.entries.length).toBe(3);
    expect(nav.pop()).toBe(true);
    expect(nav.layer).toBeNull();
    expect(nav.current).toEqual({ name: 'list' });
    expect(h.index).toBe(0);
    // Kökte çıkılacak ekran yok.
    expect(nav.pop()).toBe(false);
    // Katmansız ekranda tek adım.
    nav.push({ name: 'search' });
    expect(nav.pop()).toBe(true);
    expect(nav.current).toEqual({ name: 'list' });
    expect(h.index).toBe(0);
  });

  it('bir katman başka katmanın yerine geçer', () => {
    nav.open('drawer');
    nav.open('sheet');
    expect(nav.layer).toBe('sheet');
    expect(h.entries.length).toBe(2);
    nav.back();
    expect(nav.layer).toBeNull();
  });

  it('geri tuşu birden çok girdi atlarsa yığın hedefe indirgenir', () => {
    nav.push({ name: 'read', id: 1 });
    nav.push({ name: 'search' });
    nav.open('drawer');
    nav.onPopState({ mh: 1 });
    expect(nav.layer).toBeNull();
    expect(nav.current).toEqual({ name: 'read', id: 1 });
    // Bizim olmayan (durumsuz) girdi kök sayılır.
    nav.onPopState(null);
    expect(nav.current).toEqual({ name: 'list' });
  });

  it('ileri tuşu çıkan ekranı geri yüklemez, girdiyi yeniden işaretler', () => {
    nav.push({ name: 'read', id: 1 });
    nav.back();
    h.forward();
    expect(nav.current).toEqual({ name: 'list' });
    expect(h.entries[1]).toEqual({ mh: 0 });
  });

  it('replace üstteki ekranı değiştirir, girdi eklemez', () => {
    nav.push({ name: 'read', id: 1 });
    nav.replace({ name: 'read', id: 2 });
    expect(nav.current).toEqual({ name: 'read', id: 2 });
    expect(h.entries.length).toBe(2);
  });

  it('reset köke döner', () => {
    nav.push({ name: 'read', id: 1 });
    nav.open('drawer');
    nav.reset();
    expect(nav.stack).toEqual([{ name: 'list' }]);
    expect(nav.layer).toBeNull();
    expect(nav.depth()).toBe(0);
    // Geçmiş girdileri de geri alınır: Android geri tuşu ölü girdiye düşmez.
    expect(h.index).toBe(0);
  });

  it('home tüm girdileri tek seferde geri alır; geçmiş imleci köke iner, kökte sessiz', () => {
    nav.push({ name: 'settings', section: null });
    nav.push({ name: 'settings', section: 'devices' });
    nav.open('sheet');
    expect(h.index).toBe(3);
    nav.home();
    expect(nav.stack).toEqual([{ name: 'list' }]);
    expect(nav.layer).toBeNull();
    expect(h.index).toBe(0);
    nav.home();
    expect(h.index).toBe(0);
  });
});
