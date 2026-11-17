// Onay penceresinin durumu: geri alınamayan eylemden (kalıcı silme, kutuyu
// boşaltma) önce `askConfirm` sorar, pencereyi `ConfirmHost.svelte` çizer.
// Aynı anda tek soru vardır; yenisi gelirse eskisi "vazgeç" sayılır.

import type { IconName } from './icons';

export interface ConfirmRequest {
  title: string;
  /** Ne olacağı ve geri alınıp alınamayacağı; düz metin. */
  body: string;
  /** Onay düğmesi yaptığı işi söyler ("Kalıcı sil"). */
  confirmLabel: string;
  /** Vazgeç düğmesinin metni; verilmezse "Vazgeç". */
  cancelLabel?: string;
  icon?: IconName;
  /** Geri alınamayan eylem: rozet ve onay düğmesi `--danger`. */
  danger?: boolean;
}

interface Pending extends ConfirmRequest {
  key: number;
  resolve: (ok: boolean) => void;
}

let seq = 0;

export const confirmState = $state({ current: null as Pending | null });

/** Onay penceresini açar; onaylanırsa `true`, vazgeçilirse (Esc, dışarı tık, geri tuşu) `false`. */
export function askConfirm(request: ConfirmRequest): Promise<boolean> {
  confirmState.current?.resolve(false);
  return new Promise((resolve) => {
    confirmState.current = { ...request, key: ++seq, resolve };
  });
}

/** Açık soruyu yanıtlar (pencere ve testler). */
export function answerConfirm(ok: boolean) {
  const cur = confirmState.current;
  if (!cur) return;
  confirmState.current = null;
  cur.resolve(ok);
}
