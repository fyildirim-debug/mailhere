// Hesap ayarlarının ortak yardımcıları.

import type { Account, AccountUpdate } from '../../../lib/api';

/** Hesabın güncel düzenlenebilir alanları (tamamı yeniden yazılır). */
export function accountUpdate(a: Account): AccountUpdate {
  return {
    name: a.name,
    displayName: a.displayName,
    color: a.color,
    notify: a.notify,
    signatures: a.signatures.map((s) => ({ ...s })),
    signatureNew: a.signatureNew,
    signatureReply: a.signatureReply,
  };
}

/** Hesap renkleri kullanıcı verisidir (kenar çubuğu noktası); jeton değildir. */
export const PALETTE = ['#4C6FD8', '#2E9A61', '#D08A1E', '#9A5BD6', '#D0566E', '#2A9BA8'];
