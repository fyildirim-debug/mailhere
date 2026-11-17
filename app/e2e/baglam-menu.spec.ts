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
  await expect(menu.getByRole('menuitem')).toHaveCount(9);
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

test('menüde Enter odaktaki öğeyi seçer (bağlam menüsü ve Diğer eylemler); posta açılmaz', async ({ page }) => {
  await page.goto('/#/main');
  const row = list(page).locator('.row', { hasText: 'Cumartesi kahvaltı?' });
  await expect(row).toHaveClass(/unread/);
  // Başka bir posta okunurken satır menüsü klavyeyle açılır.
  await list(page).getByText('Sözleşme imzaya hazır').click();
  await row.locator('.main').focus();
  await page.keyboard.press('Shift+F10');
  const menu = page.getByRole('menu', { name: 'Cumartesi kahvaltı?' });
  await expect(menu.getByRole('menuitem').first()).toBeFocused();
  // ↓ ↓: "Yıldızla"; Enter seçer, menü kapanır.
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await expect(menu.getByRole('menuitem', { name: /^Yıldızla/ })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(menu).toHaveCount(0);
  await expect(row.getByRole('img', { name: 'Yıldızlı' })).toBeVisible();
  // Enter "seçili postayı aç" sayılmadı: okunan posta ve okunmamışlık değişmedi.
  await expect(page.getByRole('heading', { level: 2, name: 'Sözleşme imzaya hazır' })).toBeVisible();
  await expect(row).toHaveClass(/unread/);

  // Okuma alanındaki "Diğer eylemler": Enter ilk öğeyi ("Okunmadı say") çalıştırır.
  const read = list(page).locator('.row', { hasText: 'Sözleşme imzaya hazır' });
  await expect(read).not.toHaveClass(/unread/);
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  const more = page.getByRole('menu');
  await expect(more.getByRole('menuitem', { name: /^Okunmadı say/ })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(more).toHaveCount(0);
  await expect(read).toHaveClass(/unread/);

  // Süzgeç menüsü (işaretli öğeler): End + Enter son seçeneği seçer.
  await list(page).getByRole('button', { name: 'Süzgeç', exact: true }).click();
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  await expect(list(page).locator('.pill')).toContainText('Ekli');
});

test('Sekme ile odaklanılan düğmede Enter ve Boşluk düğmeyi çalıştırır; fareyle basılmış düğmede kısayol kalır', async ({ page }) => {
  await page.goto('/#/main');
  const row = list(page).locator('.row', { hasText: 'Cumartesi kahvaltı?' });
  await row.locator('.main').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  // Sekme: satırın seçim düğmesi; Boşluk onu işaretler.
  await page.keyboard.press('Tab');
  await expect(row.locator('.pick')).toBeFocused();
  await page.keyboard.press('Space');
  await expect(row.locator('.pick')).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.press('Space');
  await expect(row.locator('.pick')).toHaveAttribute('aria-pressed', 'false');
  // Sekme: satırın Arşivle düğmesi; Enter arşivler (seçili postayı açmak yerine).
  await page.keyboard.press('Tab');
  await expect(row.getByRole('button', { name: 'Arşivle' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('status')).toContainText('Arşivlendi');
  await expect(row).toHaveCount(0);

  // Fareyle basılmış düğmede odak kalsa da Enter kısayoldur: seçili postayı açar (okundu sayar).
  const next = list(page).locator('.row', { hasText: 'Sözleşme imzaya hazır' });
  await next.locator('.main').click();
  await page.locator('.reader .toolbar').getByRole('button', { name: 'Okunmadı say' }).click();
  await expect(next).toHaveClass(/unread/);
  await page.keyboard.press('Enter');
  await expect(next).not.toHaveClass(/unread/);
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
