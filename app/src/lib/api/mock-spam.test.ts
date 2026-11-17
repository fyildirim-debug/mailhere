import { describe, expect, it } from 'vitest';
import { createMockBackend } from './mock';
import { BLOCKED_MAX, defaultSettings, isBlocked, normalizeBlocked, normalizeBlockedEntry, SPAM_THRESHOLD, type Settings } from './types';

// İstenmeyen koruması ve doğrulama kodları: sahte arka uç Rust tarafıyla aynı sözleşmeyi izler
// (docs/tasarim/0.11-istenmeyen-ve-kod.md).

const NOW = new Date(2026, 9, 13, 10, 0);
const all = { accountId: null, category: null, cursor: null, limit: 100 };
const inbox = { ...all, view: { kind: 'inbox' as const } };

function setup() {
  const b = createMockBackend({ now: NOW });
  const bySubject = (subject: string) => b.state.messages.find((m) => m.subject === subject)!;
  const roleOf = (id: number) => b.state.mailboxes.find((x) => x.id === b.state.messages.find((m) => m.id === id)!.mailboxId)!.role;
  return { b, bySubject, roleOf };
}

describe('engellenenler listesi yardımcıları', () => {
  it('sadeleştirme arka uçla aynı: kırp, küçült, geçersizi ve yineleneni at', () => {
    expect(
      normalizeBlocked([' Reklam@Firma.COM ', 'reklam@firma.com', '@Kotu.Example', 'kotu.example', '@com', 'a@', '@', '', 'a b@c.com', 'x@y', 'a@b@c.com', '@iki..nokta.com', 'ÖZEL@şirket.com.tr']),
    ).toEqual(['reklam@firma.com', '@kotu.example', 'özel@şirket.com.tr']);
    expect(normalizeBlockedEntry(' @Firma.com ')).toBe('@firma.com');
    expect(normalizeBlockedEntry('firma.com')).toBeNull();
    expect(normalizeBlocked(Array.from({ length: BLOCKED_MAX + 20 }, (_, i) => `a${i}@b.com`))).toHaveLength(BLOCKED_MAX);
  });

  it('eşleşme: tam adres yalnız kendini, alan adı alt alan adlarını da engeller', () => {
    const list = ['reklam@firma.com', '@kotu.example'];
    expect(isBlocked(list, ' REKLAM@firma.com ')).toBe(true);
    expect(isBlocked(list, 'baska@firma.com')).toBe(false);
    expect(isBlocked(list, 'herkes@kotu.example')).toBe(true);
    expect(isBlocked(list, 'x@posta.kotu.example')).toBe(true);
    expect(isBlocked(list, 'x@cokkotu.example')).toBe(false);
    expect(isBlocked(list, 'kotu.example')).toBe(false);
    expect(isBlocked([], 'reklam@firma.com')).toBe(false);
  });
});

describe('ayar varsayılanları', () => {
  it('uyar, engellenen yok, kod kopyalanmaz ama kopyalanınca okundu sayılır, kod postaları silinmez', async () => {
    expect(defaultSettings).toMatchObject({ blocked: [], spamFilter: 'warn', otpAutoCopy: false, otpMarkRead: true, otpExpireHours: 0 });
    const { b } = setup();
    expect(await b.call('settings_get')).toMatchObject({ blocked: [], spamFilter: 'warn', otpExpireHours: 0 });
    const saved = await b.call('settings_set', {
      settings: { ...defaultSettings, blocked: [' Reklam@Firma.com', 'reklam@firma.com', 'bozuk'], spamFilter: 'move', otpAutoCopy: true, otpExpireHours: 100_000 },
    });
    expect(saved.blocked).toEqual(['reklam@firma.com']);
    expect(saved).toMatchObject({ spamFilter: 'move', otpAutoCopy: true, otpExpireHours: 24 * 30 });
  });
});

