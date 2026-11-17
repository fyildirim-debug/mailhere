// Menülerde ok tuşlarıyla gezinme (`Menu.svelte` ile aynı davranış): ↑ / ↓
// etkin düğmeler arasında döner, Home / End başa ve sona gider, Enter odaktaki
// seçeneği seçer. Kendi satırlarını çizen menüler (ertele, yanıt hatırlatması,
// sonra gönder) bunu kullanır.

const KEYS = ['ArrowDown', 'ArrowUp', 'Home', 'End'];

/** Sıradaki öğenin sırası; `current` listede değilse (-1) ↓ ilk, ↑ son öğeye gider. */
export function nextMenuIndex(key: string, current: number, count: number): number | null {
  if (!KEYS.includes(key) || count <= 0) return null;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  if (current < 0) return key === 'ArrowDown' ? 0 : count - 1;
  return (current + (key === 'ArrowDown' ? 1 : -1) + count) % count;
}

/**
 * Tuşu menü gezinmesi olarak işler; işlediyse true. Girdideyken (tarih-saat
 * alanı ok tuşlarını kendi kullanır) karışmaz.
 *
 * Enter odaktaki düğmeyi burada çalıştırır ve olayı tüketir: ana pencerede
 * Enter "seçili postayı aç" kısayoludur ve varsayılan davranışı (düğmenin
 * tıklanmasını) engellerdi; menüdeyken seçenek seçilmeli.
 */
export function menuKeys(e: KeyboardEvent, root: HTMLElement | null | undefined): boolean {
  if (!root) return false;
  const target = e.target as HTMLElement | null;
  const tag = target?.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return false;
  if (e.key === 'Enter') {
    if (tag !== 'BUTTON' || e.ctrlKey || e.metaKey || e.altKey || e.repeat) return false;
    e.preventDefault();
    (target as HTMLButtonElement).click();
    return true;
  }
  if (!KEYS.includes(e.key)) return false;
  const list = [...root.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
  const next = nextMenuIndex(e.key, list.indexOf(document.activeElement as HTMLButtonElement), list.length);
  if (next == null) return false;
  e.preventDefault();
  list[next].focus();
  return true;
}
