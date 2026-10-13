// Tarih, boyut, ad biçimlendirme. Saf işlevler; testleri format.test.ts.

import type { Address } from '../api/types';

export type DateGroup = 'today' | 'yesterday' | 'week' | 'older';

const DAY = 86_400_000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

export function dateGroup(ms: number, now: number): DateGroup {
  const today = startOfDay(now);
  if (ms >= today) return 'today';
  if (ms >= today - DAY) return 'yesterday';
  if (ms >= today - 6 * DAY) return 'week';
  return 'older';
}

function hm(ms: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit', hour12: false }).format(ms);
}

/** Listedeki kısa zaman: bugün/dün saat, bu hafta gün adı, bu yıl gün ay, yoksa tam tarih. */
export function listTime(ms: number, now: number, locale: string): string {
  const g = dateGroup(ms, now);
  if (g === 'today' || g === 'yesterday') return hm(ms, locale);
  if (g === 'week') return new Intl.DateTimeFormat(locale, { weekday: 'short' }).format(ms);
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(locale, sameYear ? { day: 'numeric', month: 'short' } : { day: '2-digit', month: '2-digit', year: 'numeric' }).format(ms);
}

/** Gün farkı: 0 bugün, -1 dün, 1 yarın. */
export function dayDiff(ms: number, now: number): number {
  return Math.round((startOfDay(ms) - startOfDay(now)) / DAY);
}

export interface TimeWords {
  today: string;
  yesterday: string;
  tomorrow: string;
}

/** Tam zaman: "Bugün 09:42", "Dün 16:48", "Yarın 09:00", "Pazartesi 14:12", "12 Eki 2025 14:12". */
export function fullTime(ms: number, now: number, locale: string, words: TimeWords): string {
  const d = dayDiff(ms, now);
  if (d === 0) return `${words.today} ${hm(ms, locale)}`;
  if (d === -1) return `${words.yesterday} ${hm(ms, locale)}`;
  if (d === 1) return `${words.tomorrow} ${hm(ms, locale)}`;
  if (Math.abs(d) <= 6) return `${new Intl.DateTimeFormat(locale, { weekday: 'long' }).format(ms)} ${hm(ms, locale)}`;
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false }).format(ms);
}

export function fileSize(bytes: number, locale: string): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  const mb = kb / 1024;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(mb)} MB`;
}

/** Ad ya da adresten iki harflik monogram (Türkçe büyük harf kuralıyla). */
export function initials(addr: Address): string {
  const name = (addr.name ?? '').replace(/["'()]/g, '').trim();
  const words = name.split(/\s+/).filter((w) => /\p{L}/u.test(w));
  let out: string;
  if (words.length >= 2) out = words[0][0] + words[words.length - 1][0];
  else if (words.length === 1) {
    // "GitHub" gibi iç büyük harfli adlarda büyük harfler alınır.
    const caps = words[0].match(/\p{Lu}/gu);
    out = caps && caps.length >= 2 ? caps[0] + caps[1] : words[0].slice(0, 2);
  }
  else out = addr.email.replace(/[^\p{L}\p{N}]/gu, '').slice(0, 2);
  return out.toLocaleUpperCase('tr');
}

export function displayName(addr: Address): string {
  const n = addr.name?.trim();
  return n ? n : addr.email.split('@')[0];
}

/** Adresten deterministik avatar tonu (0–8). */
export function avatarTone(email: string): number {
  let h = 0;
  for (const ch of email.toLowerCase()) h = (h * 31 + ch.codePointAt(0)!) >>> 0;
  return h % 9;
}

export type FileKind = 'pdf' | 'sheet' | 'doc' | 'image' | 'archive' | 'other';

export function fileKind(name: string, mime = ''): FileKind {
  const ext = name.toLowerCase().split('.').pop() ?? '';
  if (ext === 'pdf' || mime === 'application/pdf') return 'pdf';
  if (['xls', 'xlsx', 'ods', 'csv', 'tsv'].includes(ext)) return 'sheet';
  if (['doc', 'docx', 'odt', 'rtf', 'txt', 'md', 'ppt', 'pptx', 'odp'].includes(ext)) return 'doc';
  if (mime.startsWith('image/') || ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'heic'].includes(ext)) return 'image';
  if (['zip', 'rar', '7z', 'tar', 'gz', 'xz'].includes(ext)) return 'archive';
  return 'other';
}

/** Rozette görünen kısa tür adı. */
export function fileBadge(name: string): string {
  const ext = name.split('.').pop() ?? '';
  return ext && ext.length <= 4 && ext !== name ? ext.toLocaleUpperCase('en') : 'DOSYA';
}

export interface SnoozeOption {
  key: 'evening' | 'tomorrow' | 'weekend' | 'nextWeek';
  at: number;
}

/** Erteleme seçenekleri: bu akşam 18:00 (geçtiyse yok), yarın 09:00, cumartesi 09:00, pazartesi 09:00. */
export function snoozeOptions(now: number): SnoozeOption[] {
  const base = new Date(now);
  const at = (days: number, h: number) => {
    const d = new Date(base);
    d.setDate(d.getDate() + days);
    d.setHours(h, 0, 0, 0);
    return d.getTime();
  };
  const out: SnoozeOption[] = [];
  if (base.getHours() < 17) out.push({ key: 'evening', at: at(0, 18) });
  out.push({ key: 'tomorrow', at: at(1, 9) });
  const dow = base.getDay(); // 0 pazar … 6 cumartesi
  const toSat = (6 - dow + 7) % 7 || 7;
  if (toSat > 1) out.push({ key: 'weekend', at: at(toSat, 9) });
  const toMon = (1 - dow + 7) % 7 || 7;
  out.push({ key: 'nextWeek', at: at(toMon, 9) });
  return out;
}

/** Konu önekini sadeleştirir (Re:, Fwd:, YNT:, İLT:). */
export function baseSubject(subject: string): string {
  return subject.replace(/^(\s*(re|fwd?|ynt|ilt|İlt|cevap|aw|wg)\s*(\[\d+\])?\s*:\s*)+/iu, '').trim();
}

/** Metin içindeki bağlantıları ayırır (güvenli: HTML üretmez). */
export function linkify(text: string): { text: string; href?: string }[] {
  const parts: { text: string; href?: string }[] = [];
  const re = /\b(https?:\/\/[^\s<>"')]+[^\s<>"').,;:!?])/gi;
  let last = 0;
  for (const m of text.matchAll(re)) {
    const i = m.index ?? 0;
    if (i > last) parts.push({ text: text.slice(last, i) });
    parts.push({ text: m[0], href: m[0] });
    last = i + m[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last) });
  return parts;
}
