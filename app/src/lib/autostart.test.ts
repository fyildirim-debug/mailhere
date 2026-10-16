import { beforeEach, describe, expect, it, vi } from 'vitest';

let supported = true;
let on = true;
const calls: string[] = [];
vi.mock('./api', () => ({
  call: async (name: string, args?: { on?: boolean }) => {
    calls.push(name);
    if (!supported) throw { code: 'invalid', message: 'Bu ayar yalnız Android\'de var.' };
    if (name === 'autostart_get') return on;
    if (name === 'autostart_set') {
      on = Boolean(args?.on);
      return on;
    }
    throw new Error('bilinmeyen komut');
  },
}));

const mod = () => import('./autostart');

describe('açılışta başlama', () => {
  beforeEach(() => {
    calls.length = 0;
    supported = true;
    on = true;
  });

  it('varsayılan açık; kapatılır ve yeniden açılır', async () => {
    const { autostart, setAutostart } = await mod();
    expect(await autostart()).toBe(true);
    expect(await setAutostart(false)).toBe(false);
    expect(await autostart()).toBe(false);
    expect(await setAutostart(true)).toBe(true);
    expect(calls).toEqual(['autostart_get', 'autostart_set', 'autostart_get', 'autostart_set']);
  });

  it('komut yoksa desteklenmiyor sayılır, açma hatası yutulmaz', async () => {
    supported = false;
    const { autostart, setAutostart } = await mod();
    expect(await autostart()).toBeNull();
    await expect(setAutostart(true)).rejects.toMatchObject({ code: 'invalid' });
  });
});
