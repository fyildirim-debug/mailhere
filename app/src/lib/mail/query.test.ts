import { describe, expect, it } from 'vitest';
import { accountScope, accountsFor, completeWith, completing, presetDate, readChips, tokenize, withOp, ymd } from './query';

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
    expect(readChips('fatura')).toEqual({ from: null, after: null, before: null, attach: false, unread: false, account: null });
    const c = readChips('fatura from:ayse has:attachment is:unread sonra:2026-10-01 önce:2026-10-10 hesap:iş');
    expect(c).toEqual({ from: 'ayse', after: '2026-10-01', before: '2026-10-10', attach: true, unread: true, account: 'iş' });
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
