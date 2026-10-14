// Kutular, hesaplar, etiketler ve kayıtlı aramalar için gezinme verisi:
// masaüstü kenar çubuğu (Sidebar) ve mobil çekmece (Drawer) aynı listeyi
// ve "etkin mi" kurallarını kullanır. Saf işlevler; durum dışarıdan gelir.
//
// Bilgi mimarisi: üstteki görünümler (gelen kutusu, taslaklar, gönderilenler…)
// seçili hesap kapsamına göre daralır ("Tüm hesaplar" ya da tek hesap). Rol
// klasörleri bu görünümlerle zaten sunulduğu için klasör bölümünde yalnız
// istenmeyen ve hesabın özel klasörleri listelenir; böylece tek hesapta da
// çok hesapta da hiçbir satır yinelenmez.

import type { Account, Counts, Mailbox, MailboxRole, Tag, View } from '../api';
import type { MessageKey } from '../i18n/index.svelte';
import type { Screen } from '../stores/mail.svelte';
import type { IconName } from '../ui/icons';
import { folderTree, type FolderNode } from './folders';

export interface ViewItem {
  kind: Exclude<View['kind'], 'mailbox' | 'tag'>;
  icon: IconName;
  label: MessageKey;
  count?: (c: Counts) => number;
  /** Yalnız sayacı varsa (ya da açıkken) görünür. */
  optional?: boolean;
}

/** Kenar çubuğunun görünüm bölümü, sırasıyla. */
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
  /** Hesap kapsamı: `null` tüm hesaplar. */
  accountId: string | null;
  counts: Counts;
}

/**
 * Görünümün sayacı. Gelen kutusu kapsamdaki hesabın okunmamış sayısını
 * gösterir. Diğer sayaçlar (ertelenen, taslak, elek…) yalnız uygulama
 * genelinde bilinir; kapsamdayken liste hesaba daraldığından yanıltmasın
 * diye gösterilmez.
 */
export function viewCount(item: ViewItem, counts: Counts, scope: string | null = null): number {
  if (scope) return item.kind === 'inbox' ? (counts.perAccountUnread[scope] ?? 0) : 0;
  return item.count?.(counts) ?? 0;
}

/** Öğe görünsün mü: isteğe bağlı öğeler (uygulama genelinde) sayaç ya da etkinlik ister. */
export function viewVisible(item: ViewItem, s: NavState): boolean {
  return !item.optional || viewCount(item, s.counts) > 0 || viewActive(item.kind, s);
}

/** Görünüm satırı etkin mi (kapsam ne olursa olsun). */
export function viewActive(kind: ViewItem['kind'], s: NavState): boolean {
  return s.screen === 'mail' && s.search == null && s.view.kind === kind;
}

/** "Planlananlar" öğesi: bekleyen gönderim varsa ya da ekran açıkken, taslakların altında. */
export function scheduledVisible(s: NavState): boolean {
  return s.counts.scheduled > 0 || s.screen === 'scheduled';
}

export function tagActive(tag: Tag, s: NavState): boolean {
  return s.screen === 'mail' && s.search == null && s.view.kind === 'tag' && s.view.id === tag.id;
}

export function savedSearchActive(query: string, s: NavState): boolean {
  return s.screen === 'mail' && s.search === query;
}

// ------------------------------------------------------------- hesap kapsamı

/** Kapsam satırı basılı mı: `null` "Tüm hesaplar". */
export function scopeActive(accountId: string | null, s: Pick<NavState, 'accountId'>): boolean {
  return s.accountId === accountId;
}

/**
 * Kapsam değişince açılacak görünüm: görünüm (gelen kutusu, gönderilenler,
 * etiket…) korunur; klasör, arama ya da başka ekran açıksa gelen kutusu.
 */
export function scopeView(s: Pick<NavState, 'screen' | 'search' | 'view'>): View {
  if (s.screen !== 'mail' || s.search != null || s.view.kind === 'mailbox') return { kind: 'inbox' };
  return s.view;
}

/** Klasör bölümünde listelenecek hesaplar: kapsamdaki hesap, yoksa hepsi. */
export function folderAccounts(accounts: Account[], scope: string | null): Account[] {
  const one = scope ? accounts.find((a) => a.id === scope) : undefined;
  return one ? [one] : accounts;
}

// ------------------------------------------------------------------ klasörler

/** Üstteki görünümlerin zaten sunduğu roller; klasör bölümünde listelenmez. */
const VIEW_ROLES = new Set<MailboxRole>(['inbox', 'drafts', 'sent', 'archive', 'trash']);

function relevel(nodes: FolderNode[], depth: number): FolderNode[] {
  return nodes.map((n) => ({ mailbox: n.mailbox, depth, children: relevel(n.children, depth + 1) }));
}

/**
 * Hesabın kenar çubuğu klasörleri: önce istenmeyen, sonra özel klasörler ada
 * göre. Bir görünüm rolünün altındaki özel klasörler (ör. INBOX/Projeler)
 * köke çıkar; gizli klasörler zaten listede yoktur.
 */
export function sideTree(mailboxes: Mailbox[], accountId: string): FolderNode[] {
  const roots = folderTree(mailboxes, accountId).flatMap((n) => (VIEW_ROLES.has(n.mailbox.role) ? n.children : [n]));
  roots.sort((a, b) => {
    if (a.mailbox.role !== b.mailbox.role) return a.mailbox.role === 'junk' ? -1 : b.mailbox.role === 'junk' ? 1 : 0;
    return a.mailbox.name.localeCompare(b.mailbox.name, 'tr', { sensitivity: 'base', numeric: true });
  });
  return relevel(roots, 0);
}

/** Klasör satırının hedefi: gelen kutusu rolü görünümle, diğerleri klasörün kendisiyle açılır. */
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
