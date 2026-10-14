// Şablon ve hızlı yanıt değişkenleri: {ad}, {tam_ad}, {adres}, {benim_adim},
// {tarih}, {gun} ve İngilizce karşılıkları. Saf işlev; testleri variables.test.ts.

import type { Address } from '../api/types';

export interface VariableContext {
  /** Alıcı (ilk alıcı ya da yanıtlanan kişi). */
  recipient?: Address | null;
  /** Gönderen (hesabın görünen adı ve adresi). */
  sender?: Address | null;
  /** "Bugün"; verilmezse şimdi. */
  now?: Date;
  /** Tarih ve gün biçimi için yerel; verilmezse tr-TR. */
  locale?: string;
}

/** Kanonik değişken adı → kabul edilen yazımlar (Türkçe ve İngilizce). */
const ALIASES: Record<string, string[]> = {
  ad: ['ad', 'isim', 'first_name', 'firstname', 'name'],
  tam_ad: ['tam_ad', 'tamad', 'full_name', 'fullname'],
  adres: ['adres', 'eposta', 'email', 'address'],
  benim_adim: ['benim_adim', 'benim_adım', 'my_name', 'myname', 'sender'],
  tarih: ['tarih', 'date', 'today', 'bugun', 'bugün'],
  gun: ['gun', 'gün', 'day', 'weekday'],
};

const LOOKUP = new Map<string, string>();
for (const [key, names] of Object.entries(ALIASES)) for (const n of names) LOOKUP.set(n, key);

/** Değişkenin kanonik adı; bilinmiyorsa null. */
export function variableKey(raw: string): string | null {
  return LOOKUP.get(raw.trim().toLocaleLowerCase('tr')) ?? null;
}

/** Ad varsa ilk sözcüğü; yoksa boş (adresin yerel kısmı ad sayılmaz). */
function firstName(a: Address | null | undefined): string {
  const n = a?.name?.trim() ?? '';
  if (!n || n.includes('@')) return '';
  return n.split(/\s+/)[0] ?? '';
}

function fullName(a: Address | null | undefined): string {
  const n = a?.name?.trim() ?? '';
  return n.includes('@') ? '' : n;
}

/** Değişkenlerin değerleri; değeri olmayan boş metin. */
export function variableValues(ctx: VariableContext): Record<string, string> {
  const now = ctx.now ?? new Date();
  const locale = ctx.locale ?? 'tr-TR';
  return {
    ad: firstName(ctx.recipient),
    tam_ad: fullName(ctx.recipient),
    adres: ctx.recipient?.email?.trim() ?? '',
    benim_adim: fullName(ctx.sender) || firstName(ctx.sender),
    tarih: new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'long', year: 'numeric' }).format(now),
    gun: new Intl.DateTimeFormat(locale, { weekday: 'long' }).format(now),
  };
}

/**
 * Metindeki değişkenleri doldurur. Bilinmeyen `{...}` olduğu gibi kalır;
 * değeri olmayan bilinen değişken önündeki boşlukla birlikte silinir
 * ("Merhaba {ad}," → "Merhaba,").
 */
export function fillVariables(text: string, ctx: VariableContext): string {
  if (!text.includes('{')) return text;
  const values = variableValues(ctx);
  return text.replace(/( ?)\{([^{}\s]{1,32})\}/gu, (all, space: string, raw: string) => {
    const key = variableKey(raw);
    if (!key) return all;
    const value = values[key] ?? '';
    return value ? space + value : '';
  });
}

/** Ayarlardaki yardım için: kanonik ad → İngilizce eş anlamlı. */
export const VARIABLE_HELP: { tr: string; en: string }[] = [
  { tr: 'ad', en: 'first_name' },
  { tr: 'tam_ad', en: 'full_name' },
  { tr: 'adres', en: 'email' },
  { tr: 'benim_adim', en: 'my_name' },
  { tr: 'tarih', en: 'date' },
  { tr: 'gun', en: 'day' },
];
