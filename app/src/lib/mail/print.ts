// Yazdırma: başlıklar ve gövde, uygulama arayüzünden yalıtılmış gizli bir
// çerçevede kurulur ve yalnız o çerçeve yazdırılır. Gövde HTML'i Rust'ta
// temizlenmiş haldedir; çerçeve betiksizdir, CSP uzak içeriği izin
// durumuna göre sınırlar.

import type { Address, MessageDetail } from '../api/types';
import { displayName } from '../util/format';

export interface PrintLabels {
  from: string;
  to: string;
  cc: string;
  date: string;
  attachments: string;
}

export interface PrintInput {
  detail: MessageDetail;
  labels: PrintLabels;
  /** Biçimlenmiş tarih. */
  when: string;
  /** Ek boyutu biçimleyicisi. */
  fileSize: (bytes: number) => string;
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c);
}

function addressList(list: Address[]): string {
  return list.map((a) => (a.name ? `${escapeHtml(displayName(a))} &lt;${escapeHtml(a.email)}&gt;` : escapeHtml(a.email))).join(', ');
}

/** Düz metni paragraflara böler (alıntı satırları ayrı sınıfla). */
function textBody(text: string): string {
  return text
    .replace(/\r\n/g, '\n')
    .split(/\n{2,}/)
    .map((p) => p.replace(/\s+$/, ''))
    .filter((p) => p.trim().length > 0)
    .map((p) => `<p${p.split('\n').every((l) => l.startsWith('>')) ? ' class="quote"' : ''}>${escapeHtml(p)}</p>`)
    .join('');
}

/** Yazdırma belgesi (çerçevenin `srcdoc` içeriği). */
export function printDocument({ detail: d, labels, when, fileSize }: PrintInput): string {
  const remote = d.remoteAllowed ? ' https: http:' : '';
  const csp = `default-src 'none'; img-src data: cid:${remote}; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'`;
  const rows: [string, string][] = [
    [labels.from, addressList([d.from])],
    [labels.to, addressList(d.to)],
    ...(d.cc.length ? ([[labels.cc, addressList(d.cc)]] as [string, string][]) : []),
    [labels.date, escapeHtml(when)],
  ];
  const header = rows.map(([k, v]) => `<tr><th>${escapeHtml(k)}</th><td>${v}</td></tr>`).join('');
  const body = d.bodyHtml != null ? d.bodyHtml : textBody(d.bodyText ?? d.snippet);
  const atts = d.attachments.length
    ? `<section class="atts"><h2>${escapeHtml(labels.attachments)}</h2><ul>${d.attachments
        .map((a) => `<li>${escapeHtml(a.name)} <span class="size">(${escapeHtml(fileSize(a.size))})</span></li>`)
        .join('')}</ul></section>`
    : '';
  return (
    `<!doctype html><html><head><meta charset="utf-8">` +
    `<meta http-equiv="Content-Security-Policy" content="${csp}">` +
    `<meta name="referrer" content="no-referrer">` +
    `<title>${escapeHtml(d.subject || '—')}</title>` +
    `<style>@page{margin:18mm}html,body{margin:0;background:#fff;color:#000}body{padding:0 4mm;font:12pt/1.5 ui-sans-serif,system-ui,sans-serif;word-wrap:break-word;overflow-wrap:anywhere}` +
    `h1{margin:0 0 10pt;font-size:18pt;line-height:1.25}table.head{border-collapse:collapse;margin:0 0 12pt;font-size:10.5pt}table.head th{text-align:left;padding:2pt 12pt 2pt 0;font-weight:600;vertical-align:top;white-space:nowrap}table.head td{padding:2pt 0}` +
    `hr{border:0;border-top:1px solid #999;margin:0 0 12pt}img{max-width:100%;height:auto}table{max-width:100%!important}pre{white-space:pre-wrap}a{color:#000}` +
    `.quote{margin-left:4pt;padding-left:8pt;border-left:1px solid #999;color:#444}p{margin:0 0 9pt;white-space:pre-line}` +
    `.atts{margin-top:14pt;padding-top:8pt;border-top:1px solid #999}.atts h2{margin:0 0 4pt;font-size:10.5pt}.atts ul{margin:0;padding-left:16pt}.size{color:#444}</style>` +
    `</head><body><h1>${escapeHtml(d.subject || '—')}</h1><table class="head">${header}</table><hr>${body}${atts}</body></html>`
  );
}

/** Yazdırma çerçevesinin başlığı (testler ve erişilebilirlik için). */
export const PRINT_FRAME_TITLE = 'yazdır';

/**
 * Belgeyi gizli, kum havuzlu bir çerçeveye yükler ve yalnız onu yazdırır.
 * Çerçeve yazdırma bitince (ya da bir dakika sonra) kaldırılır.
 */
export function printInFrame(srcdoc: string, doc: Document = document): Promise<void> {
  return new Promise((resolve, reject) => {
    const frame = doc.createElement('iframe');
    frame.title = PRINT_FRAME_TITLE;
    // `allow-modals` olmadan kum havuzundaki çerçeve yazdırma penceresi açamaz.
    frame.setAttribute('sandbox', 'allow-same-origin allow-modals');
    frame.setAttribute('referrerpolicy', 'no-referrer');
    frame.setAttribute('aria-hidden', 'true');
    frame.style.cssText = 'position:fixed;left:-10000px;top:0;width:794px;height:1123px;border:0;opacity:0;pointer-events:none';
    let done = false;
    const finish = (err?: unknown) => {
      if (done) return;
      done = true;
      frame.remove();
      if (err) reject(err);
      else resolve();
    };
    frame.addEventListener('load', () => {
      const win = frame.contentWindow;
      if (!win) return finish(new Error('frame'));
      win.addEventListener('afterprint', () => finish());
      // Görseller yüklensin diye kısa bekleme; yazdırma penceresi kapanınca çerçeve gider.
      setTimeout(() => {
        try {
          win.focus();
          win.print();
        } catch (e) {
          return finish(e);
        }
        setTimeout(() => finish(), 60_000);
      }, 250);
    });
    frame.srcdoc = srcdoc;
    doc.body.appendChild(frame);
  });
}
