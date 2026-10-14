// Kabuk seçimi: masaüstü (pencereler, kenar çubuğu) ya da mobil (yığın, çekmece).
// Derlemede `__MH_SHELL__` (vite.config.ts) karar verir; tarayıcıda ve testlerde
// `?kabuk=mobil` sorgusu mobil kabuğu açar.

export type Shell = 'desktop' | 'mobile';

function pick(): Shell {
  if (typeof location !== 'undefined' && new URLSearchParams(location.search).get('kabuk') === 'mobil') return 'mobile';
  return typeof __MH_SHELL__ === 'string' ? __MH_SHELL__ : 'desktop';
}

export const shell: Shell = pick();
export const isMobileShell = shell === 'mobile';
