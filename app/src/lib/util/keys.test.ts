import { describe, expect, it } from 'vitest';
import { GO_KEYS, goLabel, goTarget, KEY_GROUPS, shortcutFor } from './keys';
import { tr } from '../i18n/tr';

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

  it('odak kipi, seçiciler, yardım ve arama', () => {
    expect(shortcutFor({ key: 'O', shiftKey: true })).toBe('triage');
    expect(shortcutFor({ key: 'o' })).toBeNull();
    expect(shortcutFor({ key: 'v' })).toBe('movePicker');
    expect(shortcutFor({ key: 'l' })).toBe('tagPicker');
    expect(shortcutFor({ key: '?', shiftKey: true })).toBe('help');
    expect(shortcutFor({ key: '/' })).toBe('search');
    expect(shortcutFor({ key: 'g' })).toBe('go');
    expect(shortcutFor({ key: 'v', editing: true })).toBeNull();
    expect(shortcutFor({ key: '?', editing: true })).toBeNull();
  });

  it('Ctrl B ve arayüz ölçeği', () => {
    expect(shortcutFor({ key: 'b', ctrlKey: true })).toBe('sidebar');
    expect(shortcutFor({ key: 'b', ctrlKey: true, editing: true })).toBeNull();
    expect(shortcutFor({ key: '=', ctrlKey: true })).toBe('zoomIn');
    expect(shortcutFor({ key: '+', ctrlKey: true, shiftKey: true })).toBe('zoomIn');
    expect(shortcutFor({ key: '+', code: 'NumpadAdd', ctrlKey: true })).toBe('zoomIn');
    expect(shortcutFor({ key: '-', ctrlKey: true })).toBe('zoomOut');
    expect(shortcutFor({ key: '0', ctrlKey: true })).toBe('zoomReset');
    // Ölçek metin yazarken de çalışır (tarayıcıdaki gibi); değiştiricisiz çalışmaz.
    expect(shortcutFor({ key: '=', ctrlKey: true, editing: true })).toBe('zoomIn');
    expect(shortcutFor({ key: '0' })).toBeNull();
  });

  it('G sonra ikinci tuş görünüm seçer', () => {
    expect(goTarget({ key: 'i' })).toBe('inbox');
    expect(goTarget({ key: 'S', shiftKey: true })).toBe('starred');
    expect(goTarget({ key: 't' })).toBe('sent');
    expect(goTarget({ key: 'd' })).toBe('drafts');
    expect(goTarget({ key: 'a' })).toBe('archive');
    expect(goTarget({ key: '#' })).toBe('trash');
    expect(goTarget({ key: 'x' })).toBeNull();
    expect(goTarget({ key: 'i', ctrlKey: true })).toBeNull();
    expect(goLabel('sent')).toBe('G T');
    expect(new Set(GO_KEYS.map((g) => g.key)).size).toBe(GO_KEYS.length);
  });

  it('kısayol sayfasındaki her satırın metni sözlükte var', () => {
    for (const g of KEY_GROUPS) {
      expect(tr[g.title], g.title).toBeTruthy();
      for (const r of g.rows) expect(tr[r.label], r.label).toBeTruthy();
    }
  });
});
