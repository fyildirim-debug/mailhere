import { beforeEach, describe, expect, it, vi } from 'vitest';

let status = 'prompt';
const calls: string[] = [];
vi.mock('./api', () => ({
  call: async (name: string) => {
    calls.push(name);
    if (name === 'notification_status') return status;
    if (name === 'notification_permission') return 'granted';
    throw new Error('bilinmeyen komut');
  },
}));

function storage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
    data,
  };
}

async function load() {
  vi.resetModules();
  return import('./notify-permission');
}

describe('askNotificationsOnce', () => {
  beforeEach(() => {
    calls.length = 0;
    status = 'prompt';
  });

  it('kararsızken bir kez ister; aynı anda gelen çağrılar tek isteğe bağlanır', async () => {
    const s = storage();
    vi.stubGlobal('localStorage', s);
    const { askNotificationsOnce } = await load();
    await Promise.all([askNotificationsOnce(), askNotificationsOnce()]);
    expect(calls).toEqual(['notification_status', 'notification_permission']);
    expect(s.data.get('mailhere.notifyAsked')).toBe('1');
  });

  it('daha önce sorulduysa hiçbir komut çağrılmaz', async () => {
    vi.stubGlobal('localStorage', storage({ 'mailhere.notifyAsked': '1' }));
    const { askNotificationsOnce } = await load();
    await askNotificationsOnce();
    expect(calls).toEqual([]);
  });

  it('izin zaten verildiyse ya da kalıcı reddedildiyse sistem penceresi açılmaz', async () => {
    for (const st of ['granted', 'denied']) {
      calls.length = 0;
      status = st;
      const s = storage();
      vi.stubGlobal('localStorage', s);
      const { askNotificationsOnce } = await load();
      await askNotificationsOnce();
      expect(calls).toEqual(['notification_status']);
      expect(s.data.get('mailhere.notifyAsked')).toBe('1');
    }
  });

  it('komut yoksa (masaüstü) sorulmuş sayılmaz', async () => {
    status = 'unsupported';
    const s = storage();
    vi.stubGlobal('localStorage', s);
    const { askNotificationsOnce, notifyPermission } = await load();
    await askNotificationsOnce();
    expect(await notifyPermission()).toBe('unsupported');
    expect(s.data.has('mailhere.notifyAsked')).toBe(false);
  });
});
