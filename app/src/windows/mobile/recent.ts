// Son aramalar: yalnız bu cihazda (localStorage), en yenisi başta, en çok 6.
// Depolama kapalıysa (gizli pencere, önizleme) liste boş kalır; arama çalışır.

export const RECENT_MAX = 6;
const KEY = 'mailhere.mob.recentSearches';

/** Listeye yeni sorgu: boşluklar sadeleşir, aynı sorgu (büyük/küçük harf ayırmadan) başa taşınır. */
export function pushRecent(list: string[], query: string): string[] {
  const q = query.replace(/\s+/g, ' ').trim();
  if (!q) return list;
  const low = q.toLocaleLowerCase('tr');
  return [q, ...list.filter((x) => x.toLocaleLowerCase('tr') !== low)].slice(0, RECENT_MAX);
}

export function loadRecent(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    const v: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string' && x.trim() !== '').slice(0, RECENT_MAX) : [];
  } catch {
    return [];
  }
}

export function saveRecent(list: string[]) {
  try {
    if (list.length) localStorage.setItem(KEY, JSON.stringify(list.slice(0, RECENT_MAX)));
    else localStorage.removeItem(KEY);
  } catch {
    /* depolama yoksa yalnız bu oturumda tutulur */
  }
}
