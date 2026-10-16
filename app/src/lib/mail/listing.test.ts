import { describe, expect, it } from 'vitest';
import type { View } from '../api';
import { emptyState } from './listing';

const base = { search: null, category: null, only: null } as const;
const of = (view: View) => emptyState({ ...base, view });

describe('emptyState', () => {
  it('gelen kutusu genel sakin metni korur', () => {
    expect(of({ kind: 'inbox' })).toEqual({ title: 'list.emptyTitle', body: 'list.emptyBody', icon: 'check' });
  });

  it('her görünüm kendi metnini verir', () => {
    expect(of({ kind: 'snoozed' }).title).toBe('list.emptySnoozedTitle');
    expect(of({ kind: 'starred' }).title).toBe('list.emptyStarredTitle');
    expect(of({ kind: 'drafts' }).title).toBe('list.emptyDraftsTitle');
    expect(of({ kind: 'sent' }).title).toBe('list.emptySentTitle');
    expect(of({ kind: 'archive' }).title).toBe('list.emptyArchiveTitle');
    expect(of({ kind: 'mailbox', id: 'k1' })).toEqual({ title: 'list.emptyMailboxTitle', body: 'list.emptyMailboxBody', icon: 'check' });
    expect(of({ kind: 'trash' })).toEqual({ title: 'list.emptyTrashTitle', body: 'list.emptyTrashBody', icon: 'trash' });
    expect(of({ kind: 'followups' }).title).toBe('follow.emptyTitle');
    expect(of({ kind: 'screener' }).title).toBe('screen.emptyTitle');
    expect(of({ kind: 'tag', id: 't1' }).body).toBe('tag.emptyBody');
  });

  it('arama sonucu her görünümde aramaya göre konuşur', () => {
    for (const view of [{ kind: 'inbox' }, { kind: 'trash' }, { kind: 'snoozed' }] as View[]) {
      expect(emptyState({ ...base, view, search: 'zzzz' })).toEqual({ title: 'list.emptySearchTitle', body: 'list.emptySearchBody', icon: 'search' });
    }
    expect(emptyState({ ...base, view: { kind: 'inbox' }, search: 'fatura', only: 'unread' }).title).toBe('list.emptySearchTitle');
  });

  it('süzgeçli liste süzgece göre konuşur', () => {
    expect(emptyState({ ...base, view: { kind: 'inbox' }, category: 'newsletters' }).body).toBe('list.emptyFilteredBody');
    expect(emptyState({ ...base, view: { kind: 'archive' }, only: 'unread' }).body).toBe('list.emptyFilteredBody');
  });
});
