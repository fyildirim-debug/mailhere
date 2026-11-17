import { describe, expect, it } from 'vitest';
import { attachmentWord, invalidRecipients, ownText, sendIssues, splitRecipients } from './precheck';

describe('alıcı parçaları', () => {
  it('virgül, noktalı virgül ve satırla ayrılır', () => {
    expect(splitRecipients(' a@ornek.com, b@ornek.com;c@ornek.com\n d@ornek.com ,, ')).toEqual([
      'a@ornek.com',
      'b@ornek.com',
      'c@ornek.com',
      'd@ornek.com',
    ]);
    expect(splitRecipients('   ')).toEqual([]);
  });

  it('adres olmayan parçalar bulunur, geçerliler sayılmaz', () => {
    expect(invalidRecipients('ayse@ornek.com, kerem, Kerem Aydın <kerem@ornek.com>; ali@ornek')).toEqual(['kerem', 'ali@ornek']);
    expect(invalidRecipients('ayse@ornek.com')).toEqual([]);
    expect(invalidRecipients('')).toEqual([]);
  });
});

describe('yazarın kendi metni', () => {
  it('alıntı satırları ve alıntı başlığı atılır', () => {
    const body = 'Merhaba,\n\nTamamdır.\n\n12 Kas 2026 tarihinde Kerem Aydın şunu yazdı:\n> Rapor ekte.\n> İyi çalışmalar';
    expect(ownText(body).trim()).toBe('Merhaba,\n\nTamamdır.');
  });

  it('araya yazılan yanıt kalır, alıntı gider', () => {
    const body = '> Dosya ekte mi?\nEvet.\n> Teşekkürler';
    expect(ownText(body)).toBe('Evet.');
  });

  it('iletilen postanın ayracından sonrası sayılmaz', () => {
    const body = 'Bilgine.\n\n---------- İletilen posta ----------\nKerem <kerem@ornek.com>\nRapor ekte.';
    expect(ownText(body).trim()).toBe('Bilgine.');
    expect(ownText('Not\n---------- Forwarded message ----------\nSee attached.').trim()).toBe('Not');
  });

  it('iki noktayla biten sıradan satır alıntıdan önce değilse kalır', () => {
    expect(ownText('Liste şöyle:\n- bir\n- iki')).toBe('Liste şöyle:\n- bir\n- iki');
  });
});

describe('ek sözcüğü', () => {
  it('Türkçe sözcükleri bulur ve yazıldığı haliyle döndürür', () => {
    expect(attachmentWord('', 'Raporu ekte bulabilirsin.')).toBe('ekte');
    expect(attachmentWord('', 'Dosyayı ekledim, bakar mısın?')).toBe('ekledim');
    expect(attachmentWord('', 'Sözleşme ilişikte.')).toBe('ilişikte');
    expect(attachmentWord('', 'EKLİ DOSYAYI İNCELEYİN')).toBe('EKLİ');
    expect(attachmentWord('', 'İLİŞİKTE sunulmuştur')).toBe('İLİŞİKTE');
    expect(attachmentWord('', 'Ekteki tabloya göre')).toBe('Ekteki');
  });

  it('İngilizce sözcükleri bulur (büyük I Türkçe küçültmeye takılmaz)', () => {
    expect(attachmentWord('', 'Please find the report attached.')).toBe('attached');
    expect(attachmentWord('', 'See the ATTACHMENT for details')).toBe('ATTACHMENT');
    expect(attachmentWord('', 'ENCLOSING the invoice')).toBe('ENCLOSING');
    expect(attachmentWord('', "I'm attaching the file")).toBe('attaching');
  });

  it('konuda da arar', () => {
    expect(attachmentWord('Fatura ekte', 'Merhaba')).toBe('ekte');
  });

  it('sözcüğün parçası ya da başka anlamlı kardeşi uyarı vermez', () => {
    expect(attachmentWord('', 'Buna ek olarak toplantıyı erteledik.')).toBeNull();
    expect(attachmentWord('', 'Listeye iki madde ekle lütfen, gerekli eklemeleri yaparım.')).toBeNull();
    expect(attachmentWord('', 'Proje çok etkileyici, tekliyor ama çalışıyor.')).toBeNull();
    expect(attachmentWord('', 'Please attach your CV; reattached parts are fine.')).toBeNull();
    expect(attachmentWord('', '')).toBeNull();
  });

  it('alıntılanan önceki ileti sayılmaz', () => {
    const reply = 'Teşekkürler, aldım.\n\n12 Kas 2026 tarihinde Kerem Aydın şunu yazdı:\n> Rapor ekte.\n> See attached.';
    expect(attachmentWord('', reply)).toBeNull();
    const forward = 'Bilgine.\n\n---------- İletilen posta ----------\nKerem\nRapor ekte.';
    expect(attachmentWord('', forward)).toBeNull();
    expect(attachmentWord('', `Güncel hali ekte.\n\n${reply}`)).toBe('ekte');
  });
});

describe('gönderim öncesi sorular', () => {
  const base = { subject: 'Rapor', body: 'Merhaba', hasAttachments: false, subjectIsOwn: true };

  it('sorun yoksa null', () => {
    expect(sendIssues(base)).toBeNull();
    expect(sendIssues({ ...base, body: 'Rapor ekte.', hasAttachments: true })).toBeNull();
  });

  it('boş konu sorulur', () => {
    expect(sendIssues({ ...base, subject: '   ' })).toEqual({ noSubject: true, attachmentWord: null });
  });

  it('ek yokken ek sözcüğü sorulur; ikisi birlikte de gelir', () => {
    expect(sendIssues({ ...base, body: 'Rapor ekte.' })).toEqual({ noSubject: false, attachmentWord: 'ekte' });
    expect(sendIssues({ ...base, subject: '', body: 'See attached' })).toEqual({ noSubject: true, attachmentWord: 'attached' });
  });

  it('yanıtta özgün postadan gelen konu sayılmaz, kullanıcının yazdığı sayılır', () => {
    expect(sendIssues({ ...base, subject: 'Re: Fatura ekte', subjectIsOwn: false })).toBeNull();
    expect(sendIssues({ ...base, subject: 'Re: Fatura ekte', subjectIsOwn: true })).toEqual({ noSubject: false, attachmentWord: 'ekte' });
  });
});
