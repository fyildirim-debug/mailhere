import { describe, expect, it } from 'vitest';
import { followUpDue, followUpOptions, parseLocalDateTime, toLocalDateTime } from './followup';

// 13 Ekim 2026 Salı 10:30
const now = new Date(2026, 9, 13, 10, 30).getTime();

describe('yanıt hatırlatması seçenekleri', () => {
  it('yarın, 3 gün ve 1 hafta sonra sabah 09:00', () => {
    const opts = followUpOptions(now);
    expect(opts.map((o) => o.key)).toEqual(['tomorrow', 'days3', 'week']);
    expect(new Date(opts[0].at).toString()).toBe(new Date(2026, 9, 14, 9, 0).toString());
    expect(new Date(opts[1].at).toString()).toBe(new Date(2026, 9, 16, 9, 0).toString());
    expect(new Date(opts[2].at).toString()).toBe(new Date(2026, 9, 20, 9, 0).toString());
  });

  it('süre dolunca yanıt bekleniyor', () => {
    expect(followUpDue(now - 1, now)).toBe(true);
    expect(followUpDue(now, now)).toBe(true);
    expect(followUpDue(now + 1, now)).toBe(false);
    expect(followUpDue(null, now)).toBe(false);
  });

  it('özel tarih yalnız gelecekte geçerli ve gidiş dönüş aynı', () => {
    const at = new Date(2026, 9, 21, 14, 5).getTime();
    expect(toLocalDateTime(at)).toBe('2026-10-21T14:05');
    expect(parseLocalDateTime('2026-10-21T14:05', now)).toBe(at);
    expect(parseLocalDateTime('2026-10-01T14:05', now)).toBeNull();
    expect(parseLocalDateTime('', now)).toBeNull();
    expect(parseLocalDateTime('bozuk', now)).toBeNull();
  });
});
