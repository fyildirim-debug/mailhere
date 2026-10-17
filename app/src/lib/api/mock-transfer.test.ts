import { afterEach, describe, expect, it, vi } from 'vitest';
import { createMockBackend, mockQr, qrSizeFor } from './mock';
import type { TransferState } from './types';

// QR ile cihazlar arası aktarım: sahte arka uç Rust tarafıyla aynı sözleşmeyi izler
// (transfer_begin / transfer_cancel / transfer_receive / transfer_apply, `mh:transfer` olayı).

const NOW = new Date(2026, 9, 13, 10, 0);

/** Geçerli görünümlü kod (sahte arka uç yalnız biçimi denetler). */
const CODE = 'mailhere://aktar?v=1&h=192.168.1.20&p=51234&t=oKGio6SlpqeoqaqrrK2urw&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8';

afterEach(() => {
  vi.useRealTimers();
});

describe('QR matrisi', () => {
  it('boyut gerçek kodlayıcı gibi metin uzunluğundan seçilir (EC-M, bayt kipi)', () => {
    expect(qrSizeFor('a'.repeat(14))).toBe(21);
    expect(qrSizeFor('a'.repeat(15))).toBe(25);
    expect(qrSizeFor('a'.repeat(155))).toBe(53);
    expect(qrSizeFor('a'.repeat(213))).toBe(57);
    expect(mockQr('a'.repeat(155)).size).toBe(53);
  });

  it('kare, yalnız 0/1 ve bulucu desenli; aynı metin aynı deseni verir', () => {
    const qr = mockQr('mailhere://aktar?v=1', 29);
    expect(qr.size).toBe(29);
    expect(qr.rows).toHaveLength(29);
    for (const row of qr.rows) {
      expect(row).toHaveLength(29);
      expect(row).toMatch(/^[01]+$/);
    }
    // Sol üst bulucu desen: 7×7 çerçeve ve 3×3 çekirdek.
    expect(qr.rows[0].slice(0, 7)).toBe('1111111');
    expect(qr.rows[3].slice(0, 7)).toBe('1011101');
    expect(qr.rows[7].slice(0, 8)).toBe('00000000');
    expect(mockQr('ayni')).toEqual(mockQr('ayni'));
    expect(mockQr('a')).not.toEqual(mockQr('b'));
  });
});

describe('bilgisayar tarafı: teklif', () => {
  it('QR, kod, süre döner; bağlandı → teslim edildi → uygulandı olayları sırayla gelir', async () => {
    vi.useFakeTimers({ now: NOW });
    const b = createMockBackend({ now: NOW });
    const states: TransferState[] = [];
    b.on('mh:transfer', (s) => states.push(s));

    const offer = await b.call('transfer_begin', { includeSecrets: true });
    expect(offer.text).toMatch(/^mailhere:\/\/aktar\?v=1&h=192\.168\.1\.20,172\.17\.0\.1,100\.101\.102\.103,fd12:3456::20&p=51234&t=[A-Za-z0-9_-]{22}&k=[A-Za-z0-9_-]{43}$/);
    expect(offer.hosts).toEqual(['192.168.1.20', '172.17.0.1', '100.101.102.103', 'fd12:3456::20']);
    expect(offer.port).toBe(51234);
    expect(offer.expiresAt).toBe(NOW.getTime() + 180_000);
    expect(offer.qr.size).toBe(offer.qr.rows.length);
    expect(offer.qr).toEqual(mockQr(offer.text));

    await vi.advanceTimersByTimeAsync(0);
    expect(states.map((s) => s.state)).toEqual(['waiting']);
    await vi.advanceTimersByTimeAsync(1000);
    expect(states.map((s) => s.state)).toEqual(['waiting', 'connected']);
    await vi.advanceTimersByTimeAsync(1000);
    expect(states.map((s) => s.state)).toEqual(['waiting', 'connected', 'delivered']);
    await vi.advanceTimersByTimeAsync(2000);
    expect(states.map((s) => s.state)).toEqual(['waiting', 'connected', 'delivered', 'applied']);
    // Bitmiş teklif süre dolunca yeniden olay üretmez.
    await vi.advanceTimersByTimeAsync(200_000);
    expect(states).toHaveLength(4);
    expect(states.every((s) => s.message === null)).toBe(true);
  });

  it('vazgeçince cancelled gelir ve sonraki olaylar kesilir; teklif yokken sessiz', async () => {
    vi.useFakeTimers({ now: NOW });
    const b = createMockBackend({ now: NOW });
    const states: string[] = [];
    b.on('mh:transfer', (s) => states.push(s.state));
    await b.call('transfer_begin', { includeSecrets: false });
    await vi.advanceTimersByTimeAsync(1000);
    await b.call('transfer_cancel');
    expect(states).toEqual(['waiting', 'connected', 'cancelled']);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(states).toEqual(['waiting', 'connected', 'cancelled']);
    await b.call('transfer_cancel');
    expect(states).toHaveLength(3);
  });

  it('yeni teklif eskisini sessizce kapatır', async () => {
    vi.useFakeTimers({ now: NOW });
    const b = createMockBackend({ now: NOW });
    const states: TransferState[] = [];
    b.on('mh:transfer', (s) => states.push(s));
    const first = await b.call('transfer_begin', { includeSecrets: true });
    await vi.advanceTimersByTimeAsync(500);
    const second = await b.call('transfer_begin', { includeSecrets: true });
    expect(second.text).not.toBe(first.text);
    await vi.advanceTimersByTimeAsync(600);
    // Eski teklifin "connected"ı (1 s) gelmez; yalnız yeni teklifin waiting'i.
    expect(states.map((s) => s.state)).toEqual(['waiting', 'waiting']);
    await vi.advanceTimersByTimeAsync(400);
    expect(states.map((s) => s.state)).toEqual(['waiting', 'waiting', 'connected']);
  });
});

