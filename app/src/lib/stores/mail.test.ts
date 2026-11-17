import { beforeEach, describe, expect, it } from 'vitest';
import { createMockBackend } from '../api/mock';
import { useBackend } from '../api';
import type { Account, Address } from '../api/types';
import { dismissToast, runToastAction } from '../ui/toast.svelte';
import { bulkMove } from './bulk.svelte';
import { currentRows, mail, moveTo, quickReplyTargets, refreshList, setCategory, setOnly } from './mail.svelte';

const me: Address = { name: 'Furkan', email: 'furkan@ornek.com.tr' };
const kerem: Address = { name: 'Kerem Aydın', email: 'kerem.aydin@ornek.com.tr' };
const ayse: Address = { name: 'Ayşe Demir', email: 'ayse.demir@eposta.com' };
const account = { id: 'demo-is', email: 'Furkan@Ornek.com.tr' } as Account;

describe('hızlı yanıt alıcıları', () => {
  it('gelen postada Reply-To, yoksa gönderen', () => {
    expect(quickReplyTargets({ accountId: 'demo-is', from: kerem, replyTo: [], to: [me, ayse] }, account)).toEqual([kerem]);
    expect(quickReplyTargets({ accountId: 'demo-is', from: kerem, replyTo: [ayse], to: [me] }, account)).toEqual([ayse]);
  });

  it('kendi gönderdiğimiz postada (Yanıt bekleniyor) alıcılara gider, kendimize değil', () => {
    expect(quickReplyTargets({ accountId: 'demo-is', from: me, replyTo: [], to: [kerem, ayse] }, account)).toEqual([kerem, ayse]);
    // Alıcılar arasında kendi adresimiz de varsa çıkar.
    expect(quickReplyTargets({ accountId: 'demo-is', from: me, replyTo: [], to: [me, kerem] }, account)).toEqual([kerem]);
    // Reply-To içinde biz ve başkası: yalnız başkası.
    expect(quickReplyTargets({ accountId: 'demo-is', from: kerem, replyTo: [me, ayse], to: [me] }, account)).toEqual([ayse]);
  });

  it('kimse kalmazsa yanıt adresleri; hesap verilmezse postanın hesabı bulunur', () => {
    expect(quickReplyTargets({ accountId: 'demo-is', from: me, replyTo: [], to: [me] }, account)).toEqual([me]);
    mail.accounts = [account];
    expect(quickReplyTargets({ accountId: 'demo-is', from: me, replyTo: [], to: [kerem] })).toEqual([kerem]);
    // Hesap bilinmiyorsa süzgeç uygulanmaz.
    expect(quickReplyTargets({ accountId: 'yok', from: me, replyTo: [], to: [kerem] })).toEqual([me]);
  });
});

describe('çoklu seçim listeyle birlikte budanır', () => {
  beforeEach(() => {
    useBackend(createMockBackend());
    mail.view = { kind: 'inbox' };
    mail.accountId = null;
    mail.search = null;
    mail.category = null;
    mail.only = null;
    mail.checked = [];
  });

  it('liste yenilenince görünmeyen işaretler düşer', async () => {
    await refreshList();
    const [a, b] = currentRows().map((r) => r.id);
    mail.checked = [a, b, 999_999];
    await refreshList();
    expect(mail.checked).toEqual([a, b]);
  });

  it('kategori ya da süzgeç değişince işaretler temizlenir', async () => {
    await refreshList();
    mail.checked = currentRows().slice(0, 2).map((r) => r.id);
    setCategory('newsletters');
    expect(mail.checked).toEqual([]);
    await refreshList();
    mail.checked = currentRows().slice(0, 2).map((r) => r.id);
    setOnly('unread');
    expect(mail.checked).toEqual([]);
    expect(mail.only).toBe('unread');
  });
});

describe('sil / arşivle → geri al postayı geldiği klasöre döndürür', () => {
  let backend: ReturnType<typeof createMockBackend>;

  beforeEach(() => {
    backend = createMockBackend();
    useBackend(backend);
    mail.view = { kind: 'inbox' };
    mail.accountId = null;
    mail.search = null;
    mail.category = null;
    mail.only = null;
    mail.checked = [];
    mail.selectedId = null;
    mail.detail = null;
    dismissToast();
  });

  const boxOf = async (id: number) => (await backend.call('message_get', { id })).mailboxId;

  it('Gönderilenlerden silinen posta Gönderilenlere döner, gelen kutusuna değil', async () => {
    mail.view = { kind: 'sent' };
    await refreshList();
    const m = mail.items[0];
    expect(m).toBeDefined();
    const sentBox = m.mailboxId;

    await moveTo([m.id], 'trash');
    expect(await boxOf(m.id)).not.toBe(sentBox);
    expect(mail.items.some((x) => x.id === m.id)).toBe(false);

    expect(await runToastAction()).toBe(true);
    expect(await boxOf(m.id)).toBe(sentBox);
    expect(mail.items.some((x) => x.id === m.id)).toBe(true);
  });

  /** Depoya uğramadan (okuma alanı eski klasörü hatırlamasın) gelen kutusunun ilk postası. */
  async function firstInInbox() {
    const page = await backend.call('messages_list', {
      query: { view: { kind: 'inbox' }, accountId: null, category: null, only: null, cursor: null, limit: 1 },
    });
    return page.items[0];
  }

  /** Sahte arka uçta özel klasör yok; klasör görünümü için hesabın Arşiv klasörü açılır. */
  async function folderWith(id: number, accountId: string) {
    const boxes = await backend.call('mailboxes_list');
    const box = boxes.find((b) => b.accountId === accountId && b.role === 'archive')!;
    await backend.call('messages_move_to', { ids: [id], mailboxId: box.id });
    return box;
  }

  it('klasör görünümünden silinen posta aynı klasöre döner', async () => {
    const m = await firstInInbox();
    const box = await folderWith(m.id, m.accountId);

    mail.view = { kind: 'mailbox', id: box.id };
    await refreshList();
    expect(mail.items.some((x) => x.id === m.id)).toBe(true);

    await moveTo([m.id], 'trash');
    expect(await boxOf(m.id)).not.toBe(box.id);
    await runToastAction();
    expect(await boxOf(m.id)).toBe(box.id);
  });

  it('listede ve okumada olmayan ileti klasör görünümünde o klasöre, başka görünümde gelen kutusuna döner', async () => {
    const m = await firstInInbox();
    const inbox = m.mailboxId;
    const box = await folderWith(m.id, m.accountId);

    // Klasör görünümü, liste boş: kaynağı görünümden bilinir.
    mail.view = { kind: 'mailbox', id: box.id };
    mail.items = [];
    await moveTo([m.id], 'trash');
    await runToastAction();
    expect(await boxOf(m.id)).toBe(box.id);

    // Kaynağı bilinmeyen: gelen kutusu.
    mail.view = { kind: 'starred' };
    mail.items = [];
    mail.selectedId = null;
    mail.detail = null;
    await moveTo([m.id], 'trash');
    await runToastAction();
    expect(await boxOf(m.id)).toBe(inbox);
  });

  it('toplu silme de aynı yardımcıyla geri döner', async () => {
    mail.view = { kind: 'sent' };
    await refreshList();
    const picked = mail.items.slice(0, 2);
    expect(picked.length).toBe(2);
    const before = picked.map((m) => m.mailboxId);
    await bulkMove(picked.map((m) => m.id), 'trash');
    await runToastAction();
    expect(await Promise.all(picked.map((m) => boxOf(m.id)))).toEqual(before);
  });
});
