// Doğrulama kodu: okunur gösterim ve panoya kopyalama (okuma alanı ve bildirim kartı).

/** Rakamlı kodu öbeklere ayırır: 6 hane "482 913", 8 hane "1234 5678"; diğerleri olduğu gibi. */
export function groupCode(code: string): string {
  if (!/^\d+$/.test(code)) return code;
  if (code.length === 6) return `${code.slice(0, 3)} ${code.slice(3)}`;
  if (code.length === 8) return `${code.slice(0, 4)} ${code.slice(4)}`;
  return code;
}

/**
 * Metni panoya yazar. Pano API'si yoksa ya da reddederse (odaksız pencere,
 * eski WebView) seçili metin yoluyla dener; ikisi de olmazsa `false`.
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // aşağıdaki yedek yol
  }
  if (typeof document === 'undefined') return false;
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  area.style.position = 'fixed';
  area.style.opacity = '0';
  document.body.appendChild(area);
  area.select();
  try {
    return document.execCommand('copy');
  } catch {
    return false;
  } finally {
    area.remove();
  }
}
