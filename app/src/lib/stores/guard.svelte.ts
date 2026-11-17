// İstenmeyen koruması, doğrulama kodu, kalıcı silme ve daha eski postalar: masaüstü
// ve mobil arayüzün ortak eylemleri. Bileşenler `call` yerine bunları kullanır ki
// toast metni, geri alma ve liste yenileme iki kabukta aynı olsun.

import { untrack } from 'svelte';
import { call, toAppError, type MailboxRole, type MessageId, type View } from '../api';
import { t } from '../i18n/index.svelte';
import { copyText } from '../mail/otp';
import { showToast } from '../ui/toast.svelte';
import { isLockedError } from './lock.svelte';
import { loadMore, mail, refreshCounts, refreshList, restoreAfterUndo, select, setUnread, takeOut } from './mail.svelte';
import { settings, updateSettings } from './settings.svelte';

function errorToast(e: unknown) {
  if (isLockedError(e)) return;
  showToast({ text: t('toast.error', { message: toAppError(e).message }) });
}

/** Satırdaki kod çipi yalnız taze postada görünür: kodların ömrü kısadır. */
export const OTP_CHIP_MS = 60 * 60_000;

/** Posta kod çipi gösterecek kadar yeni mi (son 60 dakika). */
export function otpFresh(date: number, now = Date.now()): boolean {
  return now - date <= OTP_CHIP_MS && date - now <= 5 * 60_000;
}

/**
 * Postanın göndericisini engeller (`sender_block`): adres engellenenlere girer, gelen
 * kutusundaki postaları İstenmeyen'e taşınır. Toast "Geri al" ile ayar ve postalar döner.
 */
export async function blockSenderOf(id: MessageId): Promise<boolean> {
  const selected = mail.selectedId;
  const had = [...settings.value.blocked];
  try {
    const r = await call('sender_block', { id });
    if (r.ids.length && mail.view.kind === 'inbox') takeOut(r.ids);
    // Okunan posta yerinde kaldıysa (arşiv, arama sonucu) yeniden yüklenir: uyarı "engelledin" der.
    void refreshList().then(() => {
      if (selected != null && mail.selectedId === selected) void select(selected, { markRead: false });
    });
    void refreshCounts();
    showToast({
      text: r.moved ? t('block.doneMoved', { email: r.address, n: r.moved }) : t('block.done', { email: r.address }),
      action: {
        label: t('act.undo'),
        run: async () => {
          // Engel bizden önce de varsa listeye dokunulmaz; yalnız taşınan postalar döner.
          if (!had.includes(r.address)) await updateSettings({ blocked: settings.value.blocked.filter((e) => e !== r.address) });
          if (r.ids.length) await call('messages_move', { ids: r.ids, target: 'inbox' });
          await restoreAfterUndo(selected);
        },
      },
    });
    return true;
  } catch (e) {
    errorToast(e);
    return false;
  }
}

/** "Güvenli" / "İstenmeyen değil" (`spam_trust`): uyarı kalkar, posta İstenmeyen'deyse gelen kutusuna döner. */
export async function trustSenderOf(id: MessageId): Promise<boolean> {
  try {
    const boxId = mail.detail?.id === id ? mail.detail.mailboxId : mail.items.find((m) => m.id === id)?.mailboxId;
    const inJunk = mail.mailboxes.find((b) => b.id === boxId)?.role === 'junk';
    await call('spam_trust', { id });
    showToast({ text: t(inJunk ? 'spam.trustedMoved' : 'spam.trusted') });
    await refreshList();
    if (mail.selectedId === id) void select(id, { markRead: false });
    void refreshCounts();
    return true;
  } catch (e) {
    errorToast(e);
    return false;
  }
}

/** Kodu panoya yazar; "kopyalayınca okundu say" açıksa okunmamış postayı okundu yapar. */
export async function copyCode(id: MessageId, code: string, unread: boolean): Promise<boolean> {
  const ok = await copyText(code);
  showToast({ text: ok ? t('read.otpCopied') : t('read.otpCopyFailed') });
  if (ok && unread && settings.value.otpMarkRead) void setUnread([id], false, { quiet: true });
  return ok;
}

/** Görünüm Çöp ya da İstenmeyen kutusu mu (kalıcı silme yalnız buralarda). */
export function purgeRole(view: View = mail.view): 'trash' | 'junk' | null {
  if (view.kind === 'trash') return 'trash';
  if (view.kind !== 'mailbox') return null;
  const role: MailboxRole | undefined = mail.mailboxes.find((b) => b.id === view.id)?.role;
  return role === 'trash' || role === 'junk' ? role : null;
}

