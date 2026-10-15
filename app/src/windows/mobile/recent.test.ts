import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { loadRecent, pushRecent, RECENT_MAX, saveRecent } from './recent';

/** Testler node ortamında: tarayıcı deposunun küçük bir sahtesi. */
function fakeStorage() {
  const m = new Map<string, string>();
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, String(v)),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  };
}

describe('son aramalar', () => {
  beforeEach(() => vi.stubGlobal('localStorage', fakeStorage()));
  afterEach(() => vi.unstubAllGlobals());

  it('yeni sorgu başa gelir, yinelenen (harf farkı da) başa taşınır, en çok 6', () => {
    let list: string[] = [];
    for (const q of ['fatura', 'from:elif', 'toplantı', 'has:ek', 'kargo', 'proje', 'sözleşme']) list = pushRecent(list, q);
    expect(list).toHaveLength(RECENT_MAX);
    expect(list[0]).toBe('sözleşme');
    expect(list).not.toContain('fatura');
    list = pushRecent(list, '  FROM:ELİF  ');
    expect(list[0]).toBe('FROM:ELİF');
    expect(list.filter((x) => x.toLocaleLowerCase('tr') === 'from:elif')).toHaveLength(1);
    expect(pushRecent(list, '   ')).toBe(list);
  });

  it('cihazda saklanır; bozuk kayıt boş liste verir', () => {
    saveRecent(['a', 'b']);
    expect(loadRecent()).toEqual(['a', 'b']);
    saveRecent([]);
    expect(loadRecent()).toEqual([]);
    localStorage.setItem('mailhere.mob.recentSearches', '{bozuk');
    expect(loadRecent()).toEqual([]);
    localStorage.setItem('mailhere.mob.recentSearches', JSON.stringify(['x', 3, '', 'y']));
    expect(loadRecent()).toEqual(['x', 'y']);
  });

  it('depolama kapalıysa (erişim hata verir) boş liste, kayıt sessizce geçer', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('kapalı');
      },
      setItem: () => {
        throw new Error('kapalı');
      },
      removeItem: () => {
        throw new Error('kapalı');
      },
    });
    expect(loadRecent()).toEqual([]);
    expect(() => saveRecent(['a'])).not.toThrow();
  });
});
