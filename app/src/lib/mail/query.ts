// Arama sorgusu metni ve işleçleri: arama çipleri ve işleç tamamlama sorguyu
// buradan okur ve yazar; doğruluk kaynağı her zaman metindir. Arka uç
// işleçleri: from:/kimden:, to:/kime:, cc:, konu:, has:ek, has:kod,
// is:okunmamış, is:okunmuş, is:yıldızlı, is:şüpheli, in:, tag:,
// after:/sonra:, before:/önce: (YYYY-AA-GG), larger:/büyük:, smaller:/küçük:
// (5mb, 100kb). `hesap:` (account:) arayüzde hesap kimliklerine çevrilir ve
// arka uca ayrı gider. Saf işlevler.

import type { Account } from '../api';

export type ChipOp = 'from' | 'after' | 'before' | 'attach' | 'unread' | 'account' | 'code' | 'spam' | 'read' | 'larger' | 'smaller';

/** İşlecin tanınan adları (katlanmış) ve yazılan biçimi. */
const NAMES: Record<ChipOp, { keys: string[]; values?: string[]; write: string }> = {
  from: { keys: ['from', 'kimden'], write: 'from' },
  after: { keys: ['after', 'sonra'], write: 'after' },
  before: { keys: ['before', 'once', 'önce'], write: 'before' },
  attach: { keys: ['has'], values: ['ek', 'attachment'], write: 'has:ek' },
  unread: { keys: ['is'], values: ['okunmamış', 'okunmamis', 'unread'], write: 'is:okunmamış' },
  account: { keys: ['hesap', 'account'], write: 'hesap' },
  code: { keys: ['has'], values: ['kod', 'code'], write: 'has:kod' },
  spam: { keys: ['is'], values: ['şüpheli', 'supheli', 'spam'], write: 'is:şüpheli' },
  read: { keys: ['is'], values: ['okunmuş', 'okunmus', 'read'], write: 'is:okunmuş' },
  larger: { keys: ['larger', 'büyük', 'buyuk'], write: 'larger' },
  smaller: { keys: ['smaller', 'küçük', 'kucuk'], write: 'smaller' },
};

/** Birlikte anlamı olmayan işleçler: biri yazılınca öteki sorgudan düşer. */
const EXCLUDES: Partial<Record<ChipOp, ChipOp>> = { read: 'unread', unread: 'read' };

function lower(s: string): string {
  return s.toLocaleLowerCase('tr');
}

/** Sorguyu parçalara ayırır; `anahtar:"tırnaklı değer"` tek parçadır. */
export function tokenize(q: string): string[] {
  return q.match(/\S+?:"[^"]*"?|\S+/g) ?? [];
}

function split(token: string): { key: string; value: string } | null {
  const i = token.indexOf(':');
  if (i <= 0) return null;
  return { key: lower(token.slice(0, i)), value: token.slice(i + 1).replace(/^"|"$/g, '') };
}

function opOf(token: string): ChipOp | null {
  const p = split(token);
  if (!p) return null;
  for (const [op, n] of Object.entries(NAMES) as [ChipOp, (typeof NAMES)[ChipOp]][]) {
    if (!n.keys.includes(p.key)) continue;
    if (n.values && !n.values.includes(lower(p.value))) continue;
    return op;
  }
  return null;
}

export interface Chips {
  from: string | null;
  after: string | null;
  before: string | null;
  attach: boolean;
  unread: boolean;
  account: string | null;
  /** has:kod — doğrulama kodu içeren postalar. */
  code: boolean;
  /** is:şüpheli — şüpheli görünen postalar. */
  spam: boolean;
  /** is:okunmuş. */
  read: boolean;
  /** larger: / büyük: değeri (ör. "5mb"). */
  larger: string | null;
  /** smaller: / küçük: değeri. */
  smaller: string | null;
}

/** Sorgudaki çip işleçleri (son yazılan geçerli). */
export function readChips(q: string): Chips {
  const out: Chips = { from: null, after: null, before: null, attach: false, unread: false, account: null, code: false, spam: false, read: false, larger: null, smaller: null };
  for (const tok of tokenize(q)) {
    const op = opOf(tok);
    if (!op) continue;
    const value = split(tok)!.value;
    if (op === 'attach' || op === 'unread' || op === 'code' || op === 'spam' || op === 'read') out[op] = true;
    else if (value) out[op] = value;
  }
  return out;
}

/** Boyut değerinin okunur yazılışı: "5mb" → "5 MB"; tanınmayan değer olduğu gibi. */
export function sizeLabel(value: string): string {
  const m = /^(\d+(?:[.,]\d+)?)\s*(b|kb|mb|gb)?$/i.exec(value.trim());
  if (!m) return value;
  return `${m[1]} ${(m[2] ?? 'b').toUpperCase()}`;
}