/** Postaları kalıcı siler (`messages_delete`). Onayı çağıran sorar; geri alınamaz. */
export async function deleteForever(ids: MessageId[]): Promise<boolean> {
  if (!ids.length) return false;
  try {
    await call('messages_delete', { ids });
    takeOut(ids);
    void refreshCounts();
    showToast({ text: t(ids.length === 1 ? 'purge.doneOne' : 'purge.done', { n: ids.length }) });
    return true;
  } catch (e) {
    errorToast(e);
    void refreshList();
    return false;
  }
}

/**
 * Boşaltılacak kutunun hesabı: klasör görünümünde o klasörün hesabı (yalnız görünen
 * kutu boşalsın), diğer görünümlerde hesap kapsamı (null: tüm hesaplar).
 */
function emptyScope(): string | null {
  const view = mail.view;
  const box = view.kind === 'mailbox' ? mail.mailboxes.find((b) => b.id === view.id) : undefined;
  return box ? box.accountId : mail.accountId;
}

/**
 * Çöp ya da İstenmeyen kutusunu boşaltır (`mailbox_empty`). Onayı çağıran sorar.
 * Hesap verilmezse açık görünümün kapsamı kullanılır (`emptyScope`).
 */
export async function emptyMailbox(role: 'trash' | 'junk', accountId: string | null = emptyScope()): Promise<boolean> {
  try {
    const n = await call('mailbox_empty', { accountId, role });
    showToast({ text: n ? t(role === 'trash' ? 'purge.emptiedTrash' : 'purge.emptiedJunk', { n }) : t('purge.nothing') });
    await refreshList(false);
    void refreshCounts();
    return true;
  } catch (e) {
    errorToast(e);
    return false;
  }
}

/** "Daha eski postalar" durumu: görünüm değişince sıfırlanır. */
export const older = $state({
  busy: false,
  /** Sunucuda daha eskisi kalmadı ("Hepsi bu kadar"). */
  done: false,
  /** Durumun ait olduğu görünüm anahtarı. */
  key: '',
});

function olderKey(): string {
  return JSON.stringify([mail.view, mail.accountId]);
}

// Görünüm (ya da hesap kapsamı) değişince durum sıfırlanır: `older.done` hep açık görünümü anlatır.
$effect.root(() => {
  $effect(() => {
    const key = olderKey();
    untrack(() => {
      if (older.key === key) return;
      older.key = key;
      older.done = false;
    });
  });
});

/** Görünüm sunucu kutularına karşılık geliyor mu (arama, süzgeç ve sanal görünümlerde hayır). */
function olderScope(): boolean {
  if (mail.search != null || mail.category != null || mail.only != null) return false;
  return ['inbox', 'mailbox', 'trash', 'sent', 'archive', 'drafts'].includes(mail.view.kind);
}

/** `older.done` açık görünüme mi ait (anahtarı bu görünümün anahtarı mı)? Yalnız durumu söyler. */
export function olderDoneHere(): boolean {
  return older.done && older.key === olderKey();
}

/** Bu görünümde sunucudaki postaların sonuna gelindi mi ("Hepsi bu kadar" gösterilsin mi). */
export function olderDone(): boolean {
  return olderScope() && olderDoneHere() && !mail.nextCursor && mail.items.length > 0;
}

/** Bu görünümde sunucudan daha eski posta istenebilir mi (arama ve sanal görünümlerde hayır). */
export function olderAvailable(): boolean {
  if (!olderScope()) return false;
  return !mail.nextCursor && !mail.loading && mail.items.length > 0 && !olderDoneHere();
}

/** Yerel liste bitince sunucudan bir sonraki eski partiyi indirir (`messages_older`). */
export async function loadOlder(): Promise<void> {
  if (older.busy) return;
  const key = olderKey();
  older.key = key;
  older.done = false;
  older.busy = true;
  try {
    const r = await call('messages_older', { view: mail.view, accountId: mail.accountId });
    if (olderKey() !== key) return;
    if (r.fetched) {
      // Yenileme yüklenmiş sayfaları korur (liste ilk sayfaya dönmez, kaydırma konumu kalır);
      // inen postalar listenin sonuna eklenir, sıradaki sayfa da yüklenir ki hemen görünsünler.
      await refreshList();
      await loadMore();
    }
    older.done = !r.more;
    // Hiç posta inmediyse nedeni söylenir: sunucuda kalmadı ("Hepsi bu kadar") ya da kutu henüz
    // hazır değil (eşitlenmedi, bir hesap yanıt vermedi); ikincisinde düğme yerinde kalır.
    if (!r.fetched) showToast({ text: t(r.more ? 'older.retry' : 'older.none') });
  } catch (e) {
    errorToast(e);
  } finally {
    older.busy = false;
  }
}
