import { describe, expect, it, vi } from 'vitest';
import { createMockBackend } from './mock';

// Yanıt hatırlatması ve yeni gönderici eleği: sahte arka uç Rust tarafıyla aynı sözleşmeyi izler.

const NOW = new Date(2026, 9, 13, 10, 0);
const inbox = { view: { kind: 'inbox' as const }, accountId: null, category: null, cursor: null, limit: 100 };

describe('yanıt hatırlatması', () => {
  it('yalnız gönderilmiş postaya kurulur; süresi dolunca gelen kutusunun üstüne düşer', async () => {
    const b = createMockBackend({ now: NOW });
    const sent = (await b.call('messages_list', { query: { ...inbox, view: { kind: 'sent' } } })).items;
    expect(sent.length).toBeGreaterThan(0);
    const received = (await b.call('messages_list', { query: inbox })).items[0];
    await expect(b.call('followup_set', { id: received.id, at: NOW.getTime() + 1000 })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('followup_set', { id: sent[0].id, at: NOW.getTime() - 1 })).rejects.toMatchObject({ code: 'invalid' });

    // Kurulu ama süresi gelmemiş: listede ve sayaçta var, gelen kutusunda yok.
    const soon = NOW.getTime() + 60_000;
    await b.call('followup_set', { id: sent[0].id, at: soon });
    expect((await b.call('counts')).followUps).toBe(1);
    expect((await b.call('followups_list'))[0]).toMatchObject({ id: sent[0].id, at: soon, subject: sent[0].subject });
    expect((await b.call('messages_list', { query: inbox })).items.some((m) => m.id === sent[0].id)).toBe(false);
    expect((await b.call('message_get', { id: sent[0].id })).followUp).toBe(soon);
    const view = await b.call('messages_list', { query: { ...inbox, view: { kind: 'followups' } } });
    expect(view.items.map((m) => m.id)).toEqual([sent[0].id]);

    // Süresi dolunca en üstte, işaretiyle; yalnız ilk sayfada.
    await b.call('followup_set', { id: sent[0].id, at: soon });
    b.state.followUps[sent[0].id].at = NOW.getTime() - 1;
    const page = await b.call('messages_list', { query: inbox });
    expect(page.items[0].id).toBe(sent[0].id);
    expect(page.items[0].followUp).toBe(NOW.getTime() - 1);
    expect(page.items.filter((m) => m.id === sent[0].id).length).toBe(1);
    const next = await b.call('messages_list', { query: { ...inbox, cursor: `${page.items[1].date}:${page.items[1].id}` } });
    expect(next.items.some((m) => m.id === sent[0].id)).toBe(false);

    // Kaldırma.
    await b.call('followup_set', { id: sent[0].id, at: null });
    expect((await b.call('counts')).followUps).toBe(0);
    expect((await b.call('messages_list', { query: inbox })).items[0].id).not.toBe(sent[0].id);
  });

  it('arama ve kişi kartı özetlerinde hatırlatma gerçek değerdir (dizin değil)', async () => {
    const b = createMockBackend({ now: NOW });
    const hits = await b.call('search', { q: 'Q4', limit: 10 });
    expect(hits.length).toBeGreaterThan(1);
    expect(hits.every((m) => m.followUp === null)).toBe(true);
    const sent = hits.find((m) => m.subject === 'Q4 bütçe taslağı')!;
    const soon = NOW.getTime() + 60_000;
    await b.call('followup_set', { id: sent.id, at: soon });
    const again = await b.call('search', { q: 'Q4', limit: 10 });
    expect(again.map((m) => m.followUp)).toEqual(again.map((m) => (m.id === sent.id ? soon : null)));
    const info = await b.call('contact_get', { email: 'kerem.aydin@ornek.com.tr' });
    expect(info.recent.length).toBeGreaterThan(1);
    expect(info.recent.map((m) => m.followUp)).toEqual(info.recent.map((m) => (m.id === sent.id ? soon : null)));
  });

  it('gönderimle birlikte kurulur; alıcı bilinen gönderici olur', async () => {
    vi.useFakeTimers();
    try {
      const b = createMockBackend();
      const at = Date.now() + 86_400_000;
      await b.call('send', {
        msg: { accountId: 'demo-is', to: [{ name: 'Ayşe', email: 'ayse@yeni.com' }], cc: [], bcc: [], subject: 'Teklif', bodyText: 'Merhaba', inReplyTo: null, attachments: [], sendAt: null, followUpAt: at },
      });
      expect(await b.call('followups_list')).toEqual([]);
      await vi.runAllTimersAsync();
      const list = await b.call('followups_list');
      expect(list.length).toBe(1);
      expect(list[0]).toMatchObject({ accountId: 'demo-is', at, subject: 'Teklif' });
      expect(b.state.known).toContain('demo-is|ayse@yeni.com');
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('yeni gönderici eleği', () => {
  it('elekteki posta gelen kutusunda ve sayaçta görünmez; kabul geri getirir, engelleme çöpe atar', async () => {
    const b = createMockBackend({ now: NOW });
    const before = await b.call('counts');
    const [a, c] = (await b.call('messages_list', { query: inbox })).items.filter((m) => m.unread);
    b.state.screened.push(a.id, c.id);
    const counts = await b.call('counts');
    expect(counts.screener).toBe(2);
    expect(counts.inboxUnread).toBe(before.inboxUnread - 2);
    expect((await b.call('messages_list', { query: inbox })).items.some((m) => m.id === a.id)).toBe(false);
    const view = await b.call('messages_list', { query: { ...inbox, view: { kind: 'screener' } } });
    expect(view.items.map((m) => m.id).sort()).toEqual([a.id, c.id].sort());

    const senders = await b.call('screener_list');
    expect(senders.length).toBe(2);
    expect(senders.find((s) => s.email === a.from.email.toLowerCase())).toMatchObject({ accountId: a.accountId, count: 1, subject: a.subject });

    await b.call('screener_decide', { email: a.from.email, accountId: a.accountId, allow: true });
    expect(b.state.known).toContain(`${a.accountId}|${a.from.email.toLowerCase()}`);
    expect((await b.call('messages_list', { query: inbox })).items.some((m) => m.id === a.id)).toBe(true);
    expect((await b.call('counts')).screener).toBe(1);

    await b.call('screener_decide', { email: c.from.email, accountId: c.accountId, allow: false });
    expect((await b.call('counts')).screener).toBe(0);
    const trash = await b.call('messages_list', { query: { ...inbox, view: { kind: 'trash' } } });
    expect(trash.items.some((m) => m.id === c.id)).toBe(true);
    await expect(b.call('screener_decide', { email: 'bozuk', accountId: c.accountId, allow: true })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('screener_decide', { email: 'x@y.com', accountId: 'yok', allow: true })).rejects.toMatchObject({ code: 'notFound' });
  });
});

describe('uzak görsel indirme (remote_images)', () => {
  it('yalnız izinli göndericinin postasında geçen adresleri, tekil, data: olarak döner', async () => {
    const b = createMockBackend({ now: NOW });
    const m = (await b.call('messages_list', { query: inbox })).items.find((x) => x.subject === 'Renk, ışık ve odak')!;
    const cover = 'https://tasarimnotlari.co/sayi-18.png';
    await expect(b.call('remote_images', { id: m.id, urls: [cover] })).rejects.toMatchObject({ code: 'invalid' });
    await b.call('remote_allow', { id: m.id });
    const got = await b.call('remote_images', { id: m.id, urls: [` ${cover} `, cover, 'https://baska.example/x.png', 'https://izleme.tasarimnotlari.co/p.gif'] });
    expect(got.map((g) => g.url)).toEqual([cover, 'https://izleme.tasarimnotlari.co/p.gif']);
    expect(got[0].data).toMatch(/^data:image\/svg\+xml;base64,/);
    expect(got[1].data).toMatch(/^data:image\/png;base64,/);
    await expect(b.call('remote_images', { id: m.id, urls: ['https://baska.example/x.png'] })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('remote_images', { id: 9999, urls: [cover] })).rejects.toMatchObject({ code: 'notFound' });
  });
});
