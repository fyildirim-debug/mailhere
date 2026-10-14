import { describe, expect, it } from 'vitest';
import { createMockBackend } from './mock';
import { SAVED_SEARCHES_MAX, TAG_COLORS, type MessageSummary, type Page } from './types';

// Sahte arka ucun liste özellikleri: sabitleme, etiketler, göndericiye göre
// seçim ve kayıtlı aramalar arka uçla aynı sözleşmeyi izler.

const NOW = new Date(2026, 9, 13, 10, 0);
const inbox = { view: { kind: 'inbox' as const }, accountId: null, category: null, cursor: null, limit: 100 };

async function fail(p: Promise<unknown>): Promise<{ code: string; message: string }> {
  try {
    await p;
  } catch (e) {
    return e as { code: string; message: string };
  }
  throw new Error('hata bekleniyordu');
}

describe('sabitleme', () => {
  it('gelen kutusunda sabitlenenler önce gelir, imleç tekrarsız sürer', async () => {
    const b = createMockBackend({ now: NOW });
    const all = (await b.call('messages_list', { query: inbox })).items;
    const oldest = all[all.length - 1];
    const middle = all[Math.floor(all.length / 2)];
    await b.call('messages_pin', { ids: [oldest.id, middle.id], pinned: true });
    const page = (await b.call('messages_list', { query: inbox })).items;
    expect(page[0].pinned && page[1].pinned).toBe(true);
    expect(page.slice(0, 2).map((m) => m.id).sort()).toEqual([oldest.id, middle.id].sort());
    expect(page.slice(2).some((m) => m.pinned)).toBe(false);
    // Yıldızlılar gibi görünümlerde sıra değişmez.
    await b.call('messages_flag', { ids: [oldest.id], unread: null, starred: true });
    const starred = (await b.call('messages_list', { query: { ...inbox, view: { kind: 'starred' } } })).items;
    expect(starred[0].id).not.toBe(oldest.id);

    // Sayfalı: üç parçalı imleç, tekrar yok, tüm postalar gelir.
    const seen: number[] = [];
    let cursor: string | null = null;
    do {
      const p: Page<MessageSummary> = await b.call('messages_list', { query: { ...inbox, limit: 3, cursor } });
      seen.push(...p.items.map((m) => m.id));
      cursor = p.nextCursor;
      if (cursor) expect(cursor.split(':')).toHaveLength(3);
    } while (cursor);
    expect(new Set(seen).size).toBe(seen.length);
    expect(seen.length).toBe(all.length);
    expect(seen.slice(0, 2).sort()).toEqual([oldest.id, middle.id].sort());

    await b.call('messages_pin', { ids: [oldest.id, middle.id], pinned: false });
    expect((await b.call('messages_list', { query: inbox })).items.some((m) => m.pinned)).toBe(false);
  });
});

describe('etiketler', () => {
  it('eklenir, denetlenir, postaya takılır, görünüm ve arama süzer, silinince kalkar', async () => {
    const b = createMockBackend({ now: NOW });
    expect((await b.call('tags_list')).map((x) => x.name)).toEqual(['Fatura', 'Proje']);
    const acil = await b.call('tag_save', { tag: { id: '', name: '  Acil \t ', color: TAG_COLORS[4].toLowerCase() } });
    expect(acil.id).not.toBe('');
    expect(acil.name).toBe('Acil');
    expect(acil.color).toBe(TAG_COLORS[4]);
    expect((await fail(b.call('tag_save', { tag: { id: '', name: 'acil', color: TAG_COLORS[0] } }))).message).toBe('Bu adda bir etiket zaten var.');
    expect((await fail(b.call('tag_save', { tag: { id: '', name: ' ', color: TAG_COLORS[0] } }))).message).toBe('Etikete bir ad ver.');
    expect((await fail(b.call('tag_save', { tag: { id: '', name: 'Yeni', color: '#000000' } }))).message).toBe('Etiket rengi paletten seçilmeli.');
    expect((await fail(b.call('tag_save', { tag: { id: 'yok', name: 'Yeni', color: TAG_COLORS[0] } }))).code).toBe('notFound');
    // Yeniden adlandırma kendi adıyla çakışmaz.
    const renamed = await b.call('tag_save', { tag: { ...acil, name: 'Acil', color: TAG_COLORS[1] } });
    expect(renamed.color).toBe(TAG_COLORS[1]);

    const [m] = (await b.call('messages_list', { query: inbox })).items;
    await b.call('messages_tag', { ids: [m.id], tagId: acil.id, on: true });
    await b.call('messages_tag', { ids: [m.id], tagId: acil.id, on: true });
    expect((await b.call('message_get', { id: m.id })).tags).toEqual([acil.id]);
    const view = (await b.call('messages_list', { query: { ...inbox, view: { kind: 'tag', id: acil.id } } })).items;
    expect(view.map((x) => x.id)).toEqual([m.id]);
    // Arama işleci: tam ad, önek, aksansız.
    expect((await b.call('search', { q: 'tag:acil', limit: 10 })).map((x) => x.id)).toEqual([m.id]);
    expect((await b.call('search', { q: 'etiket:fat', limit: 10 })).map((x) => x.subject)).toEqual(['Ekim faturanız hazır']);
    expect(await b.call('search', { q: 'tag:yok', limit: 10 })).toEqual([]);
    expect((await fail(b.call('messages_tag', { ids: [m.id], tagId: 'yok', on: true }))).message).toBe('Etiket bulunamadı; silinmiş olabilir.');

    await b.call('messages_tag', { ids: [m.id], tagId: acil.id, on: false });
    expect((await b.call('message_get', { id: m.id })).tags).toEqual([]);
    await b.call('messages_tag', { ids: [m.id], tagId: acil.id, on: true });
    await b.call('tag_delete', { id: acil.id });
    expect((await b.call('tags_list')).some((x) => x.id === acil.id)).toBe(false);
    expect((await b.call('message_get', { id: m.id })).tags).toEqual([]);
  });

  it('kural eylemi etiket ekler; silinmiş etiket kurala yazılamaz', async () => {
    const b = createMockBackend({ now: NOW });
    const rule = await b.call('rule_save', {
      rule: {
        id: '', name: 'GitHub etiketi', enabled: true, accountId: null, match: 'all',
        conditions: [{ kind: 'from', op: 'contains', value: 'github.com' }], actions: [{ kind: 'tag', tagId: 'etiket-proje' }], stop: false,
      },
    });
    expect(await b.call('rule_run', { id: rule.id })).toBe(1);
    const hit = (await b.call('search', { q: 'tag:proje', limit: 10 }))[0];
    expect(hit.from.email).toBe('notifications@github.com');
    const e = await fail(b.call('rule_save', { rule: { ...rule, id: '', actions: [{ kind: 'tag', tagId: 'yok' }] } }));
    expect(e.message).toBe('Eklenecek etiket bulunamadı; silinmiş olabilir. Başka bir etiket seç.');
  });
});