describe('tanıtım verisi', () => {
  it('şüpheli posta İstenmeyen kutusunda, taze kodlu posta arşivde; gelen kutusu değişmez', async () => {
    const { b, bySubject, roleOf } = setup();
    const spam = bySubject('Kartınız geçici olarak kapatıldı');
    expect(roleOf(spam.id)).toBe('junk');
    expect(spam.unread).toBe(false);
    const d = await b.call('message_get', { id: spam.id });
    expect(d.spam).toBe(true);
    expect(d.spamVerdict).toEqual({ score: 100, reasons: ['serverFlagged', 'lookalikeDomain', 'replyToMismatch'] });
    expect(d.spamVerdict!.score).toBeGreaterThanOrEqual(SPAM_THRESHOLD);
    expect(d.replyTo).toHaveLength(1);

    const code = bySubject('Yeni cihazdan giriş kodun');
    expect(roleOf(code.id)).toBe('archive');
    expect(code.date).toBe(NOW.getTime() - 5 * 60_000);
    const archive = (await b.call('messages_list', { query: { ...all, view: { kind: 'archive' } } })).items;
    expect(archive[0]).toMatchObject({ id: code.id, otp: '731004', spam: false });
    // Kimlik doğrulaması başarısız örnek de şüphelidir; eski kod postası kodunu özetinde taşır.
    expect(archive.find((m) => m.subject === 'Gönderiniz gümrükte bekliyor')).toMatchObject({ spam: true, otp: null });
    expect(archive.find((m) => m.subject === 'Giriş doğrulama kodun')).toMatchObject({ spam: false, otp: '482913' });

    // Gelen kutusunda şüpheli ya da kodlu posta yok (sayaçlar ve listeler eskisi gibi).
    const rows = (await b.call('messages_list', { query: inbox })).items;
    expect(rows.every((m) => !m.spam && m.otp === null)).toBe(true);
    expect(rows.some((m) => m.id === spam.id || m.id === code.id)).toBe(false);
  });

  it('arama: has:kod, is:şüpheli; istenmeyen kutusu yalnız is:şüpheli ve in: ile aranır', async () => {
    const { b } = setup();
    const subjects = async (q: string) => (await b.call('search', { q, limit: 50 })).map((m) => m.subject);
    expect(await subjects('has:kod')).toEqual(['Yeni cihazdan giriş kodun', 'Giriş doğrulama kodun']);
    expect(await subjects('has:code')).toEqual(await subjects('has:kod'));
    expect(await subjects('is:şüpheli')).toEqual(['Kartınız geçici olarak kapatıldı', 'Gönderiniz gümrükte bekliyor']);
    expect(await subjects('is:spam')).toEqual(await subjects('is:şüpheli'));
    expect(await subjects('is:supheli in:arşiv')).toEqual(['Gönderiniz gümrükte bekliyor']);
    // Düz arama istenmeyen kutusuna bakmaz.
    expect(await subjects('kapatıldı')).toEqual([]);
    expect(await subjects('kapatıldı in:junk')).toEqual(['Kartınız geçici olarak kapatıldı']);
  });
});

describe('göndereni engelle', () => {
  it('adres listeye girer, hesabın gelen kutusundaki postaları İstenmeyen’e taşınır, olaylar yayılır', async () => {
    const { b, bySubject, roleOf } = setup();
    const events: string[] = [];
    let last: Settings | null = null;
    b.on('mh:settings', (s) => {
      events.push('settings');
      last = s;
    });
    b.on('mh:changed', () => events.push('changed'));

    // Kerem'in iş hesabının gelen kutusunda birden çok postası var; biri klasörde.
    const target = bySubject('Davet: Sprint planlama');
    const mine = b.state.messages.filter((m) => m.accountId === target.accountId && m.from.email === target.from.email);
    const inInbox = mine.filter((m) => roleOf(m.id) === 'inbox');
    const elsewhere = mine.filter((m) => roleOf(m.id) !== 'inbox');
    expect(inInbox.length).toBeGreaterThan(1);
    expect(elsewhere.length).toBeGreaterThan(0);

    const r = await b.call('sender_block', { id: target.id });
    expect(r.address).toBe('kerem.aydin@ornek.com.tr');
    expect(r.moved).toBe(inInbox.length);
    expect([...r.ids].sort()).toEqual(inInbox.map((m) => m.id).sort());
    for (const m of inInbox) expect(roleOf(m.id)).toBe('junk');
    for (const m of elsewhere) expect(roleOf(m.id)).not.toBe('junk');
    expect(b.state.settings.blocked).toEqual(['kerem.aydin@ornek.com.tr']);
    expect(events).toEqual(['settings', 'changed']);
    expect(last!.blocked).toEqual(['kerem.aydin@ornek.com.tr']);

    // Okuma alanında neden: engellenenler listesinde.
    expect((await b.call('message_get', { id: target.id })).spamVerdict).toEqual({ score: 100, reasons: ['blocked'] });
    expect((await b.call('message_get', { id: target.id })).spam).toBe(true);

    // İkinci kez: liste yinelenmez, taşınacak posta kalmamıştır.
    const again = await b.call('sender_block', { id: target.id });
    expect(again).toMatchObject({ moved: 0, ids: [] });
    expect(b.state.settings.blocked).toHaveLength(1);

    // Geri al: ayar eski hâline, postalar gelen kutusuna.
    await b.call('settings_set', { settings: { ...b.state.settings, blocked: [] } });
    await b.call('messages_move', { ids: r.ids, target: 'inbox' });
    for (const m of inInbox) expect(roleOf(m.id)).toBe('inbox');
    expect((await b.call('message_get', { id: target.id })).spamVerdict).toBeNull();
  });

  it('İstenmeyen kutusu yoksa Çöp’e taşınır; kendi adresimiz ve bozuk adres engellenemez', async () => {
    const { b, bySubject, roleOf } = setup();
    const m = bySubject('Cumartesi kahvaltı?');
    b.state.mailboxes = b.state.mailboxes.filter((x) => !(x.accountId === m.accountId && x.role === 'junk'));
    const r = await b.call('sender_block', { id: m.id });
    expect(r.moved).toBeGreaterThan(0);
    expect(roleOf(m.id)).toBe('trash');

    const own = b.state.messages.find((x) => roleOf(x.id) === 'sent')!;
    await expect(b.call('sender_block', { id: own.id })).rejects.toMatchObject({ code: 'invalid', message: expect.stringContaining('Kendi adresini') });
    m.from = { name: 'Bozuk', email: 'com' };
    await expect(b.call('sender_block', { id: m.id })).rejects.toMatchObject({ code: 'invalid', message: expect.stringContaining('eksik ya da bozuk') });
    await expect(b.call('spam_trust', { id: m.id })).rejects.toMatchObject({ code: 'invalid' });
    await expect(b.call('sender_block', { id: 9999 })).rejects.toMatchObject({ code: 'notFound' });
    await expect(b.call('spam_trust', { id: 9999 })).rejects.toMatchObject({ code: 'notFound' });
  });

  it('liste doluyken yeni gönderici eklenmez; ne yapılacağı söylenir', async () => {
    const { b, bySubject, roleOf } = setup();
    b.state.settings.blocked = Array.from({ length: BLOCKED_MAX }, (_, i) => `a${i}@b.com`);
    const m = bySubject('Cumartesi kahvaltı?');
    await expect(b.call('sender_block', { id: m.id })).rejects.toMatchObject({ code: 'invalid', message: expect.stringContaining('dolu') });
    expect(roleOf(m.id)).toBe('inbox');
    expect(b.state.settings.blocked).toHaveLength(BLOCKED_MAX);
  });
});

