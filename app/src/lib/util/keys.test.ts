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

  it('değiştirici tuşlarla tek harf tetiklenmez', () => {
    expect(shortcutFor({ key: 'r', ctrlKey: true })).toBeNull();
    expect(shortcutFor({ key: 'z', ctrlKey: true })).toBe('undo');
  });
});
