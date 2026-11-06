import { expect, test, type Page } from '@playwright/test';

// Liste satırı bağlam menüsü (sağ tık / Shift F10) ve okuma alanında Space ile sayfa kaydırma.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });

test('sağ tık menüsü: satır eylemleri, Esc kapatır, Arşivle satırı kaldırır', async ({ page }) => {
  await page.goto('/#/main');
  const row = list(page).getByText('Cumartesi kahvaltı?');
  await row.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Cumartesi kahvaltı?' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem').first()).toBeFocused();
  await expect(menu.getByRole('menuitem')).toHaveCount(8);
  // Sağ tık seçimi değiştirmez.
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);

  await row.click({ button: 'right' });
  await menu.getByRole('menuitem', { name: /^Arşivle/ }).click();
  await expect(page.getByRole('status')).toContainText('Arşivlendi');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
});

test('Shift F10 klavyeden menüyü açar; yıldızla satırı işaretler', async ({ page }) => {
  await page.goto('/#/main');
  const row = list(page).getByText('Cumartesi kahvaltı?');
  await row.click();
  await page.keyboard.press('Shift+F10');
  const menu = page.getByRole('menu', { name: 'Cumartesi kahvaltı?' });
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: /^Yıldızla/ }).click();
  await expect(list(page).locator('.row', { hasText: 'Cumartesi kahvaltı?' }).getByRole('img', { name: 'Yıldızlı' })).toBeVisible();
});

test('Space okuma alanını kaydırır, Shift Space geri alır', async ({ page }) => {
  await page.goto('/#/main');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  // Kısa örnek posta kaydırılamaz; gövdeyi uzatarak kaydırma alanı açılır.
  const scroll = page.locator('.reader .scroll');
  await scroll.evaluate((el) => {
    const pad = document.createElement('div');
    pad.style.height = '4000px';
    el.append(pad);
  });
  await expect.poll(() => scroll.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
  await page.keyboard.press('Space');
  await expect.poll(() => scroll.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  await page.keyboard.press('Shift+Space');
  await expect.poll(() => scroll.evaluate((el) => el.scrollTop)).toBe(0);
});

test('bağlam menüsünden Klasöre taşı seçiciyi satır için açar', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Cumartesi kahvaltı?').click({ button: 'right' });
  await page.getByRole('menu', { name: 'Cumartesi kahvaltı?' }).getByRole('menuitem', { name: /^Klasöre taşı/ }).click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  await expect(page.getByRole('combobox', { name: 'Klasör ara' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('combobox', { name: 'Klasör ara' })).toHaveCount(0);
});

test('komut paleti: ayar bölümleri aranır ve açılır', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toBeVisible();
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox').fill('gizlilik');
  await page.getByRole('option', { name: /Ayarlar: Gizlilik ve güvenlik/ }).click();
  await expect(page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Gizlilik ve güvenlik', exact: true })).toHaveAttribute('aria-current', 'page');
});
