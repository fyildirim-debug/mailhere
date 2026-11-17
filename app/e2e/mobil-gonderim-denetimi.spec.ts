import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil yazma ekranında gönderim öncesi denetimler (Pixel 7, `?kabuk=mobil`):
// onay penceresi ekrana sığar, düğmeler dokunma ölçüsündedir, Android geri
// tuşu vazgeçer ve yazma ekranı açık kalır; onayla gönderim sürer.

const heading = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const body = (page: Page) => page.getByRole('textbox', { name: 'Posta metni' });
const dialog = (page: Page) => page.getByRole('alertdialog');
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(heading(page, 'Gelen Kutusu')).toBeVisible();
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await expect(heading(page, 'Yeni posta')).toBeVisible();
  // Ekran hazır olunca odağı Kime alanına verir; yazmaya ondan sonra başlanır.
  await expect(page.locator('#compose-to')).toBeFocused();
  await page.locator('#compose-to').fill('kerem.aydin@ornek.com.tr');
  await page.locator('#compose-to').press('Enter');
});

test('ek unutma onayı ekrana sığar; geri tuşu vazgeçer, yazma ekranı ve metin kalır', async ({ page }) => {
  await page.locator('#compose-subject').fill('Rapor');
  await body(page).fill('Raporu ekte bulabilirsin.');
  await page.getByRole('button', { name: 'Gönder', exact: true }).click();

  await expect(dialog(page).getByRole('heading', { name: 'Ek eklemeden gönderilsin mi?' })).toBeVisible();
  // Pencere ekranın içinde; iki düğme de en az 44 px (yerleşim ölçüsü: giriş
  // canlandırmasının ölçeğinden etkilenmez).
  const box = (await dialog(page).boundingBox())!;
  const view = page.viewportSize()!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(view.width);
  expect(box.y + box.height).toBeLessThanOrEqual(view.height);
  for (const name of ['Vazgeç', 'Yine de gönder']) {
    const h = await dialog(page).getByRole('button', { name }).evaluate((el) => (el as HTMLElement).offsetHeight);
    expect(h).toBeGreaterThanOrEqual(44);
  }
  // Yatay kaydırma çıkmaz.
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);

  await page.goBack();
  await expect(dialog(page)).toHaveCount(0);
  await expect(heading(page, 'Yeni posta')).toBeVisible();
  await expect(body(page)).toHaveValue('Raporu ekte bulabilirsin.');
  expect(await page.evaluate(() => window.__mh.call('outbox_list').then((l) => l.length))).toBe(0);
});

test('boş konu onayı: "Yine de gönder" gönderir ve listeye döner', async ({ page }) => {
  await body(page).fill('Merhaba');
  await page.getByRole('button', { name: 'Gönder', exact: true }).click();
  await expect(dialog(page).getByRole('heading', { name: 'Konu olmadan gönderilsin mi?' })).toBeVisible();
  await dialog(page).getByRole('button', { name: 'Yine de gönder' }).click();

  await expect(dialog(page)).toHaveCount(0);
  await expect(toast(page, 'Giden kutusunda')).toBeVisible();
  await expect(heading(page, 'Gelen Kutusu')).toBeVisible();
  await expect(heading(page, 'Yeni posta')).toHaveCount(0);
});

test('geçersiz alıcı parçası mobilde de gönderimi durdurur', async ({ page }) => {
  await page.locator('#compose-subject').fill('Plan');
  await body(page).fill('Merhaba');
  await page.locator('#compose-to').fill('ayse');
  await page.getByRole('button', { name: 'Gönder', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Kime alanındaki "ayse" geçerli bir adres değil');
  await expect(heading(page, 'Yeni posta')).toBeVisible();
  expect(await page.evaluate(() => window.__mh.call('outbox_list').then((l) => l.length))).toBe(0);
});
