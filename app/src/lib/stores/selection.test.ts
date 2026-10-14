import { beforeEach, describe, expect, it } from 'vitest';
import { createMockBackend } from '../api/mock';
import { useBackend } from '../api';
import type { MessageSummary } from '../api/types';
import { mail } from './mail.svelte';
import { checkAll, checkRange, checkedIds, checkedRows, clearChecked, clickRow, extendChecked, isChecked, multi, toggleCheck } from './selection.svelte';

function m(id: number, thread = `t${id}`): MessageSummary {
  return {
    id, accountId: 'a', mailboxId: 'a:INBOX', threadId: thread, from: { name: null, email: 'x@y.z' }, to: [], subject: `K${id}`,
    snippet: '', date: 1000 - id, unread: false, starred: false, hasAttachments: false, category: 'people', snoozedUntil: null, followUp: null, pinned: false, tags: [],
  };
}

describe('çoklu seçim', () => {
  beforeEach(() => {
    useBackend(createMockBackend());
    // 1 ve 6 aynı konuşma: tek satır (1), iki ileti.
    mail.items = [m(1), m(2), m(3), m(4), m(5), m(6, 't1')];
    mail.search = null;
    mail.selectedId = 1;
    clearChecked();
  });

  it('Ctrl+tık tek tek işaretler ve kaldırır', () => {
    toggleCheck(2);
    expect(isChecked(2)).toBe(true);
    expect(multi()).toBe(false);
    toggleCheck(4);
    expect(multi()).toBe(true);
    expect(checkedRows().map((r) => r.id)).toEqual([2, 4]);
    toggleCheck(2);
    expect(mail.checked).toEqual([4]);
  });

  it('Shift+tık son tıklanandan (yoksa okunandan) aralık işaretler', () => {
    checkRange(3);
    expect(mail.checked).toEqual([1, 2, 3]);
    toggleCheck(5);
    checkRange(4);
    expect([...mail.checked].sort()).toEqual([1, 2, 3, 4, 5]);
  });

  it('Shift+tık çapası görünüm değişince listede yoksa okunan satırdan başlar', () => {
    toggleCheck(2);
    // Görünüm değişti: 2 artık listede yok, işaretler düştü, okunan 4.
    mail.items = [m(3), m(4), m(5)];
    mail.checked = [];
    mail.selectedId = 4;
    checkRange(5);
    expect(mail.checked).toEqual([4, 5]);
  });

  it('konuşmanın tüm iletileri toplu işleme girer', () => {
    checkRange(2);
    expect(checkedIds()).toEqual([1, 6, 2]);
  });

  it('Ctrl A görünenlerin tümü, Esc bırakır', () => {
    checkAll();
    expect(mail.checked).toEqual([1, 2, 3, 4, 5]);
    clearChecked();
    expect(mail.checked).toEqual([]);
    expect(multi()).toBe(false);
  });

  it('Shift+J okunan satırla komşusunu işaretler ve odağı taşır; listenin ucunda durur', () => {
    extendChecked(1);
    expect(mail.checked).toEqual([1, 2]);
    expect(mail.selectedId).toBe(2);
    extendChecked(-1);
    expect(mail.checked).toEqual([1, 2]);
    expect(mail.selectedId).toBe(1);
    extendChecked(-1);
    expect(mail.checked).toEqual([1, 2]);
  });

  it('düz tık seçimi bırakıp satırı açar', () => {
    toggleCheck(2);
    toggleCheck(3);
    clickRow({}, 4);
    expect(mail.checked).toEqual([]);
    expect(mail.selectedId).toBe(4);
    clickRow({ ctrlKey: true }, 2);
    clickRow({ shiftKey: true }, 3);
    expect([...mail.checked].sort()).toEqual([2, 3]);
  });
});
