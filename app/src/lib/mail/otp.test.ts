import { describe, expect, it } from 'vitest';
import { createDemoData } from '../api/demo-data';
import { groupCode } from './otp';

describe('doğrulama kodu gösterimi', () => {
  it('rakamlı kod öbeklenir, karışık kod olduğu gibi kalır', () => {
    expect(groupCode('482913')).toBe('482 913');
    expect(groupCode('12345678')).toBe('1234 5678');
    expect(groupCode('5821')).toBe('5821');
    expect(groupCode('7319042')).toBe('7319042');
    expect(groupCode('K7P2QX')).toBe('K7P2QX');
  });
});

describe('güven örnekleri (sahte arka uç)', () => {
  it('arşivde okunmuş; izleyici, uyarı ve kod alanları dolu', () => {
    const { messages } = createDemoData(new Date(2026, 9, 13, 10, 0));
    const bySubject = (s: string) => messages.find((m) => m.subject === s)!;
    const news = bySubject('Hafta sonu için beş rota');
    expect([news.trackers, news.cleanedLinks, news.hasRemoteContent]).toEqual([3, 4, true]);
    expect(bySubject('Hesabınız 24 saat içinde askıya alınacak').senderWarning).toBe('nameHasOtherAddress');
    expect(bySubject('Giriş doğrulama kodun').otp).toBe('482913');
    expect(bySubject('Haftalık özet: üç görev tamamlandı').hasRemoteContent).toBe(false);
    const samples = messages.filter((m) => m.trackers || m.senderWarning || m.otp);
    expect(samples).toHaveLength(5);
    for (const m of samples) {
      expect(m.mailboxId.endsWith(':Archive')).toBe(true);
      expect(m.unread).toBe(false);
    }
    // Diğer postalarda alanlar boş.
    expect(messages.filter((m) => !samples.includes(m)).every((m) => m.trackers === 0 && m.otp === null && m.senderWarning === null)).toBe(true);
  });
});
