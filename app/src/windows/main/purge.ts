// Sil eylemi (masaüstü): Çöp ya da İstenmeyen kutusundaki postada "Kalıcı sil"
// olur ve onay ister (geri alınamaz); başka yerde postayı Çöp'e taşır (geri
// alınabilir). Araç çubuğu, kısayol, bağlam menüsü, toplu panel, odak kipi ve
// palet aynı yoldan geçer.

import type { MessageId } from '../../lib/api';
import { t } from '../../lib/i18n/index.svelte';
import { bulkMove } from '../../lib/stores/bulk.svelte';
import { deleteForever, emptyMailbox, purgeRole } from '../../lib/stores/guard.svelte';
import { mail, moveTo } from '../../lib/stores/mail.svelte';
import { askConfirm } from '../../lib/ui/confirm.svelte';

/** Kutu Çöp ya da İstenmeyen mi (kalıcı silme yalnız buralarda). */
export function purgeBox(mailboxId: string | null | undefined): boolean {
  const role = mail.mailboxes.find((b) => b.id === mailboxId)?.role;
  return role === 'trash' || role === 'junk';
}

/**
 * Bu postalar için "Sil" kalıcı silme mi: hepsi Çöp ya da İstenmeyen kutusunda.
 * Kutusu bilinmeyen posta (listede ve okuma alanında yok) görünüme göre sayılır.
 */
export function purgeable(ids: MessageId[]): boolean {
  if (!ids.length) return false;
  // Liste her değişiklikte yenilenir; okunan postanın kopyası eski kalmış olabilir, o yüzden yalnız listede yoksa sorulur.
  const box = new Map(mail.items.map((m) => [m.id, m.mailboxId]));
  if (mail.detail && !box.has(mail.detail.id)) box.set(mail.detail.id, mail.detail.mailboxId);
  const inView = mail.search == null && purgeRole() != null;
  return ids.every((id) => {
    const b = box.get(id);
    return b == null ? inView : purgeBox(b);
  });
}

/** Kalıcı silme onayı ("Bu işlem geri alınamaz"); `n` silinecek posta sayısı. */
export function askPurge(n: number): Promise<boolean> {
  const one = n === 1;
  return askConfirm({
    title: one ? t('purge.confirmOneTitle') : t('purge.confirmTitle', { n }),
    body: t(one ? 'purge.confirmOneBody' : 'purge.confirmBody'),
    confirmLabel: t('purge.delete'),
    icon: 'trash',
    danger: true,
  });
}

/** Onay sorup postaları kalıcı siler; vazgeçilirse ya da silinemezse `false`. */
export async function purgeConfirmed(ids: MessageId[]): Promise<boolean> {
  if (!ids.length) return false;
  return (await askPurge(ids.length)) ? deleteForever(ids) : false;
}

/**
 * Sil: Çöp / İstenmeyen'deki postaları onayla kalıcı siler, diğerlerini Çöp'e
 * taşır. `bulk` çoklu seçimdir (sayılı bildirim, seçim bırakılır).
 */
export async function removeMessages(ids: MessageId[], opts: { bulk?: boolean } = {}): Promise<void> {
  if (purgeable(ids)) await purgeConfirmed(ids);
  else if (opts.bulk) await bulkMove(ids, 'trash');
  else await moveTo(ids, 'trash');
}

/** Boşaltılacak kutu: görünümün rolü, hesabı ve (ucuzsa) posta sayısı. */
export function emptyTarget(): { role: 'trash' | 'junk'; accountId: string | null; count: number } | null {
  const role = mail.search == null ? purgeRole() : null;
  if (!role) return null;
  const view = mail.view;
  // Klasör görünümü tek hesabın kutusudur; Çöp görünümü seçili hesabın ya da tüm hesaplarındır.
  const own = view.kind === 'mailbox' ? mail.mailboxes.find((b) => b.id === view.id) : undefined;
  const accountId = own ? own.accountId : mail.accountId;
  const boxes = own ? [own] : mail.mailboxes.filter((b) => b.role === role && (!accountId || b.accountId === accountId));
  // Liste sonuna kadar yüklüyse sayı listedendir (kesin); değilse kutuların toplamı.
  const total = boxes.reduce((n, b) => n + b.total, 0);
  return { role, accountId, count: mail.nextCursor ? Math.max(total, mail.items.length) : mail.items.length };
}

/** Onay sorup Çöp ya da İstenmeyen kutusunu boşaltır. */
export async function emptyConfirmed(): Promise<boolean> {
  const target = emptyTarget();
  if (!target) return false;
  const trash = target.role === 'trash';
  const ok = await askConfirm({
    title: target.count ? t(trash ? 'purge.emptyTrashCount' : 'purge.emptyJunkCount', { n: target.count }) : t(trash ? 'purge.emptyTrashTitle' : 'purge.emptyJunkTitle'),
    body: t('purge.emptyBody'),
    confirmLabel: t(trash ? 'purge.emptyTrash' : 'purge.emptyJunk'),
    icon: 'trash',
    danger: true,
  });
  return ok ? emptyMailbox(target.role, target.accountId) : false;
}
