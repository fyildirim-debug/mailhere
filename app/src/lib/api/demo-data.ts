// Tarayıcı modu, testler ve ekran görüntüleri için örnek veri.
// İçerik onaylı tasarımdaki postalarla aynıdır; gerçek kişi ya da kurum değildir.

import type { Account, Address, Attachment, Category, Mailbox, MailboxRole, MessageDetail } from './types';

export interface DemoData {
  accounts: Account[];
  mailboxes: Mailbox[];
  messages: MessageDetail[];
}

const A_WORK = 'demo-is';
const A_HOME = 'demo-kisisel';
const A_SERVER = 'demo-sunucu';

const ME_WORK: Address = { name: 'Furkan', email: 'furkan@ornek.com.tr' };
const ME_HOME: Address = { name: 'Furkan', email: 'furkan@eposta.com' };

function at(base: Date, dayOffset: number, hh: number, mm: number): number {
  const d = new Date(base);
  d.setDate(d.getDate() + dayOffset);
  d.setHours(hh, mm, 0, 0);
  return d.getTime();
}

function mb(accountId: string, path: string, name: string, role: MailboxRole): Mailbox {
  return { id: `${accountId}:${path}`, accountId, path, name, role, unread: 0, total: 0 };
}

const ROLES: [string, string, MailboxRole][] = [
  ['INBOX', 'Gelen Kutusu', 'inbox'],
  ['Sent', 'Gönderilenler', 'sent'],
  ['Drafts', 'Taslaklar', 'drafts'],
  ['Archive', 'Arşiv', 'archive'],
  ['Trash', 'Çöp', 'trash'],
  ['Junk', 'İstenmeyen', 'junk'],
];

interface Seed {
  key: string;
  account: string;
  role: MailboxRole;
  from: Address;
  to: Address[];
  cc?: Address[];
  subject: string;
  body: string;
  html?: string;
  date: number;
  unread?: boolean;
  starred?: boolean;
  category: Category;
  attachments?: Attachment[];
  thread?: string;
  remote?: boolean;
}

function snippetOf(text: string): string {
  return text.replace(/\s+/g, ' ').trim().slice(0, 200);
}

