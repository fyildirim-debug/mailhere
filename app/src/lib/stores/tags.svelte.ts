// Yerel etiketler: liste arka uçtan bir kez okunur, değişince yenilenir.
// Etiketler sunucuya yazılmaz; yalnız bu bilgisayardadır.

import { call, type Tag } from '../api';

export const tagStore = $state({ list: [] as Tag[], ready: false });

export async function loadTags() {
  try {
    tagStore.list = await call('tags_list');
  } catch {
    /* etiketler yenilenemezse eski liste kalır */
  } finally {
    tagStore.ready = true;
  }
}

export function tagById(id: string): Tag | undefined {
  return tagStore.list.find((x) => x.id === id);
}

/** Kimlik listesini etiket sırasına göre etiketlere çevirir; silinmişler atlanır. */
export function tagsOf(ids: string[]): Tag[] {
  return tagStore.list.filter((x) => ids.includes(x.id));
}

/** Kaydeder (id boşsa yeni) ve listeyi yeniler; sadeleştirilmiş etiketi döner. */
export async function saveTag(tag: Tag): Promise<Tag> {
  const saved = await call('tag_save', { tag });
  await loadTags();
  return saved;
}

/** Siler ve listeyi yeniler; etiket postalardan da kalkar. */
export async function deleteTag(id: string) {
  await call('tag_delete', { id });
  await loadTags();
}
