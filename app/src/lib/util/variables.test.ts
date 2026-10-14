import { describe, expect, it } from 'vitest';
import { fillVariables, variableKey, variableValues } from './variables';

const kerem = { name: 'Kerem Aydın', email: 'kerem.aydin@ornek.com.tr' };
const me = { name: 'Furkan Yıldırım', email: 'furkan@ornek.com.tr' };
// 13 Ekim 2026 Salı
const now = new Date(2026, 9, 13, 10, 30);

describe('şablon değişkenleri', () => {
  it('Türkçe ve İngilizce adlar doldurulur', () => {
    const out = fillVariables('Merhaba {ad}, ben {benim_adim}. {tam_ad} ({adres}) · {tarih} · {gun}', { recipient: kerem, sender: me, now });
    expect(out).toBe('Merhaba Kerem, ben Furkan Yıldırım. Kerem Aydın (kerem.aydin@ornek.com.tr) · 13 Ekim 2026 · Salı');
    const en = fillVariables('Hi {first_name}, {full_name} <{email}> from {my_name} on {date} ({day})', { recipient: kerem, sender: me, now, locale: 'en-GB' });
    expect(en).toBe('Hi Kerem, Kerem Aydın <kerem.aydin@ornek.com.tr> from Furkan Yıldırım on 13 October 2026 (Tuesday)');
  });

  it('büyük/küçük harf ve Türkçe İ duyarsız', () => {
    expect(fillVariables('{AD} {Tam_Ad} {GÜN}', { recipient: kerem, now })).toBe('Kerem Kerem Aydın Salı');
    expect(variableKey('İSİM')).toBe('ad');
    expect(variableKey('bilinmeyen')).toBeNull();
  });

  it('değeri olmayan değişken boş kalır, metne süslü parantez sızmaz', () => {
    expect(fillVariables('Merhaba {ad}, nasılsın?', { recipient: null })).toBe('Merhaba, nasılsın?');
    expect(fillVariables('Merhaba {ad}', { recipient: { name: null, email: 'x@y.com' } })).toBe('Merhaba');
    expect(fillVariables('Sayın {tam_ad} {ad}!', {})).toBe('Sayın!');
    expect(fillVariables('{adres} adresine', {})).toBe(' adresine');
    expect(fillVariables('x {ad} y', {})).toBe('x y');
    expect(fillVariables('Merhaba {ad},\nselam', { recipient: { name: 'x@y.com', email: 'x@y.com' } })).toBe('Merhaba,\nselam');
  });

  it('bilinmeyen ve bozuk yer tutucular olduğu gibi kalır', () => {
    expect(fillVariables('{json} ve {a b} ve {} ve {{ad}}', { recipient: kerem })).toBe('{json} ve {a b} ve {} ve {Kerem}');
    expect(fillVariables('değişkensiz metin', {})).toBe('değişkensiz metin');
  });

  it('değerler tek yerden gelir', () => {
    const v = variableValues({ recipient: kerem, sender: { name: null, email: 'ben@ornek.com' }, now });
    expect(v.ad).toBe('Kerem');
    expect(v.benim_adim).toBe('');
    expect(v.tarih).toBe('13 Ekim 2026');
  });
});