describe('göndericiye göre postalar', () => {
  it('kapsam gelen kutusu, klasör ya da etiket; adres tam eşleşir', async () => {
    const b = createMockBackend({ now: NOW });
    const kerem = 'kerem.aydin@ornek.com.tr';
    const inInbox = await b.call('messages_by_sender', { accountId: 'demo-is', email: kerem.toUpperCase(), view: { kind: 'inbox' } });
    // Kerem'in gelen kutusundaki dört postası (bütçe yanıtları, davet, yayın planı), yeniden eskiye.
    expect(inInbox).toHaveLength(4);
    const dates = await Promise.all(inInbox.map(async (id) => (await b.call('message_get', { id })).date));
    expect([...dates].sort((x, y) => y - x)).toEqual(dates);
    expect(await b.call('messages_by_sender', { accountId: 'demo-kisisel', email: kerem, view: null })).toEqual([]);
    expect(await b.call('messages_by_sender', { accountId: null, email: 'kerem', view: null })).toEqual([]);
    const folder = await b.call('messages_by_sender', { accountId: null, email: kerem, view: { kind: 'mailbox', id: 'demo-is:Projeler/Mailhere' } });
    expect(folder).toHaveLength(1);
    const tagged = await b.call('messages_by_sender', { accountId: null, email: 'fatura@bulutfatura.com.tr', view: { kind: 'tag', id: 'etiket-fatura' } });
    expect(tagged).toHaveLength(1);
  });
});

describe('kayıtlı aramalar', () => {
  it('ayarlarda sadeleşir: boş sorgu atılır, ad kırpılır, en çok 30', async () => {
    const b = createMockBackend({ now: NOW });
    const settings = await b.call('settings_get');
    expect(settings.savedSearches).toEqual([]);
    const many = Array.from({ length: 40 }, (_, i) => ({ id: '', name: `A${i}`, query: `q${i}` }));
    const saved = await b.call('settings_set', {
      settings: {
        ...settings,
        savedSearches: [
          { id: '', name: '  Elif\tekli ', query: ' from:elif   has:ek ' },
          { id: '', name: '', query: 'is:okunmamış' },
          { id: 'k', name: 'Boş', query: '   ' },
          { id: 'k2', name: 'x'.repeat(100), query: 'y'.repeat(400) },
          ...many,
        ],
      },
    });
    const list = saved.savedSearches;
    expect(list).toHaveLength(SAVED_SEARCHES_MAX);
    expect(list[0]).toMatchObject({ name: 'Elif ekli', query: 'from:elif has:ek' });
    expect(list[0].id).not.toBe('');
    expect(list[1].name).toBe('is:okunmamış');
    expect(list[2].name).toHaveLength(60);
    expect(list[2].query).toHaveLength(300);
    expect(list.some((s) => s.name === 'Boş')).toBe(false);
  });
});
