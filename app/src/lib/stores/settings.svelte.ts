// Ayarlar: arka uçtan okunur, tema ve dil her pencerede uygulanır.

import { call, defaultSettings, EVENTS, on, toAppError, type Settings } from '../api';
import { setLanguage, t } from '../i18n/index.svelte';

export const settings = $state({
  value: { ...defaultSettings } as Settings,
  ready: false,
  /** Uygulanan tema; sistem tercihi değişince de güncellenir (Android sistem çubukları izler). */
  resolved: 'light' as 'light' | 'dark',
});

const media = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null;

/**
 * Kilitli açılışta ayarlar okunamaz (veritabanı kapalı); tema ve dil tarayıcı
 * deposundaki son ipucuyla uygulanır ki kilit ekranı doğru dilde ve temada
 * gelsin. Gizli ya da posta verisi değildir.
 */
const HINT_KEY = 'mailhere.ui-hint';

function readHint(): Partial<Pick<Settings, 'theme' | 'language'>> {
  try {
    const raw = localStorage.getItem(HINT_KEY);
    if (!raw) return {};
    const h = JSON.parse(raw) as { theme?: Settings['theme']; language?: Settings['language'] };
    const out: Partial<Pick<Settings, 'theme' | 'language'>> = {};
    if (h.theme === 'light' || h.theme === 'dark' || h.theme === 'system') out.theme = h.theme;
    if (h.language === 'tr' || h.language === 'en') out.language = h.language;
    return out;
  } catch {
    return {};
  }
}

function writeHint(s: Settings) {
  try {
    localStorage.setItem(HINT_KEY, JSON.stringify({ theme: s.theme, language: s.language }));
  } catch {
    /* özel pencerede depolama kapalı olabilir */
  }
}

/** Etkin tema (sistem seçiliyse işletim sisteminin tercihi). */
export function resolvedTheme(s: Settings = settings.value): 'light' | 'dark' {
  if (s.theme === 'system') return media?.matches ? 'dark' : 'light';
  return s.theme;
}

/** Postalar koyu temaya uyarlanır mı: tema koyu ve "Postaları koyu temaya uyarla" açık. */
export function darkMailWanted(s: Settings = settings.value): boolean {
  // Varsayılanda uygulanan tema okunur: sistem teması değişince de tepki verir.
  const theme = s === settings.value ? settings.resolved : resolvedTheme(s);
  return theme === 'dark' && s.darkMail;
}

function apply(s: Settings) {
  settings.value = s;
  setLanguage(s.language);
  settings.resolved = resolvedTheme(s);
  if (typeof document !== 'undefined') document.documentElement.dataset.theme = settings.resolved;
}

/** Ayarları arka uçtan yeniden okur; kilitliyken (`locked`) sessizce geçer. */
export async function refreshSettings(): Promise<boolean> {
  try {
    const s = await call('settings_get');
    apply(s);
    writeHint(s);
    return true;
  } catch {
    return false;
  }
}

export async function initSettings() {
  apply({ ...settings.value, ...readHint() });
  media?.addEventListener('change', () => apply(settings.value));
  on(EVENTS.settings, (s) => {
    apply(s);
    writeHint(s);
  });
  try {
    // Kilitli açılışta `locked` hatası: ipucuyla sürer, kilit açılınca
    // `refreshSettings` çağrılır.
    await refreshSettings();
  } finally {
    settings.ready = true;
  }
}

export async function updateSettings(patch: Partial<Settings>) {
  const next = { ...settings.value, ...patch };
  apply(next);
  try {
    apply(await call('settings_set', { settings: next }));
  } catch (e) {
    // Kilit ekranında tema değiştirilebilir; arka uç kilitliyken yazmaz, kilit açılınca ayar yeniden okunur.
    if (toAppError(e).code !== 'locked') throw e;
  }
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
