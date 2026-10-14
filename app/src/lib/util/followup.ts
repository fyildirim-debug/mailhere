// Yanıt hatırlatması seçenekleri: yarın, 3 gün, 1 hafta (sabah 09:00). Saf işlev.

export type FollowUpKey = 'tomorrow' | 'days3' | 'week';

export interface FollowUpOption {
  key: FollowUpKey;
  at: number;
}

const DAYS: [FollowUpKey, number][] = [
  ['tomorrow', 1],
  ['days3', 3],
  ['week', 7],
];

/** `now`dan itibaren N gün sonra yerel saatle 09:00. */
export function followUpOptions(now: number): FollowUpOption[] {
  return DAYS.map(([key, days]) => {
    const d = new Date(now);
    d.setDate(d.getDate() + days);
    d.setHours(9, 0, 0, 0);
    return { key, at: d.getTime() };
  });
}

/** Hatırlatma süresi dolmuş (yanıt bekleniyor) mu? */
export function followUpDue(at: number | null | undefined, now: number): boolean {
  return at != null && at <= now;
}

/** `<input type="datetime-local">` değerini zaman damgasına çevirir; geçersiz ya da geçmişse null. */
export function parseLocalDateTime(value: string, now: number): number | null {
  if (!value) return null;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) && ms > now ? ms : null;
}

/** Zaman damgasını `<input type="datetime-local">` biçimine çevirir (yerel saat). */
export function toLocalDateTime(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}