describe('telefon tarafı: alma ve uygulama', () => {
  it('bozuk, yeni sürümlü ve genel adresli kod reddedilir', async () => {
    const b = createMockBackend({ now: NOW });
    await expect(b.call('transfer_receive', { code: '' })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('transfer_receive', { code: 'https://ornek.com/?v=1' })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('transfer_receive', { code: CODE.replace('&k=', '&x=') })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('transfer_receive', { code: CODE.replace('v=1', 'v=2') })).rejects.toMatchObject({
      code: 'invalid',
      message: expect.stringContaining('güncelle'),
    });
    await expect(b.call('transfer_receive', { code: CODE.replace('192.168.1.20', '8.8.8.8') })).rejects.toMatchObject({
      code: 'invalid',
      message: expect.stringContaining('yerel ağ dışında'),
    });
    await expect(b.call('transfer_receive', { code: CODE.replace('192.168.1.20', '10.255.255.1') })).rejects.toMatchObject({ code: 'network' });
  });

  it('önizleme: iki hesap (biri bu cihazda var), kurallar, etiketler, ayarlar; gizli değer yok', async () => {
    const b = createMockBackend({ now: NOW });
    const pv = await b.call('transfer_receive', { code: ` ${CODE}\n` });
    expect(pv.previewId).toBeTruthy();
    expect(pv.fromVersion).toBe('0.4.0');
    expect(pv.fromPlatform).toBe('linux');
    expect(pv.createdAt).toBe(NOW.getTime());
    expect(pv.accounts).toHaveLength(2);
    expect(pv.accounts[0]).toMatchObject({ id: 'demo-is', kind: 'imap', auth: 'password', hasSecret: true, exists: true });
    expect(pv.accounts[1]).toMatchObject({ id: 'aktarim-google', auth: 'google', hasSecret: false, exists: false });
    expect(pv.rules).toBe(3);
    expect(pv.tags).toBe(2);
    expect(pv.settings).toBe(true);
    expect(JSON.stringify(pv)).not.toMatch(/"value"|"secret"/);

    // Boş cihazda hiçbir hesap yoktur.
    const empty = createMockBackend({ now: NOW, empty: true });
    const pv2 = await empty.call('transfer_receive', { code: CODE });
    expect(pv2.accounts.every((a) => !a.exists)).toBe(true);
  });

  it('uygulama: var olan güncellenir, yeni eklenir, etiket adı eşlenir, hesabı olmayan kural atlanır, cihaza özgü ayarlar korunur', async () => {
    const b = createMockBackend({ now: NOW });
    const local = await b.call('settings_set', {
      settings: {
        ...b.state.settings, cardSeconds: 20, cardPosition: 'bottomLeft', nativeNotifications: true, autoUpdate: false, theme: 'light',
        notifyActions: ['reply', 'spam'], swipeLeft: 'spam', swipeRight: 'move', afterRemove: 'previous',
      },
    });
    expect(local.cardSeconds).toBe(20);
    const changed = vi.fn();
    const settings = vi.fn();
    b.on('mh:changed', changed);
    b.on('mh:settings', settings);
    const before = b.state.accounts.find((a) => a.id === 'demo-is')!;
    const tagsBefore = b.state.tags.length;

    const pv = await b.call('transfer_receive', { code: CODE });
    const report = await b.call('transfer_apply', {
      previewId: pv.previewId,
      choice: { accounts: ['demo-is', 'aktarim-google'], rules: true, tags: true, settings: true },
    });
    expect(report).toEqual({
      accountsAdded: 1,
      accountsUpdated: 1,
      accountsWithoutSecret: ['furkan.yildirim@gmail.com'],
      rules: 2,
      rulesSkipped: 1,
      tags: 1,
      settings: true,
    });
    // Hesap: alanlar güncellendi, ekleniş zamanı korundu; yeni hesabın kutuları var.
    const work = b.state.accounts.find((a) => a.id === 'demo-is')!;
    expect(work.displayName).toBe('Furkan Yıldırım');
    expect(work.createdAt).toBe(before.createdAt);
    expect(b.state.accounts.some((a) => a.id === 'aktarim-google')).toBe(true);
    expect((await b.call('mailboxes_list')).some((m) => m.accountId === 'aktarim-google' && m.role === 'inbox')).toBe(true);
    // Etiket: "Fatura" yereldekine eşlendi, "Seyahat" eklendi.
    expect(b.state.tags).toHaveLength(tagsBefore + 1);
    expect(b.state.tags.filter((t) => t.name === 'Fatura')).toHaveLength(1);
    expect(b.state.tags.find((t) => t.name === 'Seyahat')).toMatchObject({ id: 'aktarim-etiket-seyahat', color: '#2A9BA8' });
    // Kural: etiket eylemi yerel kimliğe yazıldı; eski hesabın kuralı yok.
    const rules = await b.call('rules_list');
    expect(rules.find((r) => r.id === 'aktarim-kural-fatura')?.actions).toEqual([{ kind: 'tag', tagId: 'etiket-fatura' }]);
    expect(rules.some((r) => r.id === 'aktarim-kural-toplanti')).toBe(true);
    expect(rules.some((r) => r.id === 'aktarim-kural-eski')).toBe(false);
    // Ayarlar: tema ve yoğunluk geldi, cihaza özgü alanlar kaldı.
    const s = await b.call('settings_get');
    expect(s.theme).toBe('dark');
    expect(s.density).toBe('compact');
    expect(s.vip).toEqual(['elif@ornek.com.tr']);
    expect(s.cardSeconds).toBe(20);
    expect(s.cardPosition).toBe('bottomLeft');
    expect(s.nativeNotifications).toBe(true);
    expect(s.autoUpdate).toBe(false);
    // Bildirim düğmeleri, kaydırma ve silme sonrası davranış da bu cihazın.
    expect(s.notifyActions).toEqual(['reply', 'spam']);
    expect([s.swipeLeft, s.swipeRight, s.afterRemove]).toEqual(['spam', 'move', 'previous']);
    expect(settings).toHaveBeenCalledTimes(1);
    expect(changed).toHaveBeenCalled();
    // Aynı önizleme ikinci kez uygulanamaz.
    await expect(b.call('transfer_apply', { previewId: pv.previewId, choice: { accounts: [], rules: false, tags: false, settings: false } })).rejects.toMatchObject({
      code: 'notFound',
    });
  });

  it('seçilmeyenler uygulanmaz; hesabı seçilmeyen kural atlanır; etiket seçilmese de ad eşlemesi kurulur', async () => {
    const b = createMockBackend({ now: NOW, empty: true });
    const pv = await b.call('transfer_receive', { code: CODE });
    const report = await b.call('transfer_apply', {
      previewId: pv.previewId,
      choice: { accounts: [], rules: true, tags: false, settings: false },
    });
    expect(report).toEqual({
      accountsAdded: 0,
      accountsUpdated: 0,
      accountsWithoutSecret: [],
      // Etiket eklenmedi ve yerelde de yok: etiket kuralı eylemsiz kaldı → atlandı; demo-is ve eski-hesap yok → atlandı.
      rules: 0,
      rulesSkipped: 3,
      tags: 0,
      settings: false,
    });
    expect(b.state.accounts).toHaveLength(0);
    expect(b.state.tags).toHaveLength(0);
    expect(b.state.settings.theme).toBe('system');
  });

  it('önizleme 5 dakika sonra düşer', async () => {
    vi.useFakeTimers({ now: NOW });
    const b = createMockBackend();
    const p = b.call('transfer_receive', { code: CODE });
    await vi.advanceTimersByTimeAsync(300);
    const pv = await p;
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    await expect(b.call('transfer_apply', { previewId: pv.previewId, choice: { accounts: [], rules: false, tags: false, settings: false } })).rejects.toMatchObject({
      code: 'notFound',
      message: expect.stringContaining('süresi doldu'),
    });
  });
});
