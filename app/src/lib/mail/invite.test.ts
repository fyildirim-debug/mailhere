import { describe, expect, it } from 'vitest';
import type { Invite } from '../api/types';
import { inviteWhen } from './invite';

const base: Invite = {
  uid: 'u', summary: 'Sprint', start: 0, end: null, allDay: false, location: null, organizer: null, attendees: 0, method: 'request', sequence: 0,
};

describe('davet zamanı', () => {
  it('aynı gün saatli etkinlik', () => {
    const start = new Date(2026, 9, 20, 10, 0).getTime();
    const s = inviteWhen({ ...base, start, end: start + 3_600_000 }, 'tr-TR', 'Tüm gün');
    expect(s).toContain('20 Ekim 2026');
    expect(s).toContain('10:00–11:00');
  });

  it('bitişsiz ve gün aşan etkinlik', () => {
    const start = new Date(2026, 9, 20, 22, 30).getTime();
    const single = inviteWhen({ ...base, start }, 'tr-TR', 'Tüm gün');
    expect(single).toContain('20 Ekim 2026');
    expect(single).toMatch(/ · 22:30$/);
    const s = inviteWhen({ ...base, start, end: start + 3 * 3_600_000 }, 'tr-TR', 'Tüm gün');
    expect(s).toMatch(/20 Ekim 2026.* · 22:30 – .*21 Ekim 2026.* · 01:30$/);
  });

  it('tüm gün: tek gün ve aralık (bitiş dışlayıcı)', () => {
    const start = Date.UTC(2026, 9, 20);
    const one = inviteWhen({ ...base, start, end: Date.UTC(2026, 9, 21), allDay: true }, 'tr-TR', 'Tüm gün');
    expect(one).toContain('20 Ekim 2026');
    expect(one).toContain('Tüm gün');
    expect(one).not.toContain('21');
    const range = inviteWhen({ ...base, start, end: Date.UTC(2026, 9, 23), allDay: true }, 'en-GB', 'All day');
    expect(range).toContain('20 October – ');
    expect(range).toContain('22 October 2026');
    expect(range).not.toContain('23');
  });
});
