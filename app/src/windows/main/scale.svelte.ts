// Arayüz ölçeği (%90–150): Ctrl + / Ctrl - / Ctrl 0 ve Ayarlar › Genel.
// Tauri'de web görünümünün kendi yakınlaştırması kullanılır (tarayıcı
// yakınlaştırması gibi: CSS piksel oranı değişir, yazı ölçeği oranları
// korunur, dar pencere kipleri ölçekli genişliğe göre seçilir). Tarayıcıda
// (geliştirme, testler) kök öğeye CSS `zoom` uygulanır. Kişisel tercih;
// tarayıcı deposunda tutulur. Mobil kabuk bu modülü hiç yüklemez.

import { isTauri } from '../../lib/api';

export const SCALES = [0.9, 1, 1.1, 1.2, 1.35, 1.5] as const;
const KEY = 'mh.desk.scale';

function load(): number {
  try {
    const v = Number(localStorage.getItem(KEY));
    return SCALES.includes(v as (typeof SCALES)[number]) ? v : 1;
  } catch {
    return 1;
  }
}

export const scale = $state({
  value: typeof window !== 'undefined' ? load() : 1,
  /** Tarayıcıda kök öğeye uygulanan ölçek (Tauri'de 1: yakınlaştırmayı web görünümü yapar). */
  css: 1,
});

/** Sıradaki ölçek adımı; uçlarda durur. */
export function stepped(current: number, dir: 1 | -1): number {
  const i = SCALES.findIndex((s) => s >= current - 1e-6);
  const at = i < 0 ? SCALES.length - 1 : i;
  const exact = Math.abs(SCALES[at] - current) < 1e-6;
  const next = dir > 0 ? (exact ? at + 1 : at) : at - 1;
  return SCALES[Math.min(Math.max(next, 0), SCALES.length - 1)];
}

async function apply(v: number) {
  const root = document.documentElement;
  if (isTauri) {
    try {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview');
      await getCurrentWebview().setZoom(v);
      root.style.removeProperty('zoom');
      root.style.removeProperty('--ui-zoom');
      scale.css = 1;
      return;
    } catch {
      /* izin yoksa kök ölçeğe düşülür */
    }
  }
  if (v === 1) {
    root.style.removeProperty('zoom');
    root.style.removeProperty('--ui-zoom');
  } else {
    root.style.setProperty('zoom', String(v));
    root.style.setProperty('--ui-zoom', String(v));
  }
  scale.css = v;
  // Yerleşim kipi ölçekli genişliğe göre yeniden seçilsin.
  window.dispatchEvent(new Event('resize'));
}

export async function setScale(v: number) {
  scale.value = v;
  try {
    if (v === 1) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, String(v));
  } catch {
    /* yalnız bu oturum */
  }
  await apply(v);
}

/** Açılışta kayıtlı ölçeği uygular. */
export function applyStoredScale() {
  if (scale.value !== 1) void apply(scale.value);
}

/** Ölçeğin yüzde yazılışı ("110"). */
export function percent(v: number): string {
  return String(Math.round(v * 100));
}
