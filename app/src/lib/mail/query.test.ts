import { describe, expect, it } from 'vitest';
import { accountScope, accountsFor, completeWith, completing, presetDate, readChips, sizeLabel, tokenize, withOp, ymd } from './query';

const accounts = [
  { id: 'is', name: 'İş', email: 'furkan@ornek.com.tr' },
  { id: 'ev', name: 'Kişisel', email: 'furkan@eposta.com' },
];

describe('arama sorgusu', () => {
  it('parçalar: tırnaklı değer tek parça', () => {
    expect(tokenize('fatura from:"Ayşe Demir" has:ek')).toEqual(['fatura', 'from:"Ayşe Demir"', 'has:ek']);
    expect(tokenize('  ')).toEqual([]);
  });

  it('çipler metinden okunur; eşanlamlı işleç adları tanınır', () => {
    const none = { from: null, after: null, before: null, attach: false, unread: false, account: null, code: false, spam: false, read: false, larger: null, smaller: null };
    expect(readChips('fatura')).toEqual(none);
    const c = readChips('fatura from:ayse has:attachment is:unread sonra:2026-10-01 önce:2026-10-10 hesap:iş');
    expect(c).toEqual({ ...none, from: 'ayse', after: '2026-10-01', before: '2026-10-10', attach: true, unread: true, account: 'iş' });
    // has:/is: başka değerle çip değildir.
    expect(readChips('is:yıldızlı').unread).toBe(false);
  });

  it('işleç yazılır, değiştirilir ve kaldırılır; serbest metin ve diğerleri yerinde kalır', () => {
    let q = withOp('fatura', 'attach', true);
    expect(q).toBe('fatura has:ek');
    q = withOp(q, 'unread', true);
    expect(q).toBe('fatura has:ek is:okunmamış');
    q = withOp(q, 'from', 'ayse@eposta.com');
    q = withOp(q, 'from', 'kerem@ornek.com.tr');
    expect(q).toBe('fatura has:ek is:okunmamış from:kerem@ornek.com.tr');
    q = withOp(q, 'attach', false);
    expect(q).toBe('fatura is:okunmamış from:kerem@ornek.com.tr');
    expect(withOp('x from:"Ayşe Demir"', 'from', 'Elif Şahin')).toBe('x from:"Elif Şahin"');
    expect(withOp('x after:2026-01-01', 'after', null)).toBe('x');
  });

  it('tamamlama: son parça işleçse türü ve değeri', () => {
    expect(completing('fatura from:')).toEqual({ kind: 'from', value: '' });
    expect(completing('from:ay')).toEqual({ kind: 'from', value: 'ay' });
    expect(completing('after:')).toEqual({ kind: 'after', value: '' });
    expect(completing('önce:2026')).toEqual({ kind: 'before', value: '2026' });
    expect(completing('hesap:i')).toEqual({ kind: 'account', value: 'i' });
    expect(completing('from:ay ')).toBeNull();
    expect(completing('has:ek')).toBeNull();
    expect(completing('fatura')).toBeNull();
    expect(completeWith('fatura from:ay', 'from', 'ayse@eposta.com')).toBe('fatura from:ayse@eposta.com ');
    expect(completeWith('sonra:', 'after', '2026-10-15')).toBe('after:2026-10-15 ');
  });

  it('0.11 işleçleri: kod, şüpheli, okunmuş, boyut ve Türkçe adlar', () => {
    const c = readChips('has:kod is:şüpheli is:okunmuş büyük:5mb küçük:100kb kimden:ayse');
    expect(c).toMatchObject({ code: true, spam: true, read: true, larger: '5mb', smaller: '100kb', from: 'ayse', attach: false, unread: false });
    expect(readChips('has:code is:spam is:read larger:1mb smaller:2kb')).toMatchObject({ code: true, spam: true, read: true, larger: '1mb', smaller: '2kb' });
    // has:ek ile has:kod ayrı çiplerdir; biri ötekini etkilemez.
    expect(withOp('has:ek', 'code', true)).toBe('has:ek has:kod');
    expect(withOp('has:ek has:kod', 'attach', false)).toBe('has:kod');
    expect(withOp('fatura', 'spam', true)).toBe('fatura is:şüpheli');
    // Okunmuş ve okunmamış birlikte olmaz: yeni yazılan eskisini düşürür; kaldırmak ötekine dokunmaz.
    expect(withOp('fatura is:okunmamış', 'read', true)).toBe('fatura is:okunmuş');
    expect(withOp('fatura is:read', 'unread', true)).toBe('fatura is:okunmamış');
    expect(withOp('fatura is:okunmamış', 'read', false)).toBe('fatura is:okunmamış');
    // Boyut ve gönderen Türkçe adla yazılmışsa da çip onu değiştirir.
    expect(withOp('büyük:1mb rapor', 'larger', '5mb')).toBe('rapor larger:5mb');
    expect(withOp('kimden:ayse rapor', 'from', 'kerem@ornek.com.tr')).toBe('rapor from:kerem@ornek.com.tr');
    expect(withOp('rapor larger:5mb', 'larger', null)).toBe('rapor');
  });

  it('tamamlama: kimden / kime / cc kişi, larger / smaller boyut', () => {
    expect(completing('kimden:ay')).toEqual({ kind: 'from', value: 'ay' });
    expect(completing('rapor kime:')).toEqual({ kind: 'to', value: '' });
    expect(completing('to:me')).toEqual({ kind: 'to', value: 'me' });
    expect(completing('cc:')).toEqual({ kind: 'cc', value: '' });
    expect(completing('büyük:')).toEqual({ kind: 'larger', value: '' });
    expect(completing('smaller:1')).toEqual({ kind: 'smaller', value: '1' });
    expect(completeWith('rapor kime:me', 'to', 'mert@ornek.com.tr')).toBe('rapor to:mert@ornek.com.tr ');
    expect(completeWith('cc:', 'cc', 'ayse@eposta.com')).toBe('cc:ayse@eposta.com ');
    expect(completeWith('büyük:', 'larger', '5mb')).toBe('larger:5mb ');
  });

  it('boyut değeri okunur yazılır', () => {
    expect(sizeLabel('5mb')).toBe('5 MB');
    expect(sizeLabel('100KB')).toBe('100 KB');
    expect(sizeLabel('2048')).toBe('2048 B');
    expect(sizeLabel('büyük')).toBe('büyük');
  });

  it('hazır tarihler yerel günün başı', () => {
    // 15 Ekim 2026 perşembe, öğleden sonra.
    const now = new Date(2026, 9, 15, 15, 30).getTime();
    expect(ymd(now)).toBe('2026-10-15');
    expect(presetDate('today', now)).toBe('2026-10-15');
    expect(presetDate('week', now)).toBe('2026-10-12');
    expect(presetDate('month', now)).toBe('2026-10-01');
    expect(presetDate('last30', now)).toBe('2026-09-15');
    // Pazar günü haftanın pazartesisi altı gün öncedir.
    expect(presetDate('week', new Date(2026, 9, 18, 9).getTime())).toBe('2026-10-12');
  });

  it('hesap işleci arayüzde çözülür', () => {
    expect(accountsFor('furkan@eposta.com', accounts)).toEqual(['ev']);
    expect(accountsFor('İŞ', accounts)).toEqual(['is']);
    expect(accountsFor('kiş', accounts)).toEqual(['ev']);
    expect(accountsFor('ornek', accounts)).toEqual(['is']);
    expect(accountsFor('furkan', accounts)).toEqual(['is', 'ev']);
    expect(accountsFor('yok', accounts)).toEqual([]);

    expect(accountScope('fatura has:ek', accounts)).toEqual({ query: 'fatura has:ek', accountIds: null });
    expect(accountScope('fatura hesap:iş', accounts)).toEqual({ query: 'fatura', accountIds: ['is'] });
    // Yalnız hesap işleci: arka uca bütün postalara uyan işleç gider.
    expect(accountScope('account:furkan@eposta.com', accounts)).toEqual({ query: 'after:1970-01-02', accountIds: ['ev'] });
    expect(accountScope('x hesap:yok', accounts)).toEqual({ query: 'x', accountIds: [] });
  });
});
