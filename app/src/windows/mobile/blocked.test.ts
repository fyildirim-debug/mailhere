import { describe, expect, it } from 'vitest';
import { BLOCKED_MAX } from '../../lib/api';
import { checkBlockedEntry } from './blocked';

describe('engellenenlere elle ekleme', () => {
  it('adres kırpılır ve küçültülür, listenin sonuna eklenir', () => {
    const r = checkBlockedEntry(['a@b.com'], '  Reklam@Ornek.COM ');
    expect(r).toEqual({ ok: true, entry: 'reklam@ornek.com', list: ['a@b.com', 'reklam@ornek.com'] });
  });

  it('alan adı girdisi "@" ile başlar', () => {
    const r = checkBlockedEntry([], '@Ornek.com');
    expect(r.ok && r.entry).toBe('@ornek.com');
  });

  it('adres ya da alan adı olmayan girdi reddedilir', () => {
    for (const bad of ['', '   ', 'ornek', 'ornek.com', 'a@', '@', '@ornek', 'a b@c.com', 'a@b.com, c@d.com', 'a@@b.com']) {
      expect(checkBlockedEntry([], bad), bad).toEqual({ ok: false, reason: 'invalid', entry: null });
    }
  });

  it('listede olan girdi (harf farkıyla da) yinelenmez', () => {
    expect(checkBlockedEntry(['a@b.com'], 'A@B.com')).toEqual({ ok: false, reason: 'duplicate', entry: 'a@b.com' });
  });

  it('kendi adresimiz engellenemez; kendi alan adımız engellenebilir', () => {
    expect(checkBlockedEntry([], 'ben@ornek.com', ['Ben@Ornek.com'])).toEqual({ ok: false, reason: 'own', entry: 'ben@ornek.com' });
    expect(checkBlockedEntry([], '@ornek.com', ['ben@ornek.com']).ok).toBe(true);
  });

  it('liste doluyken yeni girdi alınmaz', () => {
    const full = Array.from({ length: BLOCKED_MAX }, (_, i) => `k${i}@ornek.com`);
    expect(checkBlockedEntry(full, 'yeni@ornek.com')).toEqual({ ok: false, reason: 'full', entry: 'yeni@ornek.com' });
    // Dolu listede de yinelenen girdi "zaten var" diye söylenir.
    expect(checkBlockedEntry(full, 'k3@ornek.com').ok).toBe(false);
  });
});
