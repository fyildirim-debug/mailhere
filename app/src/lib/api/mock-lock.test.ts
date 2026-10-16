import { describe, expect, it } from 'vitest';
import { createMockBackend, lockAllows, lockRetryDelayMs } from './mock';

// Parola kilidi: sahte arka uç Rust tarafıyla aynı sözleşmeyi izler
// (uzunluk, yanlış parola sayacı ve üstel bekleme, komut kapısı, sıfırlama).

const NOW = new Date(2026, 9, 16, 10, 0);

describe('parola kilidi', () => {
  it('varsayılan kapalı; kurma parolayı doğrular ve durumu yayar', async () => {
    const b = createMockBackend({ now: NOW });
    expect(await b.call('lock_status')).toEqual({ enabled: false, locked: false, uiLocked: false, relockOnOpen: true, retryAfterMs: 0 });
    await expect(b.call('lock_enable', { password: 'kisa' })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('lock_disable', { password: 'ne olursa' })).rejects.toMatchObject({ code: 'invalid' });
    const seen: unknown[] = [];
    b.on('mh:lock', (s) => seen.push(s));
    const s = await b.call('lock_enable', { password: 'çok gizli parola' });
    expect(s).toMatchObject({ enabled: true, locked: false, uiLocked: false, relockOnOpen: true });
    expect(seen).toHaveLength(1);
    await expect(b.call('lock_enable', { password: 'çok gizli parola' })).rejects.toMatchObject({ code: 'invalid' });
    // Kilit açıkken posta komutları çalışır (yalnız arayüz kilitlenince durur).
    expect((await b.call('accounts_list')).length).toBeGreaterThan(0);
  });

  it('arayüz kilidi içerik komutlarını kapar; doğru parola açar, yanlış parola sayılır ve bekletir', async () => {
    const b = createMockBackend({ now: NOW });
    await b.call('lock_enable', { password: 'çok gizli parola' });
    await b.call('lock_ui_lock');
    expect(await b.call('lock_status')).toMatchObject({ uiLocked: true, locked: false });
    await expect(b.call('messages_list', { query: { view: { kind: 'inbox' }, accountId: null, category: null, only: null, cursor: null, limit: 10 } })).rejects.toMatchObject({ code: 'locked' });
    await expect(b.call('settings_get')).rejects.toMatchObject({ code: 'locked' });
    // İçeriksiz komutlar geçer.
    await expect(b.call('sync_now')).resolves.toBeNull();

    for (let i = 1; i <= 5; i++) {
      await expect(b.call('lock_unlock', { password: 'yanlis' })).rejects.toMatchObject({ code: 'auth' });
      expect((await b.call('lock_status')).retryAfterMs).toBe(0);
    }
    // Altıncı hata 2 sn bekletir; bekleme bitmeden deneme `locked`.
    await expect(b.call('lock_unlock', { password: 'yanlis' })).rejects.toMatchObject({ code: 'auth' });
    expect((await b.call('lock_status')).retryAfterMs).toBe(2000);
    await expect(b.call('lock_unlock', { password: 'çok gizli parola' })).rejects.toMatchObject({ code: 'locked' });
    b.state.lock.retryAfter = 0;
    const s = await b.call('lock_unlock', { password: 'çok gizli parola' });
    expect(s).toMatchObject({ enabled: true, uiLocked: false, locked: false, retryAfterMs: 0 });
    expect(b.state.lock.failures).toBe(0);
    expect((await b.call('accounts_list')).length).toBeGreaterThan(0);
  });

  it('parola değiştirme ve kapatma mevcut parolayı ister', async () => {
    const b = createMockBackend({ now: NOW });
    await b.call('lock_enable', { password: 'çok gizli parola' });
    await expect(b.call('lock_change', { current: 'yanlis', next: 'yeni parola 12' })).rejects.toMatchObject({ code: 'auth' });
    await expect(b.call('lock_change', { current: 'çok gizli parola', next: 'kisa' })).rejects.toMatchObject({ code: 'invalid' });
    await b.call('lock_change', { current: 'çok gizli parola', next: 'yeni parola 12' });
    await expect(b.call('lock_disable', { password: 'çok gizli parola' })).rejects.toMatchObject({ code: 'auth' });
    await b.call('lock_set_relock', { on: false });
    expect((await b.call('lock_status')).relockOnOpen).toBe(false);
    const s = await b.call('lock_disable', { password: 'yeni parola 12' });
    expect(s).toMatchObject({ enabled: false, locked: false, uiLocked: false, relockOnOpen: true });
    // Kapalıyken arayüz kilidi istenirse bir şey olmaz.
    expect((await b.call('lock_ui_lock')).uiLocked).toBe(false);
  });

  it('kilitli açılış: çekirdek kapalıyken yalnız lock_* geçer; unutulan parola yerel veriyi siler', async () => {
    const b = createMockBackend({ now: NOW });
    await b.call('lock_enable', { password: 'çok gizli parola' });
    // Yeniden başlatma benzetimi.
    b.state.lock.locked = true;
    await expect(b.call('accounts_list')).rejects.toMatchObject({ code: 'locked' });
    await expect(b.call('sync_now')).rejects.toMatchObject({ code: 'locked' });
    expect((await b.call('lock_status')).locked).toBe(true);
    const s = await b.call('lock_reset_local');
    expect(s).toMatchObject({ enabled: false, locked: false });
    expect(b.state.accounts).toEqual([]);
    expect(b.state.messages).toEqual([]);
    expect(b.state.lock.password).toBeNull();
    expect(await b.call('accounts_list')).toEqual([]);
  });

  it('kapı ve bekleme tabloları Rust ile aynı', () => {
    expect(lockRetryDelayMs(5)).toBe(0);
    expect(lockRetryDelayMs(6)).toBe(2000);
    expect(lockRetryDelayMs(10)).toBe(32_000);
    expect(lockRetryDelayMs(14)).toBe(300_000);
    expect(lockRetryDelayMs(99)).toBe(300_000);
    expect(lockAllows('lock_unlock', false, false)).toBe(true);
    expect(lockAllows('messages_list', false, false)).toBe(false);
    expect(lockAllows('notify_hide', true, true)).toBe(true);
    expect(lockAllows('message_get', true, true)).toBe(false);
    expect(lockAllows('message_get', true, false)).toBe(true);
  });
});
