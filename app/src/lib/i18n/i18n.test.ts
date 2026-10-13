import { describe, expect, it } from 'vitest';
import { en } from './en';
import { tr } from './tr';

const holes = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe('dil sözlükleri', () => {
  it('aynı anahtarlar', () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(tr).sort());
  });

  it('aynı yer tutucular', () => {
    for (const k of Object.keys(tr) as (keyof typeof tr)[]) {
      expect(holes(en[k]), k).toEqual(holes(tr[k]));
    }
  });

  it('boş metin yok', () => {
    for (const [k, v] of Object.entries(tr)) expect(v.trim(), k).not.toBe('');
    for (const [k, v] of Object.entries(en)) expect(v.trim(), k).not.toBe('');
  });
});
