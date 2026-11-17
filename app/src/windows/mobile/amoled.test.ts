import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/** Testler node ortamında: tarayıcı deposunun ve kök öğenin küçük sahteleri. */
function fakeStorage(seed: Record<string, string> = {}) {
  const m = new Map<string, string>(Object.entries(seed));
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, String(v)),
    removeItem: (k: string) => void m.delete(k),
  };
}

function fakeDocument() {
  const attrs = new Set<string>();
  return {
    attrs,
    documentElement: {
      toggleAttribute: (name: string, on: boolean) => void (on ? attrs.add(name) : attrs.delete(name)),
    },
  };
}

describe('AMOLED siyah tercihi', () => {
  beforeEach(() => vi.resetModules());
  afterEach(() => vi.unstubAllGlobals());

  it('varsayılan kapalı; açılınca saklanır ve kök öğeye yazılır, kapatılınca silinir', async () => {
    const store = fakeStorage();
    const doc = fakeDocument();
    vi.stubGlobal('localStorage', store);
    vi.stubGlobal('document', doc);
    const m = await import('./amoled.svelte');
    expect(m.amoled.on).toBe(false);
    expect(doc.attrs.has('data-amoled')).toBe(false);

    m.setAmoled(true);
    expect(m.amoled.on).toBe(true);
    expect(store.getItem('mailhere.mob.amoled')).toBe('1');
    expect(doc.attrs.has('data-amoled')).toBe(true);

    m.setAmoled(false);
    expect(store.getItem('mailhere.mob.amoled')).toBeNull();
    expect(doc.attrs.has('data-amoled')).toBe(false);
  });

  it('saklanan tercih açılışta uygulanır', async () => {
    const doc = fakeDocument();
    vi.stubGlobal('localStorage', fakeStorage({ 'mailhere.mob.amoled': '1' }));
    vi.stubGlobal('document', doc);
    const m = await import('./amoled.svelte');
    expect(m.amoled.on).toBe(true);
    expect(doc.attrs.has('data-amoled')).toBe(true);
  });

  it('depolama kapalıysa (gizli pencere) çökmez; tercih oturum boyunca geçerlidir', async () => {
    const doc = fakeDocument();
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
    vi.stubGlobal('document', doc);
    const m = await import('./amoled.svelte');
    expect(m.amoled.on).toBe(false);
    m.setAmoled(true);
    expect(m.amoled.on).toBe(true);
    expect(doc.attrs.has('data-amoled')).toBe(true);
  });
});
