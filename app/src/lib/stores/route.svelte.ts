// Pencere türü adres çubuğundaki karmadan (#/...) okunur.

import type { Address, ComposeMode } from '../api/types';
import { parseRecipients } from '../util/recipients';

export type Route =
  | { name: 'main' }
  | { name: 'notify' }
  | { name: 'quick'; id: number }
  | { name: 'compose'; mode: ComposeMode; id: number | null; to: Address[] };

const MODES: ComposeMode[] = ['new', 'reply', 'replyAll', 'forward', 'draft'];

export function parseRoute(hash: string): Route {
  // `#/compose/new/0?to=…`: sorgu kısmı yalnız yazma penceresinde okunur.
  const [path, query = ''] = hash.split('?');
  const parts = path.replace(/^#\/?/, '').split('/').filter(Boolean);
  switch (parts[0]) {
    case 'notify':
      return { name: 'notify' };
    case 'quick': {
      const id = Number(parts[1]);
      return Number.isFinite(id) && id > 0 ? { name: 'quick', id } : { name: 'main' };
    }
    case 'compose': {
      const mode = MODES.includes(parts[1] as ComposeMode) ? (parts[1] as ComposeMode) : 'new';
      const id = Number(parts[2]);
      return { name: 'compose', mode, id: Number.isFinite(id) && id > 0 ? id : null, to: parseRecipients(query) };
    }
    default:
      return { name: 'main' };
  }
}

export const route = $state({ current: parseRoute(typeof location !== 'undefined' ? location.hash : '') });

if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => (route.current = parseRoute(location.hash)));
}
