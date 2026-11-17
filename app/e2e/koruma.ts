import { expect, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// 0.11 testlerinin (istenmeyen, kod, kalıcı silme, eski postalar, arama) ortak
// yardımcıları: bulucular, örnek posta konuları ve sahte arka uç kısayolları.

export const nav = (page: Page) => page.getByRole('navigation', { name: 'Kutular ve hesaplar' });
export const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
export const toast = (page: Page) => page.locator('.toast');
export const row = (page: Page, subject: string) => list(page).locator('.row', { hasText: subject });
export const notice = (page: Page) => page.getByRole('region', { name: 'Bu posta şüpheli görünüyor' });

export const SUSPECT = 'Kartınız geçici olarak kapatıldı';
export const CUSTOMS = 'Gönderiniz gümrükte bekliyor';
export const CODE = 'Yeni cihazdan giriş kodun';

/** Jetonun hesaplanmış rengi (temaya göre). */
export function token(page: Page, name: string) {
  return page.evaluate((n) => {
    const probe = document.createElement('span');
    probe.style.color = `var(${n})`;
    document.body.appendChild(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  }, name);
}

export async function openJunk(page: Page) {
  await page.goto('/#/main');
  await nav(page).getByRole('button', { name: 'Kişisel klasörlerini göster' }).click();
  await nav(page).getByRole('button', { name: /^İstenmeyen/ }).first().click();
  await expect(list(page).getByRole('heading', { level: 1 })).toContainText('İstenmeyen');
}

export async function openArchive(page: Page) {
  await page.goto('/#/main');
  await nav(page).getByRole('button', { name: /^Arşiv/ }).first().click();
  await expect(list(page).getByRole('heading', { level: 1 })).toContainText('Arşiv');
}

export async function openTrash(page: Page) {
  await nav(page).getByRole('button', { name: /^Çöp/ }).first().click();
  await expect(list(page).getByRole('heading', { level: 1 })).toContainText('Çöp');
}

/** Postanın sahte arka uçtaki durumu. */
export function state(page: Page, subject: string) {
  return page.evaluate((s) => {
    const mh = window.__mh;
    const m = mh.state.messages.find((x) => x.subject === s);
    if (!m) return null;
    return { role: mh.state.mailboxes.find((b) => b.id === m.mailboxId)?.role, unread: m.unread, spam: m.spam };
  }, subject);
}

/** Konuları verilen postaları Çöp'e taşır (sahte arka uç). */
export function trashBySubject(page: Page, subjects: string[]) {
  return page.evaluate(async (list) => {
    const mh = window.__mh;
    const ids = mh.state.messages.filter((m) => list.includes(m.subject)).map((m) => m.id);
    await mh.call('messages_move', { ids, target: 'trash' });
    return ids.length;
  }, subjects);
}
