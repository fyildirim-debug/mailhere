import { describe, expect, it } from 'vitest';
import { avatarTone, baseSubject, dateGroup, fullTime, fileBadge, fileKind, fileSize, initials, linkify, listTime, snoozeOptions } from './format';

// 13 Ekim 2026 Salı 10:00 (yerel saat)
const NOW = new Date(2026, 9, 13, 10, 0).getTime();
const at = (d: number, h: number, m = 0) => new Date(2026, 9, d, h, m).getTime();

describe('tarih grupları', () => {
  it('bugün, dün, bu hafta, daha eski', () => {
    expect(dateGroup(at(13, 9, 42), NOW)).toBe('today');
    expect(dateGroup(at(12, 23, 59), NOW)).toBe('yesterday');
    expect(dateGroup(at(7, 12), NOW)).toBe('week');
    expect(dateGroup(at(6, 12), NOW)).toBe('older');
  });

  it('tam zaman geçmiş ve gelecek', () => {
    const w = { today: 'Bugün', yesterday: 'Dün', tomorrow: 'Yarın' };
    expect(fullTime(at(13, 9, 42), NOW, 'tr-TR', w)).toBe('Bugün 09:42');
    expect(fullTime(at(12, 16, 48), NOW, 'tr-TR', w)).toBe('Dün 16:48');
    expect(fullTime(at(14, 9), NOW, 'tr-TR', w)).toBe('Yarın 09:00');
    expect(fullTime(at(17, 9), NOW, 'tr-TR', w)).toBe('Cumartesi 09:00');
  });

  it('liste zamanı', () => {
    expect(listTime(at(13, 9, 42), NOW, 'tr-TR')).toBe('09:42');
    expect(listTime(at(12, 19, 20), NOW, 'tr-TR')).toBe('19:20');
    expect(listTime(at(12 - 2, 8), NOW, 'tr-TR')).toBe('Cmt');
    expect(listTime(new Date(2025, 0, 5).getTime(), NOW, 'tr-TR')).toBe('05.01.2025');
  });
});

describe('biçimler', () => {
  it('dosya boyutu', () => {
    expect(fileSize(500, 'tr-TR')).toBe('500 B');
    expect(fileSize(253_952, 'tr-TR')).toBe('248 KB');
    expect(fileSize(1_258_291, 'tr-TR')).toBe('1,2 MB');
  });

  it('monogram Türkçe büyük harfle', () => {
    expect(initials({ name: 'Elif Şahin', email: 'e@x.com' })).toBe('EŞ');
    expect(initials({ name: 'ilkay ışık', email: 'i@x.com' })).toBe('İI');
    expect(initials({ name: null, email: 'kerem@ornek.com' })).toBe('KE');
    expect(initials({ name: '"GitHub"', email: 'n@github.com' })).toBe('GH');
    expect(initials({ name: 'ilkay', email: 'i@x.com' })).toBe('İL');
  });

  it('avatar tonu kararlı ve 0–8', () => {
    const a = avatarTone('ayse@ornek.com');
    expect(a).toBe(avatarTone('AYSE@ornek.com'));
    expect(a).toBeGreaterThanOrEqual(0);
    expect(a).toBeLessThan(9);
  });

  it('dosya türü ve rozet', () => {
    expect(fileKind('Butce.xlsx')).toBe('sheet');
    expect(fileKind('a.PDF')).toBe('pdf');
    expect(fileKind('foto', 'image/png')).toBe('image');
    expect(fileBadge('Sunum_ozeti.pdf')).toBe('PDF');
    expect(fileBadge('README')).toBe('DOSYA');
  });

  it('konu öneki sadeleşir', () => {
    expect(baseSubject('Re: RE: Fwd: Bütçe')).toBe('Bütçe');
    expect(baseSubject('YNT: İLT: Plan')).toBe('Plan');
    expect(baseSubject('Rehber')).toBe('Rehber');
  });

  it('bağlantılar ayrılır', () => {
    const parts = linkify('Bak: https://ornek.com/a?b=1. Teşekkürler');
    expect(parts.map((p) => p.href).filter(Boolean)).toEqual(['https://ornek.com/a?b=1']);
    expect(parts.map((p) => p.text).join('')).toBe('Bak: https://ornek.com/a?b=1. Teşekkürler');
  });
});

describe('erteleme seçenekleri', () => {
  it('salı sabahı: akşam, yarın, hafta sonu, pazartesi', () => {
    const o = snoozeOptions(NOW);
    expect(o.map((x) => x.key)).toEqual(['evening', 'tomorrow', 'weekend', 'nextWeek']);
    expect(new Date(o[0].at).getHours()).toBe(18);
    expect(new Date(o[2].at).getDay()).toBe(6);
    expect(new Date(o[3].at).getDay()).toBe(1);
  });

  it('cuma akşamı: bu akşam yok, hafta sonu yarın olduğu için tekrarlanmaz', () => {
    const fri = new Date(2026, 9, 16, 19, 0).getTime();
    expect(snoozeOptions(fri).map((x) => x.key)).toEqual(['tomorrow', 'nextWeek']);
  });
});
