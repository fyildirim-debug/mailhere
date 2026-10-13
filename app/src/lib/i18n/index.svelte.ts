// Dil seçimi ve metin çevirisi.

import type { Language } from '../api/types';
import { fullTime } from '../util/format';
import { en } from './en';
import { tr, type MessageKey } from './tr';

const dictionaries: Record<Language, Record<MessageKey, string>> = { tr, en };

export const i18n = $state({ lang: 'tr' as Language });

export function setLanguage(lang: Language) {
  i18n.lang = lang;
  if (typeof document !== 'undefined') document.documentElement.lang = lang;
}

/** Metni çevirir; `{ad}` yer tutucularını doldurur. */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const text = dictionaries[i18n.lang][key] ?? tr[key] ?? key;
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (_, k: string) => (k in params ? String(params[k]) : `{${k}}`));
}

export function locale(): string {
  return i18n.lang === 'tr' ? 'tr-TR' : 'en-GB';
}

/** Yerel dilde tam zaman ("Bugün 09:42", "Yarın 09:00"). */
export function when(ms: number, now = Date.now()): string {
  return fullTime(ms, now, locale(), { today: t('time.today'), yesterday: t('time.yesterday'), tomorrow: t('time.tomorrow') });
}

export type { MessageKey };
