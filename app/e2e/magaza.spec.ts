import { expect, test } from '@playwright/test';

// Microsoft Store kurulumu (MSIX): güncellemeyi Store kurar; uygulama içi
// denetim düğmesi ve "kendiliğinden denetle" anahtarı gösterilmez.

async function openUpdates(page: import('@playwright/test').Page, query: string) {
  await page.goto(`/${query}#/main`);
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Güncellemeler', exact: true }).click();
  await expect(page.getByRole('heading', { level: 2, name: 'Güncellemeler' })).toBeVisible();
}

test('Mağaza kurulumunda güncelleme düğmeleri yerine Store notu görünür', async ({ page }) => {
  await openUpdates(page, '?magaza');
  await expect(page.getByText('Bu kurulum Microsoft Store\'dan.', { exact: false })).toBeVisible();
  await expect(page.getByRole('switch', { name: 'Güncellemeleri kendiliğinden denetle' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Şimdi denetle' })).toHaveCount(0);
});

test('olağan kurulumda denetim düğmesi ve anahtar durur', async ({ page }) => {
  await openUpdates(page, '');
  await expect(page.getByRole('switch', { name: 'Güncellemeleri kendiliğinden denetle' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Şimdi denetle' })).toBeVisible();
  await expect(page.getByText('Bu kurulum Microsoft Store\'dan.', { exact: false })).toHaveCount(0);
});
