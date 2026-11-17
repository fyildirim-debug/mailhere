import { describe, expect, it } from 'vitest';
import { nextMenuIndex } from './menukeys';

describe('menüde ok tuşlarıyla gezinme', () => {
  it('aşağı ve yukarı ok döner', () => {
    expect(nextMenuIndex('ArrowDown', 0, 3)).toBe(1);
    expect(nextMenuIndex('ArrowDown', 2, 3)).toBe(0);
    expect(nextMenuIndex('ArrowUp', 0, 3)).toBe(2);
    expect(nextMenuIndex('ArrowUp', 2, 3)).toBe(1);
  });

  it('Home başa, End sona gider', () => {
    expect(nextMenuIndex('Home', 2, 4)).toBe(0);
    expect(nextMenuIndex('End', 0, 4)).toBe(3);
  });

  it('odak menüde değilken aşağı ok ilk, yukarı ok son öğeye gider', () => {
    expect(nextMenuIndex('ArrowDown', -1, 3)).toBe(0);
    expect(nextMenuIndex('ArrowUp', -1, 3)).toBe(2);
  });

  it('başka tuş ve boş menü işlenmez', () => {
    expect(nextMenuIndex('Enter', 0, 3)).toBeNull();
    expect(nextMenuIndex('ArrowDown', 0, 0)).toBeNull();
  });
});
