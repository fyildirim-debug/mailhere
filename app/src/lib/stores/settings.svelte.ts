// Ayarlar: arka uçtan okunur, tema ve dil her pencerede uygulanır.

import { call, defaultSettings, EVENTS, on, type Settings } from '../api';
import { setLanguage } from '../i18n/index.svelte';

export const settings = $state({ value: { ...defaultSettings } as Settings, ready: false });

const media = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null;

/** Etkin tema (sistem seçiliyse işletim sisteminin tercihi). */
export function resolvedTheme(s: Settings = settings.value): 'light' | 'dark' {
  if (s.theme === 'system') return media?.matches ? 'dark' : 'light';
  return s.theme;
}

function apply(s: Settings) {
  settings.value = s;
  setLanguage(s.language);
  if (typeof document !== 'undefined') document.documentElement.dataset.theme = resolvedTheme(s);
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
