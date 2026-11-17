// Engellenenler listesine elle ekleme (Ayarlar › Gizlilik ve güvenlik, mobil):
// yazılan girdi normalleştirilir ve neden eklenemediği ayrı ayrı söylenir.
// Normalleştirme çekirdekle aynıdır (`normalizeBlockedEntry`); burada yalnız
// kullanıcıya gösterilecek ret nedeni seçilir.

import { BLOCKED_MAX, normalizeBlocked, normalizeBlockedEntry } from '../../lib/api';

export type BlockedReject = 'invalid' | 'duplicate' | 'own' | 'full';

export type BlockedCheck = { ok: true; entry: string; list: string[] } | { ok: false; reason: BlockedReject; entry: string | null };

/**
 * `input` listeye eklenebilir mi. Olursa normalleştirilmiş girdi ve yeni liste
 * (sıra korunur, yeni girdi sonda); olmazsa neden. `own`: kendi hesap adreslerimiz
 * (engellenemez; alan adı girdisi serbesttir).
 */
export function checkBlockedEntry(list: string[], input: string, own: string[] = []): BlockedCheck {
  const entry = normalizeBlockedEntry(input);
  if (!entry) return { ok: false, reason: 'invalid', entry: null };
  if (list.includes(entry)) return { ok: false, reason: 'duplicate', entry };
  if (own.some((e) => e.trim().toLowerCase() === entry)) return { ok: false, reason: 'own', entry };
  if (list.length >= BLOCKED_MAX) return { ok: false, reason: 'full', entry };
  return { ok: true, entry, list: normalizeBlocked([...list, entry]) };
}
