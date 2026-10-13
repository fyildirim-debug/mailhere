import { describe, expect, it, vi } from 'vitest';
import { createMockBackend } from './mock';

const NOW = new Date(2026, 9, 13, 10, 0);
const inbox = { view: { kind: 'inbox' as const }, accountId: null, category: null, cursor: null, limit: 100 };

describe('sahte arka uç sözleşmeyi izler', () => {
  it('gelen kutusu yeniden eskiye, sayaçlar doğru', async () => {
    const b = createMockBackend({ now: NOW });
    const page = await b.call('messages_list', { query: inbox });
    expect(page.items.length).toBeGreaterThan(5);
    const dates = page.items.map((m) => m.date);
    expect([...dates].sort((a, b) => b - a)).toEqual(dates);
    const counts = await b.call('counts');
    expect(counts.inboxUnread).toBe(3);
  });

  it('kategori ve hesap süzgeci', async () => {
    const b = createMockBackend({ now: NOW });
    const news = await b.call('messages_list', { query: { ...inbox, category: 'newsletters' } });
    expect(news.items.every((m) => m.category === 'newsletters')).toBe(true);
    const work = await b.call('messages_list', { query: { ...inbox, accountId: 'demo-is' } });
    expect(work.items.every((m) => m.accountId === 'demo-is')).toBe(true);
  });

  it('sayfalama imleci tekrarsız devam eder', async () => {
    const b = createMockBackend({ now: NOW });
    const first = await b.call('messages_list', { query: { ...inbox, limit: 3 } });
    expect(first.nextCursor).not.toBeNull();
    const second = await b.call('messages_list', { query: { ...inbox, limit: 3, cursor: first.nextCursor } });
    const ids = new Set(first.items.map((m) => m.id));
    expect(second.items.some((m) => ids.has(m.id))).toBe(false);
  });

  it('arşivle ve geri getir', async () => {
    const b = createMockBackend({ now: NOW });
    const [m] = (await b.call('messages_list', { query: inbox })).items;
    await b.call('messages_move', { ids: [m.id], target: 'archive' });
    expect((await b.call('messages_list', { query: inbox })).items.some((x) => x.id === m.id)).toBe(false);
    const arch = await b.call('messages_list', { query: { ...inbox, view: { kind: 'archive' } } });
    expect(arch.items.some((x) => x.id === m.id)).toBe(true);
    await b.call('messages_move', { ids: [m.id], target: 'inbox' });
    expect((await b.call('messages_list', { query: inbox })).items.some((x) => x.id === m.id)).toBe(true);
  });

  it('ertelenen posta zamanı gelene dek gelen kutusunda görünmez', async () => {
    const b = createMockBackend({ now: NOW });
    const [m] = (await b.call('messages_list', { query: inbox })).items;
    await b.call('messages_snooze', { ids: [m.id], until: NOW.getTime() + 3_600_000 });
    expect((await b.call('counts')).snoozed).toBe(1);
    expect((await b.call('messages_list', { query: inbox })).items.some((x) => x.id === m.id)).toBe(false);
  });

  it('konuşma önceki iletileri eskiden yeniye verir', async () => {
    const b = createMockBackend({ now: NOW });
    const m = (await b.call('search', { q: 'Q4', limit: 10 }))[0];
    const d = await b.call('message_get', { id: m.id });
    expect(d.thread.length).toBe(2);
    expect(d.thread[0].date).toBeLessThan(d.thread[1].date);
  });

  it('uzak içerik izinle açılır ve ayara yazılır', async () => {
    const b = createMockBackend({ now: NOW });
    const m = (await b.call('search', { q: 'Mimar', limit: 5 }))[0];
    expect((await b.call('message_get', { id: m.id })).bodyHtml).not.toContain(' src=');
    const d = await b.call('remote_allow', { id: m.id });
    expect(d.remoteAllowed).toBe(true);
    expect(d.bodyHtml).toContain('src=');
    expect((await b.call('settings_get')).remoteAllow).toContain('bulten@mimarhaftalik.com');
  });

  it('gönderim geri alma süresinde iptal edilebilir', async () => {
    vi.useFakeTimers();
    try {
      const b = createMockBackend();
      const sent = vi.fn();
      b.on('mh:outbox', sent);
      const item = await b.call('send', {
        msg: { accountId: 'demo-is', to: [{ name: null, email: 'a@b.co' }], cc: [], bcc: [], subject: 'x', bodyText: 'y', inReplyTo: null, attachments: [], sendAt: null },
      });
      expect(await b.call('send_cancel', { id: item.id })).toBe(true);
      await vi.runAllTimersAsync();
      expect(sent).not.toHaveBeenCalled();
      const again = await b.call('send', {
        msg: { accountId: 'demo-is', to: [{ name: null, email: 'a@b.co' }], cc: [], bcc: [], subject: 'x', bodyText: 'y', inReplyTo: null, attachments: [], sendAt: null },
      });
      await vi.runAllTimersAsync();
      expect(sent).toHaveBeenCalledWith({ id: again.id, ok: true, error: null });
      expect(await b.call('send_cancel', { id: again.id })).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('Türkçe aramada büyük/küçük harf duyarsız', async () => {
    const b = createMockBackend({ now: NOW });
    expect((await b.call('search', { q: 'SÖZLEŞME', limit: 5 })).length).toBe(1);
    expect(await b.call('search', { q: '   ', limit: 5 })).toEqual([]);
  });
});
