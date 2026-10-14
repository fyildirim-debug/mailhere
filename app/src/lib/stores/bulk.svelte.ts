// Toplu işlemler ve yerel işaretler (sabitleme, etiket): iyimser güncelleme,
// sayılı bildirim ve geri alma. Tek postadaki eylemler `mail.svelte.ts`te.

import { call, toAppError, type Mailbox, type MessageId, type MoveTarget, type Tag } from '../api';
import { t, when } from '../i18n/index.svelte';
import { showToast } from '../ui/toast.svelte';
import { mail, patchLocal, refreshCounts, refreshList, reloadMailboxes, select, takeOut } from './mail.svelte';
import { clearChecked } from './selection.svelte';

function errorToast(e: unknown) {
  showToast({ text: t('toast.error', { message: toAppError(e).message }) });
}

/** Geri alınca liste yenilenir ve okunan posta yeniden seçilir. */
async function restore(selected: MessageId | null) {
  await refreshList();
  if (selected != null) void select(selected, { markRead: false });
  void refreshCounts();
}

/** Postaların geldiği klasörler (listeden ve okunan postadan bilinenler). */
function originsOf(ids: MessageId[]): Map<MessageId, string> {
  const from = new Map(mail.items.filter((m) => ids.includes(m.id)).map((m) => [m.id, m.mailboxId]));
  if (mail.detail && ids.includes(mail.detail.id)) from.set(mail.detail.id, mail.detail.mailboxId);
  return from;
}

/** Her postayı geldiği klasöre geri taşır; klasörü bilinmeyen gelen kutusuna döner. */
async function moveBack(ids: MessageId[], from: Map<MessageId, string>) {
  const back = new Map<string, MessageId[]>();
  const unknown: MessageId[] = [];
  for (const id of ids) {
    const b = from.get(id);
    if (b) back.set(b, [...(back.get(b) ?? []), id]);
    else unknown.push(id);
  }
  for (const [box, list] of back) await call('messages_move_to', { ids: list, mailboxId: box });
  if (unknown.length) await call('messages_move', { ids: unknown, target: 'inbox' });
}

/** Arşivle ya da çöpe at; geri alınca her posta geldiği klasöre döner. */
export async function bulkMove(ids: MessageId[], target: MoveTarget) {
  if (!ids.length) return;
  const selected = mail.selectedId;
  const from = originsOf(ids);
  // Klasör görünümünde listede olmayanlar da (göndericiye göre temizlik) o klasördendir.
  const view = mail.view;
  if (view.kind === 'mailbox') for (const id of ids) if (!from.has(id)) from.set(id, view.id);
  takeOut(ids);
  clearChecked();
  try {
    await call('messages_move', { ids, target });
    void refreshCounts();
    showToast({
      text: t(target === 'archive' ? 'bulk.archived' : 'bulk.deleted', { n: ids.length }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await moveBack(ids, from);
          await restore(selected);
        },
      },
    });
  } catch (e) {
    errorToast(e);
    void refreshList();
  }
}

/** Klasöre taşı; geri alınca her posta geldiği klasöre döner. */
export async function bulkMoveToFolder(ids: MessageId[], mailbox: Mailbox, label: string) {
  if (!ids.length) return;
  const selected = mail.selectedId;
  const from = originsOf(ids);
  takeOut(ids);
  clearChecked();
  try {
    await call('messages_move_to', { ids, mailboxId: mailbox.id });
    void refreshCounts();
    void reloadMailboxes();
    showToast({
      text: t('bulk.movedTo', { n: ids.length, folder: label }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await moveBack(ids, from);
          await restore(selected);
        },
      },
    });
  } catch (e) {
    errorToast(e);
    void refreshList();
  }
}

export async function bulkSnooze(ids: MessageId[], until: number) {
  if (!ids.length) return;
  const selected = mail.selectedId;
  takeOut(ids);
  clearChecked();
  try {
    await call('messages_snooze', { ids, until });
    void refreshCounts();
    showToast({
      text: t('bulk.snoozed', { n: ids.length, when: when(until) }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await call('messages_snooze', { ids, until: null });
          await restore(selected);
        },
      },
    });
  } catch (e) {
    errorToast(e);
    void refreshList();
  }
}

