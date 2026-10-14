import { describe, expect, it } from 'vitest';
import type { MessageSummary } from '../api/types';
import { rowsOf } from './mail.svelte';

function m(id: number, thread: string, unread = false): MessageSummary {
  return {
    id, accountId: 'a', mailboxId: 'a:INBOX', threadId: thread, from: { name: null, email: 'x@y.z' }, to: [], subject: thread,
    snippet: '', date: 1000 - id, unread, starred: false, hasAttachments: false, category: 'people', snoozedUntil: null, pinned: false, tags: [],
  };
}

describe('konuşma satırları', () => {
  it('aynı konuşmayı en yeni iletide toplar', () => {
    const rows = rowsOf([m(1, 't1'), m(2, 't2'), m(3, 't1', true)]);
    expect(rows.map((r) => r.id)).toEqual([1, 2]);
    expect(rows[0].ids).toEqual([1, 3]);
    expect(rows[0].count).toBe(2);
    expect(rows[0].unread).toBe(true);
  });

  it('arama sonuçları toplanmaz', () => {
    expect(rowsOf([m(1, 't1'), m(3, 't1')], false)).toHaveLength(2);
  });
});
