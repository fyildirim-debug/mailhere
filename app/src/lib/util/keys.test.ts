import { describe, expect, it } from 'vitest';
import { shortcutFor } from './keys';

describe('kısayollar', () => {
  it('tek tuşlar', () => {
    expect(shortcutFor({ key: 'e' })).toBe('archive');
    expect(shortcutFor({ key: 'E' })).toBe('archive');
    expect(shortcutFor({ key: 'j' })).toBe('next');
    expect(shortcutFor({ key: 'Delete' })).toBe('delete');
    expect(shortcutFor({ key: '#' })).toBe('delete');
  });

  it('metin yazarken tek tuşlar çalışmaz, Ctrl K ve Esc çalışır', () => {
    expect(shortcutFor({ key: 'e', editing: true })).toBeNull();
    expect(shortcutFor({ key: 'z', ctrlKey: true, editing: true })).toBeNull();
    expect(shortcutFor({ key: 'k', ctrlKey: true, editing: true })).toBe('palette');
    expect(shortcutFor({ key: 'Escape', editing: true })).toBe('escape');
  });

  it('F5 postaları alır; yazarken de çalışır, değiştiriciyle çalışmaz', () => {
    expect(shortcutFor({ key: 'F5' })).toBe('sync');
    expect(shortcutFor({ key: 'F5', editing: true })).toBe('sync');
    expect(shortcutFor({ key: 'F5', ctrlKey: true })).toBeNull();
    expect(shortcutFor({ key: 'F5', shiftKey: true })).toBeNull();
  });

  it('değiştirici tuşlarla tek harf tetiklenmez', () => {
    expect(shortcutFor({ key: 'r', ctrlKey: true })).toBeNull();
    expect(shortcutFor({ key: 'z', ctrlKey: true })).toBe('undo');
  });

  it('seçim ve sabitleme kısayolları', () => {
    expect(shortcutFor({ key: 'p' })).toBe('pin');
    expect(shortcutFor({ key: 'a', ctrlKey: true })).toBe('selectAll');
    expect(shortcutFor({ key: 'a', metaKey: true })).toBe('selectAll');
    expect(shortcutFor({ key: 'a', ctrlKey: true, editing: true })).toBeNull();
    expect(shortcutFor({ key: 'A', shiftKey: true })).toBe('replyAll');
    expect(shortcutFor({ key: 'J', shiftKey: true })).toBe('extendNext');
    expect(shortcutFor({ key: 'ArrowDown', shiftKey: true })).toBe('extendNext');
    expect(shortcutFor({ key: 'K', shiftKey: true })).toBe('extendPrev');
    expect(shortcutFor({ key: 'ArrowUp', shiftKey: true })).toBe('extendPrev');
    expect(shortcutFor({ key: 'ArrowDown' })).toBe('next');
  });
});
