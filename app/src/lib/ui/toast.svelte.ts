// Pencere içi geri bildirim (alt ortada, ters renkli, "Geri al" eylemli).

export interface Toast {
  id: number;
  text: string;
  action?: { label: string; run: () => void | Promise<void> };
  /** Milisaniye; varsayılan 5 sn. */
  duration?: number;
  /** Kısayol ipucu (masaüstü): metnin yanında soluk "Kısayol E". */
  hint?: { label: string; keys: string };
}

let seq = 0;
let timer: ReturnType<typeof setTimeout> | null = null;

export const toasts = $state({ current: null as Toast | null });

export function showToast(toast: Omit<Toast, 'id'>) {
  if (timer) clearTimeout(timer);
  const t = { ...toast, id: ++seq };
  toasts.current = t;
  timer = setTimeout(() => {
    if (toasts.current?.id === t.id) toasts.current = null;
  }, toast.duration ?? 5000);
}

/** Son toast'ın kimliği (yeni bir toast çıktı mı diye bakmak için). */
export function lastToastId(): number {
  return seq;
}

/** Gösterilen toast'a kısayol ipucu ekler; süresi ve canlandırması değişmez. */
export function addToastHint(id: number, hint: NonNullable<Toast['hint']>): boolean {
  const cur = toasts.current;
  if (!cur || cur.id !== id) return false;
  toasts.current = { ...cur, hint };
  return true;
}

/** Toast'ın eylemi çalıştıktan sonra `after` da çalışır (ör. geri alınan postayı yeniden göstermek). */
export function afterToastAction(id: number, after: () => void): boolean {
  const cur = toasts.current;
  if (!cur || cur.id !== id || !cur.action) return false;
  const action = cur.action;
  toasts.current = {
    ...cur,
    action: {
      ...action,
      run: async () => {
        await action.run();
        after();
      },
    },
  };
  return true;
}

export function dismissToast() {
  if (timer) clearTimeout(timer);
  toasts.current = null;
}

/** Toast eylemini çalıştırır (Ctrl Z de bunu çağırır). */
export async function runToastAction(): Promise<boolean> {
  const a = toasts.current?.action;
  if (!a) return false;
  dismissToast();
  await a.run();
  return true;
}
