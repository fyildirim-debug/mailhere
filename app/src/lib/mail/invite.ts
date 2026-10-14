// Takvim daveti: tarih/saat metni (yerel saat dilimi). Saf işlevler; testleri invite.test.ts.

import type { Invite } from '../api/types';

const DAY = 86_400_000;

function fmt(ms: number, locale: string, opts: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(locale, { hour12: false, ...opts }).format(ms);
}

/**
 * Davetin zamanı: tüm gün etkinlikte yalnız tarih (UTC gün sınırlarıyla,
 * bitiş dışlayıcı), saatli etkinlikte yerel saatle "Salı, 20 Ekim 2026 · 10:00–11:00".
 */
export function inviteWhen(inv: Invite, locale: string, allDayWord: string): string {
  if (inv.allDay) {
    const day: Intl.DateTimeFormatOptions = { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' };
    const lastDay = inv.end != null && inv.end - inv.start > DAY ? inv.end - DAY : null;
    const range = lastDay == null ? fmt(inv.start, locale, day) : `${fmt(inv.start, locale, { day: 'numeric', month: 'long', timeZone: 'UTC' })} – ${fmt(lastDay, locale, day)}`;
    return `${range} · ${allDayWord}`;
  }
  const date: Intl.DateTimeFormatOptions = { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' };
  const time: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit' };
  const start = `${fmt(inv.start, locale, date)} · ${fmt(inv.start, locale, time)}`;
  if (inv.end == null) return start;
  const sameDay = new Date(inv.start).toDateString() === new Date(inv.end).toDateString();
  if (sameDay) return `${start}–${fmt(inv.end, locale, time)}`;
  return `${start} – ${fmt(inv.end, locale, date)} · ${fmt(inv.end, locale, time)}`;
}