export function createDemoData(now = new Date()): DemoData {
  const accounts: Account[] = [
    {
      id: A_WORK, name: 'İş', email: ME_WORK.email, displayName: 'Furkan', color: '#4C6FD8', kind: 'imap',
      imap: { host: 'imap.ornek.com.tr', port: 993, security: 'tls', username: ME_WORK.email },
      smtp: { host: 'smtp.ornek.com.tr', port: 465, security: 'tls', username: ME_WORK.email },
      jmapUrl: null, signature: '— Furkan', createdAt: at(now, -30, 9, 0),
    },
    {
      id: A_HOME, name: 'Kişisel', email: ME_HOME.email, displayName: 'Furkan', color: '#2E9A61', kind: 'imap',
      imap: { host: 'imap.eposta.com', port: 993, security: 'tls', username: ME_HOME.email },
      smtp: { host: 'smtp.eposta.com', port: 465, security: 'tls', username: ME_HOME.email },
      jmapUrl: null, signature: null, createdAt: at(now, -30, 9, 5),
    },
    {
      id: A_SERVER, name: 'Sunucu', email: 'posta@alanadi.com', displayName: 'Furkan', color: '#D08A1E', kind: 'jmap',
      imap: null, smtp: null, jmapUrl: 'https://posta.alanadi.com/jmap', signature: null, createdAt: at(now, -30, 9, 10),
    },
  ];

  const mailboxes = accounts.flatMap((a) => ROLES.map(([p, n, r]) => mb(a.id, p, n, r)));

  const kerem: Address = { name: 'Kerem Aydın', email: 'kerem.aydin@ornek.com.tr' };
  const ayse: Address = { name: 'Ayşe Demir', email: 'ayse.demir@eposta.com' };
  const murat: Address = { name: 'Murat Kaya', email: 'murat.kaya@ornek.com.tr' };

  const seeds: Seed[] = [
    {
      key: 'm1a', account: A_WORK, role: 'sent', from: ME_WORK, to: [kerem], subject: 'Q4 bütçe taslağı',
      body: 'Kerem selam, Perşembe toplantısından önce taslağı görebilir miyim?', date: at(now, -1, 17, 20),
      category: 'people', thread: 'q4',
    },
    {
      key: 'm1b', account: A_WORK, role: 'inbox', from: kerem, to: [ME_WORK], subject: 'Re: Q4 bütçe taslağı',
      body: 'Tabii, yarın sabah ilk iş gönderiyorum.', date: at(now, -1, 18, 4), category: 'people', thread: 'q4',
    },
    {
      key: 'm1', account: A_WORK, role: 'inbox', from: kerem, to: [ME_WORK, ayse], subject: 'Re: Q4 bütçe taslağı',
      body:
        'Merhaba,\n\nPerşembe toplantısından önce bütçe taslağının son halini ekledim. Pazarlama kaleminde geçen çeyreğe göre %8\'lik bir düşüş var; gerekçesini toplantıda konuşalım.\n\nSenden ricam altyapı giderlerine bir göz atman. Sunucu yenileme kalemi önceki çeyreğin üzerinde görünüyor; bir kısmını ilk çeyreğe kaydırabiliriz.\n\nSunum özetini de ekledim, on dakikalık bir anlatım için yeterli.\n\nKolay gelsin,\nKerem',
      date: at(now, 0, 9, 42), category: 'people', thread: 'q4',
      attachments: [
        { index: 0, name: 'Butce_Q4_taslak.xlsx', mime: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet', size: 253_952 },
        { index: 1, name: 'Sunum_ozeti.pdf', mime: 'application/pdf', size: 1_258_291 },
      ],
    },
    {
      key: 'm2', account: A_HOME, role: 'inbox', from: ayse, to: [ME_HOME], subject: 'Cumartesi kahvaltı?',
      body:
        'Selam!\n\nCumartesi Moda\'daki yeni kahvaltı yerine gidelim mi? Saat 10 gibi buluşursak kalabalığa kalmayız.\n\nDeniz ve Can da geliyor. Sen de gelirsen dört kişilik masa ayırtırım; akşama kadar haber vermen yeter.\n\nSevgiler,\nAyşe',
      date: at(now, 0, 8, 15), unread: true, category: 'people',
    },
    {
      key: 'm3', account: A_WORK, role: 'inbox', from: { name: 'GitHub', email: 'notifications@github.com' }, to: [ME_WORK],
      subject: '[mailhere] Derleme başarılı · main #128',
      body:
        'mailhere deposunda main dalı için 128 numaralı derleme tamamlandı.\n\nLinux x64 · geçti · 2 dk 41 sn\nWindows x64 · geçti · 3 dk 12 sn\nBirim testleri · 214 / 214\nUçtan uca testler · 38 / 38\n\nPaketler sürüm sayfasına yüklendi. Bu bildirimleri depo ayarlarından yönetebilirsin.',
      date: at(now, 0, 7, 3), category: 'notifications',
    },
    {
      key: 'm4', account: A_HOME, role: 'inbox', from: { name: 'Derin Kargo', email: 'bilgi@derinkargo.com.tr' }, to: [ME_HOME],
      subject: 'Gönderiniz dağıtıma çıktı',
      body:
        'Merhaba,\n\n4021 8833 17 numaralı gönderiniz dağıtıma çıktı. Kurye bugün 14:00 ile 18:00 arasında adresinize uğrayacak.\n\nEvde olmayacaksanız teslimatı en yakın şubeye yönlendirebilirsiniz.\n\nDerin Kargo',
      date: at(now, 0, 6, 30), unread: true, category: 'notifications',
    },
    {
      key: 'm5', account: A_HOME, role: 'inbox', from: { name: 'Mimar Haftalık', email: 'bulten@mimarhaftalik.com' }, to: [ME_HOME],
      subject: 'Sayı 142 · Sakin arayüzler üzerine',
      body:
        'Bu hafta üç yazı seçtik.\n\nBoşluğun sesi — Ekranda hiçbir şey olmayan alanlar neden en çok şeyi anlatır?\n\nAz renkle hiyerarşi — Tek vurgu rengiyle yön bulmayı kolaylaştıran yedi örnek.\n\nYavaş teknoloji — Bildirimleri azaltmak odaklanmayı gerçekten artırıyor mu?\n\nİyi okumalar,\nMimar Haftalık',
      html:
        '<div style="max-width:560px;margin:0 auto;font-family:Georgia,serif;color:#222"><img data-mh-src="https://mimarhaftalik.com/kapak-142.jpg" alt="Sayı 142 kapak" width="560" height="220" style="display:block;width:100%;height:auto;background:#eee"><h1 style="font-size:26px;margin:24px 0 8px">Sakin arayüzler üzerine</h1><p>Bu hafta üç yazı seçtik.</p><p><b>Boşluğun sesi</b> — Ekranda hiçbir şey olmayan alanlar neden en çok şeyi anlatır?</p><p><b>Az renkle hiyerarşi</b> — Tek vurgu rengiyle yön bulmayı kolaylaştıran yedi örnek.</p><p><b>Yavaş teknoloji</b> — Bildirimleri azaltmak odaklanmayı gerçekten artırıyor mu?</p><p><a href="https://mimarhaftalik.com/142" target="_blank" rel="noopener noreferrer">Sayının tamamını oku</a></p><img data-mh-src="https://izleme.mimarhaftalik.com/p.gif" width="1" height="1" alt=""></div>',
      date: at(now, -1, 19, 20), unread: true, category: 'newsletters', remote: true,
    },
    {
      key: 'm6', account: A_WORK, role: 'inbox', from: { name: 'Elif Şahin', email: 'elif.sahin@hukukbiz.com.tr' }, to: [ME_WORK],
      subject: 'Sözleşme imzaya hazır',
      body:
        'Merhaba,\n\nKonuştuğumuz son düzeltmeleri işledim. 4. maddedeki hizmet süresi 24 aya çekildi, fesih bildirimi 30 güne indi.\n\nUygunsa e-imza bağlantısını yarın sabah gönderebilirim.\n\nSaygılarımla,\nAv. Elif Şahin',
      date: at(now, -1, 16, 48), starred: true, category: 'people',
      attachments: [{ index: 0, name: 'Sozlesme_v3.pdf', mime: 'application/pdf', size: 395_264 }],
    },
    {
      key: 'm7', account: A_HOME, role: 'inbox', from: { name: 'Bulut Fatura', email: 'fatura@bulutfatura.com.tr' }, to: [ME_HOME],
      subject: 'Ekim faturanız hazır',
      body:
        'Sayın müşterimiz,\n\nEkim dönemine ait faturanız oluşturuldu.\n\nTutar: 1.240,00 ₺\nSon ödeme: 24 Ekim\n\nOtomatik ödeme talimatınız varsa ek bir işlem yapmanıza gerek yok.',
      date: at(now, -1, 11, 2), category: 'notifications',
      attachments: [{ index: 0, name: 'Fatura_Ekim.pdf', mime: 'application/pdf', size: 94_208 }],
    },
    {
      key: 'm8', account: A_HOME, role: 'inbox', from: { name: 'Tasarım Notları', email: 'merhaba@tasarimnotlari.co' }, to: [ME_HOME],
      subject: 'Renk, ışık ve odak',
      body:
        'Merhaba,\n\nUzun ekran saatlerinde gözü yoran şey çoğu zaman parlaklık değil, karşıtlığın dağınıklığıdır.\n\nBu sayıda beş küçük karar var: zemin tonunu kırmak, tek vurgu rengi seçmek, satır aralığını açmak, hareketi azaltmak ve bildirimleri gruplamak.\n\nGörüşmek üzere,\nTasarım Notları',
      date: at(now, -3, 8, 0), category: 'newsletters', remote: true,
    },
    {
      key: 'm9a', account: A_WORK, role: 'sent', from: ME_WORK, to: [murat], subject: 'Sunucu taşıma planı',
      body: 'Taşıma için cumartesi gece uygun mu?', date: at(now, -3, 10, 30), category: 'people', thread: 'tasima',
    },
    {
      key: 'm9', account: A_WORK, role: 'inbox', from: murat, to: [ME_WORK, kerem], subject: 'Re: Sunucu taşıma planı',
      body:
        'Selam,\n\nCumartesi gece 02:00 bakım penceresini onayladım. Yedekleri cuma akşamı alalım, taşımadan sonra iki saat gözlemde kalalım.\n\nGeri dönüş planı ekteki belgede; bir göz atarsan sevinirim.\n\nMurat',
      date: at(now, -3, 14, 12), category: 'people', thread: 'tasima',
      attachments: [{ index: 0, name: 'Geri_donus_plani.pdf', mime: 'application/pdf', size: 215_040 }],
    },
  ];

  const messages: MessageDetail[] = seeds.map((s, i) => {
    const id = i + 1;
    const html = s.html ?? null;
    return {
      id,
      accountId: s.account,
      mailboxId: `${s.account}:${ROLES.find((r) => r[2] === s.role)![0]}`,
      threadId: s.thread ?? s.key,
      from: s.from,
      to: s.to,
      subject: s.subject,
      snippet: snippetOf(s.body),
      date: s.date,
      unread: !!s.unread,
      starred: !!s.starred,
      hasAttachments: !!s.attachments?.length,
      category: s.category,
      snoozedUntil: null,
      cc: s.cc ?? [],
      replyTo: [],
      messageIdHeader: `<${s.key}@mailhere.demo>`,
      references: [],
      bodyText: s.body,
      bodyHtml: html,
      hasRemoteContent: !!s.remote,
      remoteAllowed: false,
      attachments: s.attachments ?? [],
      thread: [],
    };
  });

  return { accounts, mailboxes, messages };
}
