import { afterEach, describe, expect, it } from 'vitest';
import { openExternal, useBackend, type ComposePrefill } from '../api';
import { bindComposeAccount, bindComposeOpener, mailtoPrefill } from './open';

describe('mailto ön dolgusu', () => {
  it('alıcıyı yeni postaya taşır', () => {
    expect(mailtoPrefill('mailto:destek@ornek.com')).toEqual({
      mode: 'new',
      messageId: null,
      to: [{ name: null, email: 'destek@ornek.com' }],
      accountId: null,
    });
  });

  it('birden çok alıcı, yüzde kodlama ve `to` parametresi', () => {
    const p = mailtoPrefill('MAILTO:a@ornek.com,%20b%40ornek.com?to=c@ornek.com,a@ornek.com&subject=Merhaba', 'demo-is');
    expect(p?.to?.map((a) => a.email)).toEqual(['a@ornek.com', 'b@ornek.com', 'c@ornek.com']);
    expect(p?.accountId).toBe('demo-is');
  });

  it('konu, gövde, bilgi ve gizli şimdilik taşınmaz; geçersiz adres atlanır', () => {
    const p = mailtoPrefill('mailto:ayril@tasarimnotlari.co,bozuk?subject=ayril&body=x&cc=k@ornek.com&bcc=g@ornek.com');
    expect(p).toEqual({ mode: 'new', messageId: null, to: [{ name: null, email: 'ayril@tasarimnotlari.co' }], accountId: null });
  });

  it('alıcısız mailto boş yeni posta açar; mailto olmayan adres null', () => {
    expect(mailtoPrefill('mailto:?subject=Merhaba')?.to).toEqual([]);
    expect(mailtoPrefill('https://ornek.com')).toBeNull();
    expect(mailtoPrefill('javascript:alert(1)')).toBeNull();
  });

  it('en çok 20 alıcı', () => {
    const many = Array.from({ length: 30 }, (_, i) => `k${i}@ornek.com`).join(',');
    expect(mailtoPrefill(`mailto:${many}`)?.to?.length).toBe(20);
  });
});

describe('openExternal: mailto Mailhere’de açılır', () => {
  const calls: { name: string; args: unknown }[] = [];
  const backend = {
    call: async (name: string, args?: unknown) => {
      calls.push({ name, args });
      return null as never;
    },
    on: () => () => {},
  };

  afterEach(() => {
    calls.length = 0;
    bindComposeOpener(null);
    bindComposeAccount(null);
  });

  it('masaüstü: işletim sistemine değil, alıcısı dolu yazma penceresine gider', async () => {
    useBackend(backend as never);
    bindComposeAccount(() => 'demo-is');
    await openExternal('mailto:destek@ornek.com?subject=Yard%C4%B1m');
    expect(calls).toEqual([
      {
        name: 'open_compose',
        args: { prefill: { mode: 'new', messageId: null, to: [{ name: null, email: 'destek@ornek.com' }], accountId: 'demo-is' } },
      },
    ]);
    // `link_open` (dış program) çağrılmadı.
    expect(calls.some((c) => c.name === 'link_open')).toBe(false);
  });

  it('mobil: bağlı yazma açıcısı çağrılır, arka uca gidilmez', async () => {
    useBackend(backend as never);
    const opened: ComposePrefill[] = [];
    bindComposeOpener((p) => opened.push(p));
    await openExternal('mailto:ayse.demir@eposta.com');
    expect(opened).toEqual([{ mode: 'new', messageId: null, to: [{ name: null, email: 'ayse.demir@eposta.com' }], accountId: null }]);
    expect(calls).toEqual([]);
  });

  it('desteklenmeyen şema hiçbir şey açmaz', async () => {
    useBackend(backend as never);
    await openExternal('file:///etc/passwd');
    await openExternal('javascript:alert(1)');
    expect(calls).toEqual([]);
  });
});