function quote(v: string): string {
  return /\s/.test(v) ? `"${v.replace(/"/g, '')}"` : v;
}

/**
 * İşleci sorguya yazar ya da (değer `null` / `false`) kaldırır. Aynı türden
 * eski parçalar düşer; serbest metin ve diğer işleçler yerinde kalır.
 */
export function withOp(q: string, op: ChipOp, value: string | boolean | null): string {
  const other = value ? EXCLUDES[op] : undefined;
  const kept = tokenize(q).filter((tok) => {
    const o = opOf(tok);
    return o !== op && (other == null || o !== other);
  });
  if (value) {
    const n = NAMES[op];
    kept.push(n.values ? n.write : `${n.write}:${quote(String(value))}`);
  }
  return kept.join(' ');
}

// ---------------------------------------------------------------- tamamlama

export type CompleteKind = 'from' | 'to' | 'cc' | 'after' | 'before' | 'account' | 'larger' | 'smaller';

/** Yalnız tamamlanan (çipi olmayan) kişi işleçleri: alıcı ve bilgi alıcısı. */
const PEOPLE: Record<'to' | 'cc', string[]> = { to: ['to', 'kime'], cc: ['cc'] };

/** Boyut işleçlerinin hazır değerleri (tamamlama ve çip menüsü). */
export const SIZE_PRESETS = ['1mb', '5mb', '10mb', '25mb'] as const;

/** Yazılmakta olan son parça bir işleçse türü ve yazılan değer. */
export function completing(q: string): { kind: CompleteKind; value: string } | null {
  if (/\s$/.test(q)) return null;
  const last = tokenize(q).pop();
  if (!last) return null;
  const p = split(last);
  if (!p) return null;
  for (const kind of ['from', 'after', 'before', 'account', 'larger', 'smaller'] as const) {
    if (NAMES[kind].keys.includes(p.key)) return { kind, value: p.value };
  }
  for (const kind of ['to', 'cc'] as const) {
    if (PEOPLE[kind].includes(p.key)) return { kind, value: p.value };
  }
  return null;
}

/** Son parçayı tamamlanmış işleçle değiştirir; ardından boşluk bırakır. */
export function completeWith(q: string, kind: CompleteKind, value: string): string {
  const parts = tokenize(q);
  parts.pop();
  const write = kind === 'to' || kind === 'cc' ? kind : NAMES[kind].write;
  parts.push(`${write}:${quote(value)}`);
  return `${parts.join(' ')} `;
}

// ------------------------------------------------------------------- tarihler

/** Yerel günün "YYYY-AA-GG" yazılışı. */
export function ymd(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export type DatePreset = 'today' | 'week' | 'month' | 'last30';

/** Hazır tarihlerin başlangıç günü: bugün, bu haftanın pazartesisi, ayın biri, 30 gün önce. */
export function presetDate(preset: DatePreset, now: number): string {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  if (preset === 'week') d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  else if (preset === 'month') d.setDate(1);
  else if (preset === 'last30') d.setDate(d.getDate() - 30);
  return ymd(d.getTime());
}

// -------------------------------------------------------------------- hesap

/** Hesap işlecinin değeri hangi hesaplara uyar: adres, ad ya da alan adı (baştan). */
export function accountsFor(value: string, accounts: Pick<Account, 'id' | 'name' | 'email'>[]): string[] {
  const v = lower(value.trim());
  if (!v) return [];
  const exact = accounts.filter((a) => lower(a.email) === v || lower(a.name) === v);
  if (exact.length) return exact.map((a) => a.id);
  return accounts
    .filter((a) => lower(a.email).startsWith(v) || lower(a.name).startsWith(v) || lower(a.email.split('@')[1] ?? '').startsWith(v))
    .map((a) => a.id);
}

/** Bütün postalara uyan işleç: yalnız hesap işleci yazılmışsa arka uca bu gider. */
const EVERYTHING = 'after:1970-01-02';

/**
 * Arka uca gidecek sorgu ve (hesap işleci varsa) sonuçların süzüleceği
 * hesaplar. Uymayan hesap değeri boş küme döndürür (sonuç yok).
 */
export function accountScope(q: string, accounts: Pick<Account, 'id' | 'name' | 'email'>[]): { query: string; accountIds: string[] | null } {
  const rest: string[] = [];
  let ids: string[] | null = null;
  for (const tok of tokenize(q)) {
    if (opOf(tok) === 'account') {
      const found = accountsFor(split(tok)!.value, accounts);
      ids = ids ? ids.filter((x) => found.includes(x)) : found;
    } else rest.push(tok);
  }
  const query = rest.join(' ').trim();
  return { query: query || (ids ? EVERYTHING : ''), accountIds: ids };
}
