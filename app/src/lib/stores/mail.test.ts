import { beforeEach, describe, expect, it } from 'vitest';
import { createMockBackend } from '../api/mock';
import { useBackend } from '../api';
import type { Account, Address } from '../api/types';
import { currentRows, mail, quickReplyTargets, refreshList, setCategory, setOnly } from './mail.svelte';

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