/** Okundu / okunmadı say; geri alınca eski durumuna döner. */
export async function bulkSetUnread(ids: MessageId[], unread: boolean) {
  if (!ids.length) return;
  const before = new Map(mail.items.filter((m) => ids.includes(m.id)).map((m) => [m.id, m.unread]));
  patchLocal(ids, { unread });
  try {
    await call('messages_flag', { ids, unread, starred: null });
    void refreshCounts();
    showToast({
      text: t(unread ? 'bulk.markedUnread' : 'bulk.markedRead', { n: ids.length }),
      action: {
        label: t('act.undo'),
        run: async () => {
          for (const was of [true, false]) {
            const list = ids.filter((id) => (before.get(id) ?? !unread) === was);
            if (list.length) await call('messages_flag', { ids: list, unread: was, starred: null });
          }
          await refreshList();
          void refreshCounts();
        },
      },
    });
  } catch (e) {
    patchLocal(ids, { unread: !unread });
    errorToast(e);
  }
}

/** Yıldızla ya da yıldızı kaldır. */
export async function bulkStar(ids: MessageId[], starred: boolean) {
  if (!ids.length) return;
  patchLocal(ids, { starred });
  try {
    await call('messages_flag', { ids, unread: null, starred });
    if (mail.view.kind === 'starred' && !starred) takeOut(ids);
    showToast({
      text: t(starred ? 'bulk.starred' : 'bulk.unstarred', { n: ids.length }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await call('messages_flag', { ids, unread: null, starred: !starred });
          await refreshList();
        },
      },
    });
  } catch (e) {
    patchLocal(ids, { starred: !starred });
    errorToast(e);
  }
}

/** Sabitler ya da sabitlemeyi kaldırır; liste yeniden sıralanır (yalnız yerel). */
export async function setPinned(ids: MessageId[], pinned: boolean) {
  if (!ids.length) return;
  const selected = mail.selectedId;
  patchLocal(ids, { pinned });
  try {
    await call('messages_pin', { ids, pinned });
    // Bu arada açılan posta eski haliyle gelmiş olabilir; işareti yeniden yaz.
    patchLocal(ids, { pinned });
    await refreshList();
    showToast({
      text: t(pinned ? 'pin.pinned' : 'pin.unpinned', { n: ids.length }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await call('messages_pin', { ids, pinned: !pinned });
          await restore(selected);
        },
      },
    });
  } catch (e) {
    patchLocal(ids, { pinned: !pinned });
    errorToast(e);
  }
}

/** Okunan postanın (ve konuşmasının) sabitlemesini çevirir. */
export async function togglePin(ids: MessageId[]) {
  const first = mail.items.find((m) => ids.includes(m.id)) ?? (mail.detail && ids.includes(mail.detail.id) ? mail.detail : null);
  await setPinned(ids, !first?.pinned);
}

/** Etiketi ekler ya da kaldırır; geri alınabilir (yalnız yerel). */
export async function setTag(ids: MessageId[], tag: Tag, on: boolean) {
  if (!ids.length) return;
  const apply = (add: boolean) => {
    const set = new Set(ids);
    const fix = (tags: string[]) => (add ? (tags.includes(tag.id) ? tags : [...tags, tag.id]) : tags.filter((x) => x !== tag.id));
    mail.items = mail.items.map((m) => (set.has(m.id) ? { ...m, tags: fix(m.tags) } : m));
    if (mail.detail && set.has(mail.detail.id)) mail.detail = { ...mail.detail, tags: fix(mail.detail.tags) };
  };
  apply(on);
  try {
    await call('messages_tag', { ids, tagId: tag.id, on });
    // Bu arada açılan posta eski haliyle gelmiş olabilir; etiketi yeniden yaz.
    apply(on);
    if (mail.view.kind === 'tag' && mail.view.id === tag.id && !on) takeOut(ids);
    const single = ids.length === 1;
    showToast({
      text: single
        ? t(on ? 'tag.added' : 'tag.removedFrom', { tag: tag.name })
        : t(on ? 'bulk.tagged' : 'bulk.untagged', { n: ids.length, tag: tag.name }),
      action: {
        label: t('act.undo'),
        run: async () => {
          await call('messages_tag', { ids, tagId: tag.id, on: !on });
          await refreshList();
        },
      },
    });
  } catch (e) {
    apply(!on);
    errorToast(e);
  }
}

/**
 * Göndericinin kapsamdaki (gelen kutusu, açık klasör ya da etiket) postaları,
 * yeniden eskiye; yalnız postanın hesabında.
 */
export async function senderMessages(accountId: string, email: string): Promise<MessageId[]> {
  return call('messages_by_sender', { accountId, email, view: mail.view });
}
