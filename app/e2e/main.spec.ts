import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/#/main');
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
});

test('gelen kutusu konuşmaları tek satırda listeler ve ilkini açar', async ({ page }) => {
  const list = page.getByRole('region', { name: 'Posta listesi' });
  await expect(list.getByText('Kerem Aydın').first()).toBeVisible();
  await expect(list.getByText('Re: Q4 bütçe taslağı')).toHaveCount(1);
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await expect(page.getByText('3 ileti', { exact: false })).toBeVisible();
});

test('akıllı süzgeç bültenleri ayırır', async ({ page }) => {
  await page.getByRole('button', { name: 'Bültenler' }).click();
  const list = page.getByRole('region', { name: 'Posta listesi' });
  await expect(list.getByText('Mimar Haftalık')).toBeVisible();
  await expect(list.getByText('Tasarım Notları')).toBeVisible();
  await expect(list.getByText('Ayşe Demir')).toHaveCount(0);
});

test('E ile arşivlenir, Geri al geri getirir', async ({ page }) => {
  const list = page.getByRole('region', { name: 'Posta listesi' });
  await page.keyboard.press('e');
  await expect(page.getByRole('status')).toContainText('Arşivlendi');
  await expect(list.getByText('Re: Q4 bütçe taslağı')).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(list.getByText('Re: Q4 bütçe taslağı')).toHaveCount(1);
});

test('J ile gezinilir, Enter okundu sayar', async ({ page }) => {
  const counter = page.getByRole('navigation').getByRole('button', { name: /Gelen Kutusu/ });
  await expect(counter).toContainText('3');
  await page.keyboard.press('j');
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  await expect(counter).toContainText('3');
  await page.keyboard.press('Enter');
  await expect(counter).toContainText('2');
});

test('erteleme menüsü postayı listeden çıkarır', async ({ page }) => {
  await page.getByRole('main').getByRole('button', { name: 'Ertele' }).click();
  await page.getByRole('menuitem', { name: /Yarın sabah/ }).click();
  await expect(page.getByRole('status')).toContainText('Ertelendi');
  const list = page.getByRole('region', { name: 'Posta listesi' });
  await expect(list.getByText('Re: Q4 bütçe taslağı')).toHaveCount(0);
  await page.getByRole('navigation').getByRole('button', { name: /Ertelenenler/ }).click();
  await expect(list.getByText('Re: Q4 bütçe taslağı')).toHaveCount(1);
});

test('bültende uzak görseller engellenir ve izinle açılır', async ({ page }) => {
  await page.getByRole('region', { name: 'Posta listesi' }).getByText('Mimar Haftalık').click();
  const banner = page.getByText('Uzak görseller ve izleme pikselleri engellendi', { exact: false });
  await expect(banner).toBeVisible();
  await page.getByRole('button', { name: 'Bu göndericiye izin ver' }).click();
  await expect(banner).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('görseller açıldı');
});

test('hızlı yanıt giden kutusuna gider ve geri alınabilir', async ({ page }) => {
  const box = page.getByRole('textbox', { name: 'Hızlı yanıt yaz…' });
  await box.fill('Tamam, bakıyorum.');
  await box.press('Control+Enter');
  await expect(page.getByRole('status')).toContainText('Giden kutusunda');
  await expect(box).toHaveValue('');
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(box).toHaveValue('Tamam, bakıyorum.');
});

test('komut paleti yerel aramayla postayı açar', async ({ page }) => {
  await page.keyboard.press('Control+k');
  const palette = page.getByRole('dialog', { name: 'Komut paleti' });
  await expect(palette).toBeVisible();
  await page.keyboard.type('fatura');
  await expect(palette.getByRole('option', { name: /Ekim faturanız hazır/ })).toBeVisible();
  await palette.getByRole('option', { name: /Ekim faturanız hazır/ }).click();
  await expect(palette).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 2, name: 'Ekim faturanız hazır' })).toBeVisible();
});

test('tema düğmesi koyu temaya geçer', async ({ page }) => {
  await page.getByRole('button', { name: 'Temayı değiştir' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

test('ayarlardan dil İngilizceye geçer', async ({ page }) => {
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await expect(page.getByRole('heading', { name: 'Ayarlar' })).toBeVisible();
  await page.getByRole('radio', { name: 'English' }).click();
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await expect(page.getByRole('navigation').getByRole('button', { name: /Inbox/ })).toBeVisible();
});
