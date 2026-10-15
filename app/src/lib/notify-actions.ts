// Bildirim düğmeleri (masaüstü köşe kartı ve Android bildirimi): sabit sıra,
// ikon ve metin anahtarları, ayardaki seçimin kuralı. Kart ve ayar bölümü
// aynı tanımı kullanır.

import { NOTIFY_ACTIONS_MAX, type NotifyAction } from './api';
import type { MessageKey } from './i18n/index.svelte';
import type { IconName } from './ui/icons';

/** Kanonik sıra: ayarda seçilenler her zaman bu sırayla dizilir. */
export const NOTIFY_ACTION_ORDER: readonly NotifyAction[] = ['reply', 'markRead', 'archive', 'trash', 'spam'];

export interface NotifyActionInfo {
  icon: IconName;
  /** Karttaki kısa etiket. */
  label: MessageKey;
  /** Erişilebilir ad ve ipucu (etiketi içerir). */
  aria: MessageKey;
  /** Ayardaki açıklama. */
  hint: MessageKey;
  /** Karttaki onay satırı (yanıtın kendi durumu var). */
  done: MessageKey | null;
}

export const NOTIFY_ACTION_INFO: Record<NotifyAction, NotifyActionInfo> = {
  reply: { icon: 'reply', label: 'notify.act.reply', aria: 'notify.aria.reply', hint: 'set.na.replyHint', done: null },
  markRead: { icon: 'check', label: 'notify.act.markRead', aria: 'notify.aria.markRead', hint: 'set.na.markReadHint', done: 'notify.done.markRead' },
  archive: { icon: 'archive', label: 'notify.act.archive', aria: 'notify.aria.archive', hint: 'set.na.archiveHint', done: 'notify.done.archive' },
  trash: { icon: 'trash', label: 'notify.act.trash', aria: 'notify.aria.trash', hint: 'set.na.trashHint', done: 'notify.done.trash' },
  spam: { icon: 'spam', label: 'notify.act.spam', aria: 'notify.aria.spam', hint: 'set.na.spamHint', done: 'notify.done.spam' },
};

/** Arka uçtan gelen listeden bilinen, yinelenmeyen en çok üç düğme (sıra korunur). */
export function shownNotifyActions(list: readonly string[] | null | undefined): NotifyAction[] {
  const out: NotifyAction[] = [];
  for (const a of list ?? []) {
    if ((NOTIFY_ACTION_ORDER as readonly string[]).includes(a) && !out.includes(a as NotifyAction)) out.push(a as NotifyAction);
  }
  return out.slice(0, NOTIFY_ACTIONS_MAX);
}

/**
 * Ayarda bir düğmeyi açar ya da kapatır. Sonuç kanonik sıradadır; üç düğme
 * seçiliyken yenisi eklenmez (liste değişmeden döner).
 */
export function toggleNotifyAction(list: readonly NotifyAction[], action: NotifyAction, on: boolean): NotifyAction[] {
  const current = shownNotifyActions(list);
  if (on && !current.includes(action) && current.length >= NOTIFY_ACTIONS_MAX) return current;
  const want = new Set(current);
  if (on) want.add(action);
  else want.delete(action);
  return NOTIFY_ACTION_ORDER.filter((a) => want.has(a));
}
