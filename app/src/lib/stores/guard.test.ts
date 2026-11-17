import { beforeEach, describe, expect, it } from 'vitest';
import { createMockBackend } from '../api/mock';
import { useBackend, type OlderResult } from '../api';
import { t } from '../i18n/index.svelte';
import { dismissToast, toasts } from '../ui/toast.svelte';
import { emptyMailbox, loadOlder, older, olderAvailable, olderDone, olderDoneHere, otpFresh, OTP_CHIP_MS, purgeRole } from './guard.svelte';
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
    expect(olderDoneHere()).toBe(true);
    expect(olderAvailable()).toBe(false);
    // Durum görünüme aittir: başka görünümün anahtarıyla "burada bitti" sayılmaz.
    mail.view = { kind: 'sent' };
    expect(olderDoneHere()).toBe(false);
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
    expect(toasts.current?.text).toBe(t('older.none'));
    expect(olderDone()).toBe(true);
  });

  /** `messages_older` yanıtı sabitlenmiş sahte arka uç. */
  function olderReturns(result: OlderResult) {
    useBackend({
      call: ((name: string, args: unknown) => (name === 'messages_older' ? Promise.resolve(result) : backend.call(name as never, args as never))) as typeof backend.call,
      on: backend.on.bind(backend),
    });
  }

  it('posta inmedi ama sunucuda daha eskisi var: düğme kalır, "biraz sonra yeniden dene" denir', async () => {
    olderReturns({ fetched: 0, more: true });
    const before = mail.items.length;
    await loadOlder();
    expect(toasts.current?.text).toBe(t('older.retry'));
    expect(mail.items).toHaveLength(before);
    expect(older.busy).toBe(false);
    expect(olderDone()).toBe(false);
    expect(olderAvailable()).toBe(true);
  });

  it('posta inmedi ve sunucuda da kalmadı: "daha eski posta yok" denir, liste sonu "hepsi bu kadar"', async () => {
    olderReturns({ fetched: 0, more: false });
    await loadOlder();
    expect(toasts.current?.text).toBe(t('older.none'));
    expect(olderDone()).toBe(true);
    expect(olderAvailable()).toBe(false);
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

describe('kutuyu boşaltma kapsamı', () => {
  it('klasör görünümünde yalnız o klasörün hesabı boşalır; Çöp görünümünde kapsamdaki hesaplar', async () => {
    const backend = createMockBackend();
    useBackend(backend);
    await reloadMailboxes();
    const roleOf = (id: string) => mail.mailboxes.find((b) => b.id === id)?.role ?? 'other';
    /** Hesabın Çöp ve İstenmeyen dışındaki bir postasını bu kutuya koyar; hesapta posta yoksa false. */
    const fill = (box: { id: string; accountId: string }) => {
      const m = backend.state.messages.find((x) => x.accountId === box.accountId && !['trash', 'junk'].includes(roleOf(x.mailboxId)));
      if (m) m.mailboxId = box.id;
      return m != null;
    };
    // Postası olan her hesabın İstenmeyen kutusuna bir posta koy.
    const junks = mail.mailboxes.filter((b) => b.role === 'junk').filter(fill);
    expect(junks.length).toBeGreaterThan(1);
    const count = (id: string) => backend.state.messages.filter((m) => m.mailboxId === id).length;
    mail.accountId = null;
    mail.view = { kind: 'mailbox', id: junks[0].id };
    expect(await emptyMailbox('junk')).toBe(true);
    expect(count(junks[0].id)).toBe(0);
    for (const other of junks.slice(1)) expect(count(other.id)).toBeGreaterThan(0);

    // Çöp görünümü, hesap kapsamı yokken tüm hesapların çöpünü boşaltır.
    const trashes = mail.mailboxes.filter((b) => b.role === 'trash').filter(fill);
    expect(trashes.length).toBeGreaterThan(1);
    mail.view = { kind: 'trash' };
    expect(await emptyMailbox('trash')).toBe(true);
    for (const box of trashes) expect(count(box.id)).toBe(0);
    dismissToast();
  });
});
