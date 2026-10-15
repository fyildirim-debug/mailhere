import { describe, expect, it } from 'vitest';
import type { MessageDetail } from '../api/types';
import { escapeHtml, printDocument } from './print';

const labels = { from: 'Kimden', to: 'Kime', cc: 'Bilgi', date: 'Tarih', attachments: 'Ekler' };

const detail: MessageDetail = {
  id: 1, accountId: 'a', mailboxId: 'a:INBOX', threadId: 't', from: { name: 'Kerem <script>', email: 'kerem@ornek.com.tr' },
  to: [{ name: null, email: 'furkan@ornek.com.tr' }], cc: [{ name: 'Ayşe', email: 'ayse@eposta.com' }], bcc: [], replyTo: [],
  subject: 'Bütçe & plan <2026>', snippet: 'özet', date: 0, unread: false, starred: false, hasAttachments: true, category: 'people',
  snoozedUntil: null, messageIdHeader: null, references: [], bodyText: 'Merhaba\n\n> alıntı\n> satır\n\nSon', bodyHtml: null,
  hasRemoteContent: false, remoteAllowed: false, attachments: [{ index: 0, name: 'Butce.xlsx', mime: 'x', size: 2048 }], thread: [],
  unsubscribe: null, invite: null, followUp: null, pinned: false, tags: [], trackers: 0, cleanedLinks: 0, senderWarning: null, otp: null,
};

const input = { detail, labels, when: 'Bugün 09:42', fileSize: (n: number) => `${n} B` };

describe('yazdırma belgesi', () => {
  it('başlıklar kaçışlanır, metin paragraflara bölünür, ekler listelenir', () => {
    const html = printDocument(input);
    expect(html).toContain('<h1>Bütçe &amp; plan &lt;2026&gt;</h1>');
    expect(html).toContain('Kerem &lt;script&gt; &lt;kerem@ornek.com.tr&gt;');
    expect(html).not.toContain('<script>');
    expect(html).toContain('<th>Bilgi</th><td>Ayşe &lt;ayse@eposta.com&gt;</td>');
    expect(html).toContain('<p class="quote">&gt; alıntı\n&gt; satır</p>');
    expect(html).toContain('<li>Butce.xlsx <span class="size">(2048 B)</span></li>');
    expect(html).toContain('Bugün 09:42');
  });

  it('uzak içerik yalnız izinliyse CSP ile açılır; HTML gövde olduğu gibi girer', () => {
    const blocked = printDocument(input);
    expect(blocked).toContain("img-src data: cid:;");
    expect(blocked).toContain("default-src 'none'");
    const allowed = printDocument({ ...input, detail: { ...detail, remoteAllowed: true, bodyHtml: '<p>Temiz <b>HTML</b></p>', attachments: [] } });
    expect(allowed).toContain('img-src data: cid: https: http:;');
    expect(allowed).toContain('<hr><p>Temiz <b>HTML</b></p></body>');
    expect(allowed).not.toContain('Ekler');
  });

  it('kaçışlama', () => {
    expect(escapeHtml(`<a href="x">'&'</a>`)).toBe('&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;');
  });
});
