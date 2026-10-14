// Sessiz kaydırma çubuğu (`.scroll-quiet`, global.css): çubuk yalnız üzerine
// gelince ya da kaydırırken görünür. Bu eylem kaydırma sırasında `.scrolling`
// sınıfını ekler, kısa bir duraksamadan sonra kaldırır (dokunmatikte hover yok).

import type { Action } from 'svelte/action';

const IDLE_MS = 700;

export const quietScroll: Action<HTMLElement> = (node) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const onScroll = () => {
    node.classList.add('scrolling');
    clearTimeout(timer);
    timer = setTimeout(() => node.classList.remove('scrolling'), IDLE_MS);
  };
  node.addEventListener('scroll', onScroll, { passive: true });
  return {
    destroy() {
      clearTimeout(timer);
      node.removeEventListener('scroll', onScroll);
    },
  };
};
