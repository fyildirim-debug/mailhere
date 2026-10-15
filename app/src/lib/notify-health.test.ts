import { beforeEach, describe, expect, it, vi } from 'vitest';

let supported = true;
let battery = 'optimized';
let mode = false;
const calls: string[] = [];
vi.mock('./api', () => ({
  call: async (name: string, args?: { on?: boolean }) => {
    calls.push(name);
    if (!supported) throw { code: 'invalid', message: 'Bu ayar yalnız Android\'de var.' };
    if (name === 'battery_status') return battery;
    if (name === 'battery_settings') {
      battery = 'unrestricted';
      return null;
    }
    if (name === 'background_mode_get') return mode;
    if (name === 'background_mode_set') {
      mode = Boolean(args?.on);
      return mode;
    }
    throw new Error('bilinmeyen komut');
  },
}));

const health = () => import('./notify-health');

describe('bildirim sağlığı', () => {
  beforeEach(() => {
    calls.length = 0;
    supported = true;
    battery = 'optimized';
    mode = false;
  });

  it('pil kısıtlaması kaldırılınca durum yeniden okunur', async () => {
    const { batteryStatus, liftBatteryRestriction } = await health();
    expect(await batteryStatus()).toBe('optimized');
    expect(await liftBatteryRestriction()).toBe('unrestricted');
    expect(calls).toEqual(['battery_status', 'battery_settings', 'battery_status']);
  });

  it('anlık bildirim açılır ve kapanır', async () => {
    const { backgroundMode, setBackgroundMode } = await health();
    expect(await backgroundMode()).toBe(false);
    expect(await setBackgroundMode(true)).toBe(true);
    expect(await backgroundMode()).toBe(true);
    expect(await setBackgroundMode(false)).toBe(false);
  });

  it('komut yoksa (masaüstü) desteklenmiyor sayılır', async () => {
    supported = false;
    const { backgroundMode, batteryStatus, setBackgroundMode } = await health();
    expect(await batteryStatus()).toBeNull();
    expect(await backgroundMode()).toBeNull();
    // Açma isteği sessizce yutulmaz: arayüz hatayı gösterir.
    await expect(setBackgroundMode(true)).rejects.toMatchObject({ code: 'invalid' });
  });
});
