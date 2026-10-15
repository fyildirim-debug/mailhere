// Kısayol öğretimi: fareyle yapılan bir eylemden sonra toast kısayolu bir kez
// söyler; her eylem için en çok üç kez, sonra susar. Kısayolu klavyeyle
// kullanan için o eylem öğrenilmiş sayılır. Sayaçlar ve aç/kapa kişisel
// tercihtir (tarayıcı deposu); Ayarlar › Genel'den kapatılır.

import { t, type MessageKey } from '../../lib/i18n/index.svelte';
import { addToastHint, lastToastId, showToast } from '../../lib/ui/toast.svelte';
import { KEY_LABEL, type Shortcut } from '../../lib/util/keys';

export type Teachable = 'archive' | 'snooze' | 'reply' | 'delete' | 'star' | 'unread';

export const TEACH_MAX = 3;
const KEY = 'mh.teach';

interface Stored {
  off?: boolean;
  n?: Partial<Record<Teachable, number>>;
}

function load(): Stored {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Stored;
    return v && typeof v === 'object' ? v : {};
  } catch {
    return {};
  }
}

function save(s: Stored) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    /* yalnız bu oturum */
  }
}

export const teachPref = $state({ on: typeof window !== 'undefined' ? !load().off : true });

export function setTeach(on: boolean) {
  teachPref.on = on;
  save({ ...load(), off: !on });
}

/** Eylemin kaç kez öğretildiği (test ve ayarlar için). */
export function taught(action: Teachable): number {
  return load().n?.[action] ?? 0;
}

const SOLO: Record<Teachable, MessageKey> = {
  archive: 'act.archive',
  snooze: 'act.snooze',
  reply: 'act.reply',
  delete: 'act.delete',
  star: 'act.star',
  unread: 'act.markUnread',
};

const TEACHABLE = new Set<string>(Object.keys(SOLO));

/** Kısayol klavyeyle kullanıldı: o eylem artık öğretilmez. */
export function learned(sc: Shortcut) {
  if (!TEACHABLE.has(sc)) return;
  const s = load();
  const n = s.n ?? {};
  if ((n[sc as Teachable] ?? 0) >= TEACH_MAX) return;
  save({ ...s, n: { ...n, [sc]: TEACH_MAX } });
}

/**
 * Fareyle yapılan eylemden sonra çağrılır. `e.detail === 0` klavyeyle
 * (Enter / Boşluk) basılmış düğmedir: öğretilmez. `done` verilirse eylemin
 * kendi toast'ı (Arşivlendi · Geri al) beklenir ve ipucu ona eklenir; yoksa
 * kısa bir ipucu toast'ı çıkar.
 */
export async function teach(action: Teachable, e?: { detail: number } | null, done?: Promise<unknown>) {
  if (e && e.detail === 0) return;
  if (!teachPref.on) return;
  const s = load();
  const n = s.n ?? {};
  const count = n[action] ?? 0;
  if (count >= TEACH_MAX) return;
  save({ ...s, n: { ...n, [action]: count + 1 } });
  const hint = { label: t('teach.key'), keys: KEY_LABEL[action] ?? '' };
  if (done) {
    const before = lastToastId();
    await done;
    const id = lastToastId();
    if (id !== before && addToastHint(id, hint)) return;
  }
  showToast({ text: t(SOLO[action]), hint });
}
