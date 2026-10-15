// Mobilde klasöre taşıma: okuma ekranının "Klasöre taşı" alt sayfası ve
// listede "Klasöre taşı" kaydırma eylemi aynı seçenekleri kullanır (hesabın
// klasör ağacı, girintili; bulunduğu klasör ve Taslaklar hariç).

import type { AccountId, Mailbox } from '../../lib/api';
import { flatten, folderLabel, folderTree } from '../../lib/mail/folders';
import type { MenuItem } from '../../lib/ui/Menu.svelte';
import { mail } from '../../lib/stores/mail.svelte';

/** Klasör seçicinin satırları; seçilen klasör `onpick` ile gelir. */
export function moveItems(accountId: AccountId, here: string | null, onpick: (box: Mailbox, label: string) => void): MenuItem[] {
  return flatten(folderTree(mail.mailboxes, accountId, true))
    .filter((n) => n.mailbox.id !== here && n.mailbox.role !== 'drafts')
    .map((n) => ({
      key: n.mailbox.id,
      label: `${'  '.repeat(n.depth)}${folderLabel(n.mailbox)}`,
      icon: 'folder' as const,
      onclick: () => onpick(n.mailbox, folderLabel(n.mailbox)),
    }));
}

/** Hesabın İstenmeyen kutusu (sunucudan bulunan ya da elle eşlenen rol); yoksa undefined. */
export function junkOf(accountId: AccountId): Mailbox | undefined {
  return mail.mailboxes.find((b) => b.accountId === accountId && b.role === 'junk');
}
