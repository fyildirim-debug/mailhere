import { describe, expect, it } from 'vitest';
import { MAX_PREFILL_TO, parseAddress, parseRecipients } from './recipients';

describe('rota alıcıları', () => {
  it('ad ve adres çözülür', () => {
    expect(parseAddress('Ayşe Demir <ayse@ornek.com>')).toEqual({ name: 'Ayşe Demir', email: 'ayse@ornek.com' });
    expect(parseAddress('"Kerem" <kerem@ornek.com.tr>')).toEqual({ name: 'Kerem', email: 'kerem@ornek.com.tr' });
    expect(parseAddress(' murat@ornek.com.tr ')).toEqual({ name: null, email: 'murat@ornek.com.tr' });
    expect(parseAddress('bozuk adres')).toBeNull();
    expect(parseAddress('')).toBeNull();
  });

  it('sorgudaki to parametreleri okunur, bozuklar atlanır, sayı sınırlı', () => {
    const q = new URLSearchParams();
    q.append('to', 'Ayşe Demir <ayse@ornek.com>');
    q.append('to', 'yok');
    q.append('to', 'k@o.com');
    expect(parseRecipients(q.toString())).toEqual([
      { name: 'Ayşe Demir', email: 'ayse@ornek.com' },
      { name: null, email: 'k@o.com' },
    ]);
    expect(parseRecipients('')).toEqual([]);
    const many = new URLSearchParams();
    for (let i = 0; i < 40; i++) many.append('to', `k${i}@o.com`);
    expect(parseRecipients(many.toString())).toHaveLength(MAX_PREFILL_TO);
  });
});
