// Yazma ekranını açma. Masaüstünde arka uç ayrı pencere açar (`open_compose`);
// mobil kabuk açılışta kendi yığınını bağlar ve yazma ekranını iter (sahte
// arka uçta `open_compose` yeni sekme açar; mobilde kullanılmaz).

import { call, type ComposePrefill } from '../api';

let opener: ((prefill: ComposePrefill) => void) | null = null;

export function bindComposeOpener(fn: ((prefill: ComposePrefill) => void) | null) {
  opener = fn;
}

export function openCompose(prefill: ComposePrefill) {
  if (opener) opener(prefill);
  else void call('open_compose', { prefill });
}
