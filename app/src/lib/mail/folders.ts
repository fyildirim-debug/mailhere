// Hesap klasörlerinin ağacı ve görünen adları (kenar çubuğu, ayarlar, kurallar).

import type { Mailbox, MailboxRole } from '../api';
import { t, type MessageKey } from '../i18n/index.svelte';
import type { IconName } from '../ui/icons';

/** Rol klasörlerinin sırası; özel klasörler ardından ada göre gelir. */
const ROLE_ORDER: MailboxRole[] = ['inbox', 'drafts', 'sent', 'archive', 'junk', 'trash'];

const ROLE_LABEL: Record<Exclude<MailboxRole, 'other'>, MessageKey> = {
  inbox: 'nav.inbox',
  drafts: 'nav.drafts',
  sent: 'nav.sent',
  archive: 'nav.archive',
  junk: 'nav.junk',
  trash: 'nav.trash',
};

const ROLE_ICON: Record<MailboxRole, IconName> = {
  inbox: 'inbox',
  drafts: 'draft',
  sent: 'send',
  archive: 'archive',
  junk: 'spam',
  trash: 'trash',
  other: 'folder',
};

/** Rol klasörü uygulamanın diliyle, özel klasör sunucudaki adıyla görünür. */
export function folderLabel(b: Mailbox): string {
  return b.role === 'other' ? b.name : t(ROLE_LABEL[b.role]);
}

export function folderIcon(b: Mailbox): IconName {
  return ROLE_ICON[b.role];
}

export interface FolderNode {
  mailbox: Mailbox;
  depth: number;
  children: FolderNode[];
}

function byName(a: Mailbox, b: Mailbox): number {
  return a.name.localeCompare(b.name, 'tr', { sensitivity: 'base', numeric: true });
}

/**
 * Hesabın klasör ağacı. Gizli klasörler (ve altları) `withHidden` verilmedikçe
 * çıkarılır; üstü listede olmayan klasör köke bağlanır.
 */
export function folderTree(mailboxes: Mailbox[], accountId: string, withHidden = false): FolderNode[] {
  const own = mailboxes.filter((b) => b.accountId === accountId && (withHidden || !b.hidden));
  const ids = new Set(own.map((b) => b.id));
  const kids = new Map<string | null, Mailbox[]>();
  for (const b of own) {
    // Rol klasörü her zaman kökte; üstü gizli ya da yoksa kök.
    const parent = b.role !== 'other' || !b.parentId || !ids.has(b.parentId) ? null : b.parentId;
    const list = kids.get(parent) ?? [];
    list.push(b);
    kids.set(parent, list);
  }
  const build = (parent: string | null, depth: number, seen: Set<string>): FolderNode[] => {
    const list = [...(kids.get(parent) ?? [])];
    list.sort((a, b) => {
      const ra = ROLE_ORDER.indexOf(a.role);
      const rb = ROLE_ORDER.indexOf(b.role);
      if (ra !== rb) return (ra < 0 ? 99 : ra) - (rb < 0 ? 99 : rb);
      return byName(a, b);
    });
    return list
      .filter((b) => !seen.has(b.id))
      .map((b) => {
        const next = new Set(seen).add(b.id);
        return { mailbox: b, depth, children: build(b.id, depth + 1, next) };
      });
  };
  return build(null, 0, new Set());
}

/** Ağacı düz listeye açar; `collapsed` içindeki düğümlerin altları atlanır. */
export function flatten(nodes: FolderNode[], collapsed: Set<string> = new Set()): FolderNode[] {
  const out: FolderNode[] = [];
  const walk = (list: FolderNode[]) => {
    for (const n of list) {
      out.push(n);
      if (!collapsed.has(n.mailbox.id)) walk(n.children);
    }
  };
  walk(nodes);
  return out;
}
