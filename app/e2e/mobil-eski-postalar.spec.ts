import { expect, test, type Locator, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil 0.11 · daha eski postalar: yerel sayfalar bitince liste sonunda
// "Daha eski postalar" düğmesi (bekleme durumu, inen postalar, korunan kaydırma
// konumu), sunucuda kalmayınca "Hepsi bu kadar"; aşağı çekip yenileme ve hata.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });

/** Çekmece kapandı ve örtüsü söndü. */
async function drawerGone(page: Page) {
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => page.locator('.scrim').evaluateAll((els) => els.every((el) => getComputedStyle(el).visibility === 'hidden'))).toBe(true);
}

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

async function openView(page: Page, name: RegExp, title: string) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name }).click();
  await expect(h1(page, title)).toBeVisible();
  await drawerGone(page);
}

async function height(el: Locator): Promise<number> {
  return Math.round((await el.boundingBox())!.height * 100) / 100;
}

type Box = { x: number; y: number; width: number; height: number };

/** İki kutu birbirine değmiyor mu. */
function apart(a: Box, b: Box): boolean {
  return a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y;
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('daha eski postalar: liste sonunda düğme, bekleme durumu, inen postalar, "Hepsi bu kadar"', async ({ page }) => {
  await openView(page, /^Arşiv/, 'Arşiv');
  // Sunucu yanıtı gecikir (bekleme durumu görünsün); çağrılar sayılır.
  await page.evaluate(() => {
    const w = window as unknown as { __older: number };
    w.__older = 0;
    const mh = window.__mh;
    const real = mh.call.bind(mh);
    mh.call = ((name: string, args: unknown) => {
      if (name !== 'messages_older') return real(name as never, args as never);
      w.__older++;
      return new Promise((resolve) => setTimeout(() => resolve(real(name as never, args as never)), 500));
    }) as typeof mh.call;
  });
  const older = list(page).getByRole('button', { name: 'Daha eski postalar' });
  await expect(older).toBeVisible();
  expect(await height(older)).toBeGreaterThanOrEqual(44);
  // Düğme son satırın altında; yüzen düğmeler onu örtmez.
  const last = (await list(page).locator('.slot').last().boundingBox())!;
  const box = (await older.boundingBox())!;
  expect(box.y).toBeGreaterThanOrEqual(last.y + last.height);
  expect(apart(box, (await page.locator('.m-fab').boundingBox())!)).toBe(true);
  expect(apart(box, (await page.locator('.m-search-fab').boundingBox())!)).toBe(true);

  const before = await page.evaluate(() => window.__mh.state.messages.length);
  await older.click();
  const busy = list(page).getByRole('button', { name: 'Daha eski postalar indiriliyor…' });
  await expect(busy).toHaveAttribute('aria-busy', 'true');
  // Beklerken ikinci dokunuş yeni istek açmaz.
  await busy.click();
  await expect(list(page).getByText('Eski yazışma 1', { exact: true }).first()).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __older: number }).__older)).toBe(1);
  expect(await page.evaluate(() => window.__mh.state.messages.length)).toBeGreaterThan(before);
  // Kutuda daha eskisi var: düğme yeniden listenin sonunda.
  await expect(older).toBeVisible();
  await expect(older).not.toHaveAttribute('aria-busy', 'true');

  await page.locator('.scroller').evaluate((el) => el.scrollTo(0, el.scrollHeight));
  const top = await page.locator('.scroller').evaluate((el) => el.scrollTop);
  expect(top).toBeGreaterThan(0);
  await older.click();
  await expect(list(page).getByRole('status').filter({ hasText: 'Hepsi bu kadar' })).toBeVisible();
  await expect(older).toHaveCount(0);
  await expect(busy).toHaveCount(0);
  // Okur yerinde kalır: liste başa sarmaz.
  expect(await page.locator('.scroller').evaluate((el) => el.scrollTop)).toBeGreaterThanOrEqual(top);
  await expect(list(page).getByText('Eski yazışma 4', { exact: true }).first()).toBeVisible();

  // Başka görünümün kendi durumu var: gelen kutusunda düğme yeniden çıkar.
  await openView(page, /^Gelen Kutusu/, 'Gelen Kutusu');
  await expect(list(page).getByText('Hepsi bu kadar')).toHaveCount(0);
  await page.locator('.scroller').evaluate((el) => el.scrollTo(0, el.scrollHeight));
  await expect(older).toBeVisible();
  // Kategori süzgecinde (sunucuda karşılığı yok) düğme çıkmaz.
  await page.locator('.scroller').evaluate((el) => el.scrollTo(0, 0));
  await list(page).getByRole('button', { name: /^Kişiler/ }).click();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  await expect(older).toHaveCount(0);
});

test('daha eski postalar: aşağı çekip yenileme durumu bozmaz; hata söylenir, düğme yerinde kalır', async ({ page }) => {
  await openView(page, /^Arşiv/, 'Arşiv');
  const older = list(page).getByRole('button', { name: 'Daha eski postalar' });
  await older.click();
  await expect(list(page).getByText('Eski yazışma 1', { exact: true }).first()).toBeVisible();

  // Aşağı çekip yenileme: liste yeniden kurulur, inen eski postalar ve düğme yerinde.
  const scroller = page.locator('.scroller');
  await scroller.evaluate((el) => el.scrollTo(0, 0));
  const box = (await scroller.boundingBox())!;
  await page.mouse.move(box.x + 200, box.y + 40);
  await page.mouse.down();
  await page.mouse.move(box.x + 200, box.y + 240, { steps: 8 });
  await expect(page.getByText('Bırak, yenilensin')).toBeVisible();
  await page.mouse.up();
  await expect(page.getByText('Yenileniyor…')).toBeHidden();
  await scroller.evaluate((el) => el.scrollTo(0, el.scrollHeight));
  await expect(list(page).getByText('Eski yazışma 1', { exact: true }).first()).toBeVisible();
  await expect(older).toBeVisible();

  // Sunucuya ulaşılamazsa ne olduğu söylenir; düğme yeniden denemek için durur.
  await page.evaluate(() => {
    const mh = window.__mh;
    const real = mh.call.bind(mh);
    mh.call = ((name: string, args: unknown) => {
      if (name === 'messages_older') return Promise.reject({ code: 'network', message: 'Sunucuya ulaşılamadı. Bağlantını denetleyip yeniden dene.' });
      return real(name as never, args as never);
    }) as typeof mh.call;
  });
  await older.click();
  await expect(toast(page, 'Olmadı: Sunucuya ulaşılamadı. Bağlantını denetleyip yeniden dene.')).toBeVisible();
  await expect(older).toBeVisible();
  await expect(older).not.toHaveAttribute('aria-busy', 'true');
  await expect(list(page).getByText('Hepsi bu kadar')).toHaveCount(0);
});
