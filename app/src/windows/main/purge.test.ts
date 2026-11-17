import { beforeEach, describe, expect, it } from 'vitest';
import { createMockBackend } from '../../lib/api/mock';
import { useBackend } from '../../lib/api';
import { mail, refreshList, reloadMailboxes } from '../../lib/stores/mail.svelte';
import { answerConfirm, confirmState } from '../../lib/ui/confirm.svelte';
import { dismissToast, toasts } from '../../lib/ui/toast.svelte';
import { emptyConfirmed, emptyTarget, purgeable, purgeBox, removeMessages } from './purge';

describe('kalıcı silme (masaüstü)', () => {
  let backend: ReturnType<typeof createMockBackend>;

  beforeEach(async () => {
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
    mail.accounts = await backend.call('accounts_list');
    await reloadMailboxes();
    await refreshList();
    answerConfirm(false);
    dismissToast();
  });

  const exists = (id: number) => backend.state.messages.some((m) => m.id === id);
  const roleOf = (id: number) => {
    const m = backend.state.messages.find((x) => x.id === id);
    return backend.state.mailboxes.find((b) => b.id === m?.mailboxId)?.role;
  };

  /** Gelen kutusunun ilk `n` postasını çöpe taşır ve Çöp görünümünü açar. */
  async function trashSome(n: number): Promise<number[]> {
    const ids = mail.items.slice(0, n).map((m) => m.id);
    await backend.call('messages_move', { ids, target: 'trash' });
    await reloadMailboxes();
    mail.view = { kind: 'trash' };
    await refreshList(false);
    return ids;
  }

  /** Açılan onayı bekler ve yanıtlar. */
  async function answer(ok: boolean) {
    for (let i = 0; i < 20 && !confirmState.current; i++) await Promise.resolve();
    expect(confirmState.current).not.toBeNull();
    answerConfirm(ok);
  }

  it('yalnız Çöp ve İstenmeyen kutusundaki postalar kalıcı silinir sayılır', async () => {
    const inbox = mail.items.slice(0, 2).map((m) => m.id);
    expect(purgeable(inbox)).toBe(false);
    expect(purgeable([])).toBe(false);
    const junk = backend.state.mailboxes.find((b) => b.role === 'junk')!;
    expect(purgeBox(junk.id)).toBe(true);
    expect(purgeBox(mail.items[0].mailboxId)).toBe(false);
    expect(purgeBox(null)).toBe(false);

    const trashed = await trashSome(2);
    expect(purgeable(trashed)).toBe(true);
    // Listede olmayan posta görünüme göre sayılır: Çöp görünümünde kalıcı, aramada değil.
    expect(purgeable([987_654])).toBe(true);
    mail.search = 'fatura';
    expect(purgeable([987_654])).toBe(false);
  });

  it('gelen kutusunda Sil çöpe taşır ve onay sormaz', async () => {
    const id = mail.items[0].id;
    await removeMessages([id]);
    expect(confirmState.current).toBeNull();
    expect(roleOf(id)).toBe('trash');
    expect(toasts.current?.action).toBeDefined();
  });

  it('Çöpte Sil onay ister: vazgeçince posta kalır, onaylayınca sunucudan da silinir', async () => {
    const [a, b] = await trashSome(2);
    const cancelled = removeMessages([a]);
    await answer(false);
    await cancelled;
    expect(exists(a)).toBe(true);
    expect(mail.items.some((m) => m.id === a)).toBe(true);

    const done = removeMessages([a, b], { bulk: true });
    expect(confirmState.current).toMatchObject({ danger: true });
    await answer(true);
    await done;
    expect(exists(a)).toBe(false);
    expect(exists(b)).toBe(false);
    expect(mail.items.some((m) => m.id === a || m.id === b)).toBe(false);
    // Kalıcı silmede "Geri al" yoktur.
    expect(toasts.current?.action).toBeUndefined();
  });

  it('boşaltılacak kutu: Çöp görünümü kapsamdaki hesapları, klasör görünümü yalnız o kutuyu boşaltır', async () => {
    expect(emptyTarget()).toBeNull();
    await trashSome(3);
    expect(emptyTarget()).toMatchObject({ role: 'trash', accountId: null, count: 3 });
    mail.search = 'fatura';
    expect(emptyTarget()).toBeNull();
    mail.search = null;

    const junk = backend.state.mailboxes.find((b) => b.role === 'junk' && backend.state.messages.some((m) => m.mailboxId === b.id))!;
    mail.view = { kind: 'mailbox', id: junk.id };
    await refreshList(false);
    const target = emptyTarget()!;
    expect(target).toMatchObject({ role: 'junk', accountId: junk.accountId });
    expect(target.count).toBe(backend.state.messages.filter((m) => m.mailboxId === junk.id).length);

    const done = emptyConfirmed();
    await answer(true);
    expect(await done).toBe(true);
    expect(backend.state.messages.some((m) => m.mailboxId === junk.id)).toBe(false);
    // Çöpe dokunulmadı.
    expect(backend.state.messages.filter((m) => roleOf(m.id) === 'trash')).toHaveLength(3);
  });
});