describe('güvenli / istenmeyen değil', () => {
  it('şüpheli işareti kalkar, adres listeden çıkar (alan adı kalır), posta gelen kutusuna döner', async () => {
    const { b, bySubject, roleOf } = setup();
    const spam = bySubject('Kartınız geçici olarak kapatıldı');
    const email = spam.from.email;
    await b.call('settings_set', { settings: { ...b.state.settings, blocked: [email, '@xn--bnka-53d.example', 'baska@firma.com'] } });
    expect((await b.call('message_get', { id: spam.id })).spamVerdict!.reasons[0]).toBe('blocked');

    const events: string[] = [];
    b.on('mh:settings', () => events.push('settings'));
    b.on('mh:changed', () => events.push('changed'));
    expect(await b.call('spam_trust', { id: spam.id })).toBeNull();
    expect(events).toEqual(['settings', 'changed']);
    expect(b.state.settings.blocked).toEqual(['@xn--bnka-53d.example', 'baska@firma.com']);
    expect(roleOf(spam.id)).toBe('inbox');
    // Kabul edilmiş gönderici alan adı engelinden muaftır: uyarı tümüyle kalkar.
    const d = await b.call('message_get', { id: spam.id });
    expect(d).toMatchObject({ spam: false, spamVerdict: null });
    expect((await b.call('messages_list', { query: inbox })).items.find((m) => m.id === spam.id)).toMatchObject({ spam: false });

    // Sonradan engellenirse son karar geçerlidir.
    const r = await b.call('sender_block', { id: spam.id });
    expect(r.ids).toEqual([spam.id]);
    expect(roleOf(spam.id)).toBe('junk');
    expect((await b.call('message_get', { id: spam.id })).spamVerdict).toEqual({ score: 100, reasons: ['blocked'] });
  });

  it('gelen kutusundaki şüpheli postada: taşınmaz, işaret kalkar; elekten kabul de güvenli sayar', async () => {
    const { b, bySubject, roleOf } = setup();
    const m = bySubject('Gönderiniz gümrükte bekliyor');
    expect((await b.call('message_get', { id: m.id })).spamVerdict).toEqual({ score: 50, reasons: ['authFailed'] });
    const box = m.mailboxId;
    await b.call('spam_trust', { id: m.id });
    expect(m.mailboxId).toBe(box);
    expect((await b.call('message_get', { id: m.id })).spamVerdict).toBeNull();
    // Aynı göndericinin öteki postası da (aynı hesapta) işaretsizdir.
    expect(b.state.messages.filter((x) => x.accountId === m.accountId && x.from.email === m.from.email).every((x) => !x.spam)).toBe(true);

    // Elekten kabul edilen gönderici alan adı engelinden muaftır.
    const other = bySubject('Cumartesi kahvaltı?');
    b.state.screened.push(other.id);
    await b.call('screener_decide', { email: other.from.email, accountId: other.accountId, allow: true });
    await b.call('settings_set', { settings: { ...b.state.settings, blocked: ['@eposta.com'] } });
    expect((await b.call('message_get', { id: other.id })).spamVerdict).toBeNull();
    expect(roleOf(other.id)).toBe('inbox');
  });

  it('kendi postamızda kod ve şüpheli işareti yok', async () => {
    const { b, roleOf } = setup();
    const sent = b.state.messages.find((x) => roleOf(x.id) === 'sent')!;
    await b.call('settings_set', { settings: { ...b.state.settings, blocked: ['@ornek.com.tr'] } });
    expect(await b.call('message_get', { id: sent.id })).toMatchObject({ spam: false, spamVerdict: null, otp: null });
  });
});
