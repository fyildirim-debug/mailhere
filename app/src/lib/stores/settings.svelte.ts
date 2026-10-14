// Ayarlar: arka uçtan okunur, tema ve dil her pencerede uygulanır.

import { call, defaultSettings, EVENTS, on, type Settings } from '../api';
import { setLanguage, t } from '../i18n/index.svelte';

export const settings = $state({
  value: { ...defaultSettings } as Settings,
  ready: false,
  /** Uygulanan tema; sistem tercihi değişince de güncellenir (Android sistem çubukları izler). */
  resolved: 'light' as 'light' | 'dark',
});

const media = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null;

/** Etkin tema (sistem seçiliyse işletim sisteminin tercihi). */
export function resolvedTheme(s: Settings = settings.value): 'light' | 'dark' {
  if (s.theme === 'system') return media?.matches ? 'dark' : 'light';
  return s.theme;
}

function apply(s: Settings) {
  settings.value = s;
  setLanguage(s.language);
  settings.resolved = resolvedTheme(s);
  if (typeof document !== 'undefined') document.documentElement.dataset.theme = settings.resolved;
}

export async function initSettings() {
  apply(settings.value);
  media?.addEventListener('change', () => apply(settings.value));
  on(EVENTS.settings, (s) => apply(s));
  try {
    apply(await call('settings_get'));
  } finally {
    settings.ready = true;
  }
}

export async function updateSettings(patch: Partial<Settings>) {
  const next = { ...settings.value, ...patch };
  apply(next);
  apply(await call('settings_set', { settings: next }));
}

export async function toggleTheme() {
  await updateSettings({ theme: resolvedTheme() === 'dark' ? 'light' : 'dark' });
}

/** Hızlı yanıt düğmeleri; kullanıcı düzenlemediyse dile göre varsayılanlar. */
export function quickReplies(s: Settings = settings.value): string[] {
  return s.quickReplies ?? [t('quick.chip1'), t('quick.chip2'), t('quick.chip3')];
}

/** Yalnız hızlı yanıtları yazar; null varsayılanlara döndürür. */
export async function saveQuickReplies(list: string[] | null) {
  apply({ ...settings.value, quickReplies: list });
  apply(await call('quick_replies_set', { replies: list }));
}
