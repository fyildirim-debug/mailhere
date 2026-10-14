// Kutular, hesaplar, etiketler ve kayıtlı aramalar için gezinme verisi:
// masaüstü kenar çubuğu (Sidebar) ve mobil çekmece (Drawer) aynı listeyi
// ve "etkin mi" kurallarını kullanır. Saf işlevler; durum dışarıdan gelir.

import type { Account, Counts, Mailbox, Tag, View } from '../api';
import type { MessageKey } from '../i18n/index.svelte';
import type { Screen } from '../stores/mail.svelte';
import type { IconName } from '../ui/icons';

export interface ViewItem {
  kind: Exclude<View['kind'], 'mailbox' | 'tag'>;
  icon: IconName;
  label: MessageKey;
  count?: (c: Counts) => number;
  /** Yalnız sayacı varsa (ya da açıkken) görünür. */
  optional?: boolean;
}

/** Kenar çubuğunun üst bölümü, sırasıyla. */
export const VIEW_ITEMS: ViewItem[] = [
  { kind: 'inbox', icon: 'inbox', label: 'nav.inbox', count: (c) => c.inboxUnread },
  { kind: 'screener', icon: 'sieve', label: 'nav.screener', count: (c) => c.screener, optional: true },
  { kind: 'snoozed', icon: 'clock', label: 'nav.snoozed', count: (c) => c.snoozed },
  { kind: 'followups', icon: 'followUp', label: 'nav.followups', count: (c) => c.followUps, optional: true },
  { kind: 'starred', icon: 'star', label: 'nav.starred' },
  { kind: 'drafts', icon: 'draft', label: 'nav.drafts', count: (c) => c.drafts },
  { kind: 'sent', icon: 'send', label: 'nav.sent' },
  { kind: 'archive', icon: 'archive', label: 'nav.archive' },
  { kind: 'trash', icon: 'trash', label: 'nav.trash' },
];

/** Etkinlik kuralları için gereken durum (mail deposunun ilgili alanları). */
export interface NavState {
  screen: Screen;
  search: string | null;
  view: View;
  accountId: string | null;
  counts: Counts;
}

export function viewCount(item: ViewItem, counts: Counts): number {
  return item.count?.(counts) ?? 0;
}

/** Öğe görünsün mü: isteğe bağlı öğeler sayaç ya da etkinlik ister. */
export function viewVisible(item: ViewItem, s: NavState): boolean {
  return !item.optional || viewCount(item, s.counts) > 0 || viewActive(item.kind, s);
}

/** Tüm hesaplar için görünüm (hesap seçili değilken). */
export function viewActive(kind: ViewItem['kind'], s: NavState): boolean {
  return s.screen === 'mail' && s.search == null && s.view.kind === kind && s.accountId == null;
}

/** "Zamanlanmış" öğesi: bekleyen gönderim varsa ya da ekran açıkken, taslakların altında. */
export function scheduledVisible(s: NavState): boolean {
  return s.counts.scheduled > 0 || s.screen === 'scheduled';
}

export function tagActive(tag: Tag, s: NavState): boolean {
  return s.screen === 'mail' && s.search == null && s.view.kind === 'tag' && s.view.id === tag.id;
}

export function savedSearchActive(query: string, s: NavState): boolean {
  return s.screen === 'mail' && s.search === query;
}

/** Hesap satırı: o hesabın gelen kutusu açıkken ve klasörleri kapalıyken. */
export function accountActive(account: Account, expanded: boolean, s: NavState): boolean {
  return s.screen === 'mail' && s.search == null && s.view.kind === 'inbox' && s.accountId === account.id && !expanded;
}

/** Gelen kutusu ertelenenleri gizleyen görünümle açılır; diğerleri klasörün kendisi. */
export function folderTarget(account: Account, b: Mailbox): { view: View; accountId: string } {
  return { view: b.role === 'inbox' ? { kind: 'inbox' } : { kind: 'mailbox', id: b.id }, accountId: account.id };
}

export function folderActive(account: Account, b: Mailbox, s: NavState): boolean {
  if (s.screen !== 'mail' || s.search != null || s.accountId !== account.id) return false;
  return b.role === 'inbox' ? s.view.kind === 'inbox' : s.view.kind === 'mailbox' && s.view.id === b.id;
}

/** Klasör sayacı: gönderilen, taslak ve çöpte okunmamış sayısı gösterilmez. */
export function folderCount(b: Mailbox): number {
  return b.role === 'sent' || b.role === 'drafts' || b.role === 'trash' ? 0 : b.unread;
}

export function domainOf(email: string): string {
  return email.split('@')[1] ?? email;
}

/** Hesap satırının ikincil metni: alan adı ya da JMAP. */
export function accountHint(account: Account): string {
  return account.kind === 'jmap' ? 'JMAP' : domainOf(account.email);
}

// ------------------------------------------------ açık hesaplar ve klasörler
// Kişisel kolaylık: yalnız bu cihazda, tarayıcı deposunda tutulur.

export const OPEN_ACCOUNTS_KEY = 'mailhere.side.open';
export const COLLAPSED_FOLDERS_KEY = 'mailhere.side.collapsed';

export function loadSet(key: string): Set<string> | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? new Set(JSON.parse(raw) as string[]) : null;
  } catch {
    return null;
  }
}

export function saveSet(key: string, set: Set<string>) {
  try {
    localStorage.setItem(key, JSON.stringify([...set]));
  } catch {
    /* depo kapalıysa yalnız bu oturumda geçerli */
  }
}

/** İlk açılışta tek hesap varsa klasörleri açık gelir. */
export function initialOpenAccounts(accounts: Account[]): Set<string> {
  return loadSet(OPEN_ACCOUNTS_KEY) ?? new Set(accounts.length === 1 ? [accounts[0].id] : []);
}

export function toggled(set: Set<string>, id: string): Set<string> {
  const next = new Set(set);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}
