// Posta listesinin sırası ve grup başlıkları: masaüstü (MessageList) ve
// mobil (MobileList) aynı mantığı kullanır. Saf işlevler.

import type { Account, Category, ListOnly, Mailbox, MessageSummary, Tag, View } from '../api';
import { t, type MessageKey } from '../i18n/index.svelte';
import { followUpDue } from '../util/followup';
import { dateGroup, type DateGroup } from '../util/format';
import type { IconName } from '../ui/icons';
import { folderLabel } from './folders';

export type ListGroup = DateGroup | 'due' | 'pinned';

const GROUP_LABEL: Record<DateGroup, MessageKey> = {
  today: 'group.today',
  yesterday: 'group.yesterday',
  week: 'group.week',
  older: 'group.older',
};

export interface ListContext {
  view: View;
  search: string | null;
  now: number;
}

/** Gelen kutusu görünümü (arama dışı): süresi dolan hatırlatmalar başa gelir. */
export function isInboxView(ctx: Pick<ListContext, 'view' | 'search'>): boolean {
  return ctx.view.kind === 'inbox' && ctx.search == null;
}

/** Gelen kutusu ve klasörde sabitlenenler "Sabitlenenler" grubunda, en üstte. */
export function isPinnedView(ctx: Pick<ListContext, 'view' | 'search'>): boolean {
  return ctx.search == null && (ctx.view.kind === 'inbox' || ctx.view.kind === 'mailbox');
}

/** Gelen kutusunun başına düşen, süresi dolan yanıt hatırlatması. */
export function isDueRow(m: MessageSummary, ctx: ListContext): boolean {
  return isInboxView(ctx) && followUpDue(m.followUp, ctx.now);
}

/**
 * Arka uç zaten süresi dolan hatırlatmaları ve sabitlenenleri öne koyar;
 * yerel işaret anında da aynı sıra korunur (sıralama kararlıdır).
 */
export function orderRows<T extends MessageSummary>(rows: T[], ctx: ListContext): T[] {
  if (!isPinnedView(ctx)) return rows;
  const rank = (r: MessageSummary) => (isDueRow(r, ctx) ? 2 : r.pinned ? 1 : 0);
  return [...rows].sort((a, b) => rank(b) - rank(a));
}

/** Satırın grup anahtarı: süresi dolan, sabitlenen ya da tarih grubu. */
export function groupOf(m: MessageSummary, ctx: ListContext): ListGroup {
  if (isDueRow(m, ctx)) return 'due';
  if (isPinnedView(ctx) && m.pinned) return 'pinned';
  return dateGroup(m.date, ctx.now);
}

export function groupLabel(g: ListGroup): string {
  if (g === 'due') return t('nav.followups');
  if (g === 'pinned') return t('list.pinnedGroup');
  return t(GROUP_LABEL[g]);
}

// ------------------------------------------------------------ başlık ve boş durum

const TITLE_KEY: Record<Exclude<View['kind'], 'mailbox' | 'tag'>, MessageKey> = {
  inbox: 'nav.inbox',
  snoozed: 'nav.snoozed',
  starred: 'nav.starred',
  drafts: 'nav.drafts',
  sent: 'nav.sent',
  archive: 'nav.archive',
  trash: 'nav.trash',
  followups: 'nav.followups',
  screener: 'nav.screener',
};

export interface TitleContext {
  view: View;
  search: string | null;
  accountId: string | null;
  mailboxes: Mailbox[];
  accounts: Account[];
  tagById: (id: string) => Tag | undefined;
}

/** Liste başlığı: arama sorgusu, klasör/etiket adı ya da görünüm; hesap seçiliyse adıyla. */
export function listTitle(c: TitleContext): string {
  if (c.search != null) return t('list.searchTitle', { q: c.search });
  const view = c.view;
  const base =
    view.kind === 'mailbox'
      ? (() => {
          const box = c.mailboxes.find((b) => b.id === view.id);
          return box ? folderLabel(box) : '';
        })()
      : view.kind === 'tag'
        ? (c.tagById(view.id)?.name ?? t('tag.title'))
        : t(TITLE_KEY[view.kind]);
  const acc = c.accounts.find((a) => a.id === c.accountId);
  return acc ? `${base} · ${acc.name}` : base;
}

/** Boş durumun metni ve simgesi. */
export interface EmptyState {
  title: MessageKey;
  body: MessageKey;
  icon: IconName;
}

/**
 * Boş durum: görünümün ne olduğunu söyler (ne oldu + ne yapılabilir). Arama
 * sonucu her görünümde aramaya göre konuşur; süzgeçli liste süzgece göre.
 */
export function emptyState(c: { view: View; search: string | null; category: Category | null; only: ListOnly | null }): EmptyState {
  if (c.search != null) return { title: 'list.emptySearchTitle', body: 'list.emptySearchBody', icon: 'search' };
  if (!c.category && !c.only) {
    switch (c.view.kind) {
      case 'followups':
        return { title: 'follow.emptyTitle', body: 'follow.emptyBody', icon: 'check' };
      case 'screener':
        return { title: 'screen.emptyTitle', body: 'screen.emptyBody', icon: 'check' };
      case 'tag':
        return { title: 'list.emptyTitle', body: 'tag.emptyBody', icon: 'check' };
      case 'snoozed':
        return { title: 'list.emptySnoozedTitle', body: 'list.emptySnoozedBody', icon: 'check' };
      case 'starred':
        return { title: 'list.emptyStarredTitle', body: 'list.emptyStarredBody', icon: 'check' };
      case 'drafts':
        return { title: 'list.emptyDraftsTitle', body: 'list.emptyDraftsBody', icon: 'check' };
      case 'sent':
        return { title: 'list.emptySentTitle', body: 'list.emptySentBody', icon: 'check' };
      case 'archive':
        return { title: 'list.emptyArchiveTitle', body: 'list.emptyArchiveBody', icon: 'check' };
      case 'trash':
        return { title: 'list.emptyTrashTitle', body: 'list.emptyTrashBody', icon: 'trash' };
      case 'mailbox':
        return { title: 'list.emptyMailboxTitle', body: 'list.emptyMailboxBody', icon: 'check' };
    }
  }
  return { title: 'list.emptyTitle', body: c.category || c.only ? 'list.emptyFilteredBody' : 'list.emptyBody', icon: 'check' };
}
