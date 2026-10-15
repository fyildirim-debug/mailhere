import { beforeEach, describe, expect, it } from 'vitest';
import { createMockBackend } from '../../lib/api/mock';
import { useBackend } from '../../lib/api';
import type { MessageSummary } from '../../lib/api/types';
import { mail } from '../../lib/stores/mail.svelte';
import { fold, matchRange, score, scoreText } from './fuzzy';
import { clamp, LIST, listWidthFor, modeFor, READER_MIN } from './layout.svelte';
import { SCALES, stepped } from './scale.svelte';
import { back, finished, progress, skip, startTriage, stopTriage, triage } from './triage.svelte';

describe('bulanık eşleşme', () => {
  it('Türkçe harfler ve aksanlar katlanır', () => {
    expect(fold('Çöpe GİT')).toBe('cope git');
    expect(fold('Işık')).toBe('isik');
  });

  it('puan sırası: tam > baştan > sözcük başı > içinde > harfler sırayla', () => {
    const exact = scoreText('Arşivle', 'arsivle');
    const prefix = scoreText('Arşivle', 'arş');
    const word = scoreText('Gelen kutusuna git', 'kutu');
    const inside = scoreText('Gelen kutusuna git', 'usu');
    const initials = scoreText('Gelen kutusuna git', 'gkg');
    const loose = scoreText('Gönderilenlere git', 'gnderilen');
    expect(exact).toBeGreaterThan(prefix);
    expect(prefix).toBeGreaterThan(word);
    expect(word).toBeGreaterThan(inside);
    expect(inside).toBeGreaterThan(initials);
    expect(initials).toBeGreaterThan(loose);
    expect(loose).toBeGreaterThan(0);
    expect(scoreText('Arşivle', 'xyz')).toBe(0);
  });

  it('eşanlamlılar ve çok sözcüklü arama', () => {
    expect(score(['Sil', 'Delete', 'çöp kaldır trash'], 'trash')).toBeGreaterThan(0);
    expect(score(['Gelen kutusuna git', 'Go to inbox'], 'git gelen')).toBeGreaterThan(0);
    expect(score(['Gelen kutusuna git'], 'git yok')).toBe(0);
  });

  it('vurgu aralığı etikete denk düşer', () => {
    expect(matchRange('Çöpe git', 'cop')).toEqual([0, 3]);
    expect(matchRange('Arşive git', 'git')).toEqual([7, 10]);
    expect(matchRange('Arşive git', 'xyz')).toBeNull();
  });
});

describe('yerleşim', () => {
  it('genişliğe göre kip', () => {
    expect(modeFor(1440)).toBe('wide');
    expect(modeFor(1120)).toBe('wide');
    expect(modeFor(1119)).toBe('rail');
    expect(modeFor(860)).toBe('rail');
    expect(modeFor(859)).toBe('stack');
  });

  it('liste genişliği okuma alanına yer bırakır', () => {
    expect(listWidthFor(1440, 248, 404)).toBe(404);
    expect(listWidthFor(1440, 248, 9999)).toBe(LIST.max);
    expect(listWidthFor(1000, 56, 600)).toBe(1000 - 56 - READER_MIN);
    // Çok dar pencerede alt sınır korunur.
    expect(listWidthFor(860, 56, 404)).toBe(860 - 56 - READER_MIN);
    expect(listWidthFor(800, 56, 404)).toBe(LIST.min);
    expect(clamp(10, { min: 20, max: 30 })).toBe(20);
  });
});

describe('arayüz ölçeği', () => {
  it('adımlar uçlarda durur; ara değerden en yakın adıma gidilir', () => {
    expect(SCALES[0]).toBe(0.9);
    expect(SCALES[SCALES.length - 1]).toBe(1.5);
    expect(stepped(1, 1)).toBe(1.1);
    expect(stepped(1, -1)).toBe(0.9);
    expect(stepped(0.9, -1)).toBe(0.9);
    expect(stepped(1.5, 1)).toBe(1.5);
    expect(stepped(1.15, 1)).toBe(1.2);
    expect(stepped(1.15, -1)).toBe(1.1);
  });
});

function m(id: number, thread = `t${id}`): MessageSummary {
  return {
    id, accountId: 'a', mailboxId: 'a:INBOX', threadId: thread, from: { name: null, email: 'x@y.z' }, to: [], subject: `K${id}`,
    snippet: '', date: 1000 - id, unread: false, starred: false, hasAttachments: false, category: 'people', snoozedUntil: null, followUp: null, pinned: false, tags: [],
  };
}

describe('odak kipi', () => {
  beforeEach(() => {
    useBackend(createMockBackend());
    mail.items = [m(1), m(2), m(3), m(4, 't1')];
    mail.search = null;
    mail.selectedId = 2;
    mail.checked = [3];
    stopTriage();
  });

  it('okunan postadan başlar, çoklu seçimi bırakır; atla ve geri', () => {
    startTriage();
    expect(triage.on).toBe(true);
    expect(mail.checked).toEqual([]);
    // 1 ve 4 aynı konuşma: üç satır.
    expect(triage.queue).toEqual([1, 2, 3]);
    expect(triage.cursor).toBe(2);
    expect(progress()).toEqual({ n: 1, total: 3 });
    skip();
    expect(triage.cursor).toBe(3);
    expect(progress()).toEqual({ n: 2, total: 3 });
    // Sondan sonra baştaki işlenmemiş satır.
    skip();
    expect(triage.cursor).toBe(1);
    back();
    expect(triage.cursor).toBe(3);
    expect(progress()).toEqual({ n: 2, total: 3 });
    skip();
    skip();
    expect(finished()).toBe(true);
    expect(progress()).toEqual({ n: 3, total: 3 });
    expect(mail.selectedId).toBeNull();
    stopTriage();
    expect(triage.on).toBe(false);
  });

  it('boş listede hemen biter', () => {
    mail.items = [];
    mail.selectedId = null;
    startTriage();
    expect(finished()).toBe(true);
    expect(progress()).toEqual({ n: 0, total: 0 });
  });
});
