// Yazma penceresi rotasındaki önceden doldurulan alıcılar (`?to=Ad <adres>`).
// Arka uç (`windows.rs`) ve sahte arka uç aynı biçimi üretir.

import type { Address } from '../api/types';

/** En çok bu kadar alıcı okunur (arka uçla aynı). */
export const MAX_PREFILL_TO = 20;

const ADDRESS = /^(?:"?([^"<]*?)"?\s*<)?([^\s<>@]+@[^\s<>@]+\.[^\s<>@]+)>?$/;

/** `Ad <adres>` ya da `adres` metnini çözer; geçersizse null. */
export function parseAddress(text: string): Address | null {
  const m = text.trim().match(ADDRESS);
  if (!m) return null;
  return { name: m[1]?.trim() || null, email: m[2] };
}

/** Sorgu dizgisindeki `to` parametrelerinden alıcılar; bozuk olanlar atlanır. */
export function parseRecipients(query: string): Address[] {
  const out: Address[] = [];
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(query);
  } catch {
    return out;
  }
  for (const v of params.getAll('to').slice(0, MAX_PREFILL_TO)) {
    const a = parseAddress(v);
    if (a) out.push(a);
  }
  return out;
}
