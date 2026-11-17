import { beforeEach, describe, expect, it } from 'vitest';
import { createMockBackend } from '../api/mock';
import { useBackend } from '../api';
import { dismissToast, toasts } from '../ui/toast.svelte';
import { loadOlder, older, olderAvailable, olderDone, otpFresh, OTP_CHIP_MS, purgeRole } from './guard.svelte';
import { loadMore, mail, refreshList, reloadMailboxes } from './mail.svelte';

describe('kod çipinin tazeliği', () => {
  it('son 60 dakikadaki posta taze; eskisi ve saati ileri kaymış posta değil', () => {
    const now = 1_800_000_000_000;
    expect(otpFresh(now - 5 * 60_000, now)).toBe(true);
    expect(otpFresh(now - OTP_CHIP_MS, now)).toBe(true);
    expect(otpFresh(now - OTP_CHIP_MS - 1, now)).toBe(false);
    // Gönderenin saati biraz ileriyse hoş görülür; çok ileriyse değil.
    expect(otpFresh(now + 60_000, now)).toBe(true);
    expect(otpFresh(now + 6 * 60_000, now)).toBe(false);
  });
});

describe('daha eski postalar ve liste derinliği', () => {
  let backend: ReturnType<typeof createMockBackend>;

  beforeEach(async () => {
    backend = createMockBackend();
    useBackend(backend);
    mail.view = { kind: 'archive' };
    mail.accountId = null;
    mail.search = null;
    mail.category = null;
    mail.only = null;
    mail.checked = [];
    mail.selectedId = null;
    mail.detail = null;
    older.busy = false;
    older.done = false;
    older.key = '';
    await reloadMailboxes();
    await refreshList(false);
    dismissToast();
  });

  it('yerel liste bitince istenir; inen postalar listeye eklenir, sunucu bitince "hepsi bu kadar"', async () => {
    expect(olderAvailable()).toBe(true);
    expect(olderDone()).toBe(false);
    const before = mail.items.length;
    await loadOlder();
    expect(mail.items.length).toBeGreaterThan(before);
    expect(mail.items.some((m) => m.subject.startsWith('Eski yazışma'))).toBe(true);
    expect(older.busy).toBe(false);
    // Sahte sunucuda kutu başına iki parti var.
    expect(olderDone()).toBe(false);
    expect(olderAvailable()).toBe(true);
    const mid = mail.items.length;
    await loadOlder();
    expect(mail.items.length).toBeGreaterThan(mid);
    expect(olderDone()).toBe(true);
    expect(olderAvailable()).toBe(false);
  });

  it('arama, süzgeç ve sanal görünümlerde ne düğme ne "hepsi bu kadar" vardır', async () => {
    mail.only = 'unread';
    expect(olderAvailable()).toBe(false);
    mail.only = null;
    mail.search = 'fatura';
    expect(olderAvailable()).toBe(false);
    mail.search = null;
    mail.view = { kind: 'starred' };
    await refreshList(false);
    expect(olderAvailable()).toBe(false);
    older.done = true;
    older.key = JSON.stringify([mail.view, mail.accountId]);
    expect(olderDone()).toBe(false);
  });

  it('sunucuda daha eskisi yoksa söylenir', async () => {
    await loadOlder();
    await loadOlder();
    dismissToast();
    older.done = false;
    await loadOlder();
    expect(toasts.current?.text).toBeTruthy();
    expect(olderDone()).toBe(true);
  });

  it('yenileme yüklenmiş derinliği korur: aşağı kaydırılmış liste ilk sayfaya kısalmaz', async () => {
    // Gelen kutusuna 250 posta daha koy: üç sayfa.
    const sample = backend.state.messages.find((m) => m.mailboxId.endsWith('INBOX'))!;
    let id = Math.max(...backend.state.messages.map((m) => m.id)) + 1;
    for (let i = 0; i < 250; i++) {
      backend.state.messages.push({ ...structuredClone(sample), id: id++, threadId: `derin-${i}`, subject: `Derin ${i}`, date: sample.date - (i + 1) * 60_000, unread: false, pinned: false });
    }
    mail.view = { kind: 'inbox' };
    await refreshList(false);
    expect(mail.items).toHaveLength(100);
    expect(mail.nextCursor).not.toBeNull();
    await loadMore();
    await loadMore();
    const deep = mail.items.length;
    expect(deep).toBeGreaterThan(200);
    // Yenileme (yeni posta, bayrak değişimi) derinliği korur, sıra aynı kalır.
    const order = mail.items.map((m) => m.id);
    await refreshList();
    expect(mail.items.length).toBeGreaterThanOrEqual(deep);
    expect(mail.items.slice(0, deep).map((m) => m.id)).toEqual(order);
    // Görünüm değişimi (keepSelection = false) yine ilk sayfayla başlar.
    await refreshList(false);
    expect(mail.items).toHaveLength(100);
  });
});

describe('kalıcı silme görünümleri', () => {
  it('Çöp görünümü ve Çöp / İstenmeyen klasörü kalıcı silmeye açıktır', async () => {
    const backend = createMockBackend();
    useBackend(backend);
    await reloadMailboxes();
    const junk = mail.mailboxes.find((b) => b.role === 'junk')!;
    const trash = mail.mailboxes.find((b) => b.role === 'trash')!;
    const other = mail.mailboxes.find((b) => b.role === 'other')!;
    expect(purgeRole({ kind: 'trash' })).toBe('trash');
    expect(purgeRole({ kind: 'mailbox', id: junk.id })).toBe('junk');
    expect(purgeRole({ kind: 'mailbox', id: trash.id })).toBe('trash');
    expect(purgeRole({ kind: 'mailbox', id: other.id })).toBeNull();
    expect(purgeRole({ kind: 'inbox' })).toBeNull();
    expect(purgeRole({ kind: 'archive' })).toBeNull();
  });
});
