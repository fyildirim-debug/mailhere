// Pencere içi geri bildirim (alt ortada, ters renkli, "Geri al" eylemli).

export interface Toast {
  id: number;
  text: string;
  action?: { label: string; run: () => void | Promise<void> };
  /** Milisaniye; varsayılan 5 sn. */
  duration?: number;
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
