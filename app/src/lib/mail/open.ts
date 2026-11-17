// Yazma ekranını açma. Masaüstünde arka uç ayrı pencere açar (`open_compose`);
// mobil kabuk açılışta kendi yığınını bağlar ve yazma ekranını iter (sahte
// arka uçta `open_compose` yeni sekme açar; mobilde kullanılmaz).
//
// Postadaki `mailto:` bağlantıları da buradan geçer: işletim sisteminin
// e-posta programı yerine Mailhere'nin yazma ekranı açılır (`openMailto`).

import { call, type Address, type ComposePrefill } from '../api';
import { MAX_PREFILL_TO, parseAddress } from '../util/recipients';

let opener: ((prefill: ComposePrefill) => void) | null = null;

export function bindComposeOpener(fn: ((prefill: ComposePrefill) => void) | null) {
  opener = fn;
}

export function openCompose(prefill: ComposePrefill) {
  if (opener) opener(prefill);
  else void call('open_compose', { prefill });
}

let currentAccount: (() => string | null) | null = null;

/**
 * Bağlantıdan açılan yeni postanın gönderen hesabını veren işlev (ana pencere:
 * okunan postanın hesabı). Bağlı değilse ya da null dönerse ilk hesap kullanılır.
 */
export function bindComposeAccount(fn: (() => string | null) | null) {
  currentAccount = fn;
}

function decode(part: string): string {
  try {
    return decodeURIComponent(part);
  } catch {
    return part;
  }
}

/**
 * `mailto:` bağlantısından yeni posta ön dolgusu; `mailto:` değilse null.
 *
 * Yalnız alıcılar taşınır: adresin yol kısmı ve `to` parametreleri (virgülle
 * ayrılmış, yüzde kodlu olabilir; geçersiz ve yinelenen adresler atlanır, en
 * çok `MAX_PREFILL_TO`). `subject`, `body`, `cc` ve `bcc` şimdilik taşınmaz:
 * `ComposePrefill` yalnız `to` taşıyor; sözleşme genişlerse burada eklenir.
 */
export function mailtoPrefill(url: string, accountId: string | null = null): ComposePrefill | null {
  const m = url.trim().match(/^mailto:([^?#]*)(?:\?([^#]*))?/i);
  if (!m) return null;
  const parts = m[1].split(',').map(decode);
  try {
    for (const v of new URLSearchParams(m[2] ?? '').getAll('to')) parts.push(...v.split(','));
  } catch {
    /* bozuk sorgu: yalnız yol kısmındaki alıcılar */
  }
  const to: Address[] = [];
  for (const part of parts) {
    const a = parseAddress(part);
    if (a && to.length < MAX_PREFILL_TO && !to.some((x) => x.email.toLowerCase() === a.email.toLowerCase())) to.push(a);
  }
  return { mode: 'new', messageId: null, to, accountId };
}

/** `mailto:` bağlantısını Mailhere'nin yazma ekranında açar; `mailto:` değilse false. */
export function openMailto(url: string): boolean {
  const prefill = mailtoPrefill(url, currentAccount?.() ?? null);
  if (!prefill) return false;
  openCompose(prefill);
  return true;
}
