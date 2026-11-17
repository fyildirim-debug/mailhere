import { expect, test } from '@playwright/test';
import { list, row, SUSPECT, CUSTOMS, CODE } from './koruma';

// 0.11 (masaüstü): yeni arama işleçleri; palet ipuçları, tamamlama ve
// "Diğer" süzgeç çipi.

test('arama: is:şüpheli ve has:kod işleçleri; palet yeni işleçleri ve Türkçe adları önerir', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await page.keyboard.press('/');
  const input = page.getByRole('combobox');
  const tips = page.getByRole('listbox');
  await input.fill('is:');
  await expect(tips.getByRole('option', { name: /is:şüpheli/ })).toContainText('Şüpheli görünenler');
  await expect(tips.getByRole('option', { name: /is:okunmuş/ })).toContainText('Okunmuşlar');
  await input.fill('has:');
  await expect(tips.getByRole('option', { name: /has:kod/ })).toContainText('Doğrulama kodu içerenler');
  // Türkçe adı yazınca işlecin ipucu çıkar ve işleci yazar.
  await input.fill('büy');
  await tips.getByRole('option', { name: /larger:/ }).click();
  await expect(input).toHaveValue('larger:');
  // Boyut işlecinde hazır değerler; Tab ilkini yazar.
  await expect(tips.getByRole('option')).toHaveText([/1 MB üstü/, /5 MB üstü/, /10 MB üstü/, /25 MB üstü/]);
  await page.keyboard.press('Tab');
  await expect(input).toHaveValue('larger:1mb ');
  // kimden: gönderen, kime: ve cc: alıcı önerir.
  await input.fill('kimden:ayş');
  await expect(tips.getByRole('option', { name: /Ayşe Demir/ })).toBeVisible();
  await input.fill('kime:');
  await expect(tips.getByRole('option').first()).toBeVisible();
  await input.fill('cc:');
  await expect(tips.getByRole('option').first()).toBeVisible();

  // is:şüpheli İstenmeyen kutusundaki şüpheliyi de bulur.
  await input.fill('is:şüpheli');
  await page.keyboard.press('Enter');
  await expect(row(page, SUSPECT)).toBeVisible();
  await expect(row(page, CUSTOMS)).toBeVisible();
  await expect(list(page).locator('.row')).toHaveCount(2);
  const chips = page.getByRole('group', { name: 'Arama süzgeçleri' });
  await expect(chips.getByRole('button', { name: 'Şüpheli', exact: true })).toHaveAttribute('aria-pressed', 'true');
});

test('arama çipleri: Diğer menüsü kod, şüpheli, okunmuş ve boyut işleçlerini yazar', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await page.keyboard.press('/');
  await page.getByRole('combobox').fill('kod');
  await page.keyboard.press('Enter');
  const title = list(page).getByRole('heading', { level: 1 });
  const chips = page.getByRole('group', { name: 'Arama süzgeçleri' });
  const more = chips.getByRole('button', { name: 'Diğer süzgeçler' });
  const menu = page.getByRole('menu', { name: 'Diğer süzgeçler' });
  // Etkin olmayan ek süzgeçlerin çipi yoktur (satır sakin kalır).
  await expect(chips.getByRole('button', { name: 'Kod içeren' })).toHaveCount(0);

  await more.click();
  await expect(menu.getByRole('menuitemcheckbox')).toHaveText(['Kod içeren', 'Şüpheli', 'Okunmuş', '1 MB üstü', '5 MB üstü', '10 MB üstü', '25 MB üstü']);
  await menu.getByRole('menuitemcheckbox', { name: 'Kod içeren' }).click();
  await expect(title).toContainText('kod has:kod');
  await expect(row(page, CODE)).toBeVisible();
  await expect(row(page, 'Giriş doğrulama kodun')).toBeVisible();
  const codeChip = chips.getByRole('button', { name: 'Kod içeren' });
  await expect(codeChip).toHaveAttribute('aria-pressed', 'true');
  await more.click();
  await expect(menu.getByRole('menuitemcheckbox', { name: 'Kod içeren' })).toBeChecked();

  // Okunmuş ve okunmamış birlikte olmaz.
  await menu.getByRole('menuitemcheckbox', { name: 'Okunmuş' }).click();
  await expect(title).toContainText('kod has:kod is:okunmuş');
  await chips.getByRole('button', { name: 'Okunmamış', exact: true }).click();
  await expect(title).toContainText('kod has:kod is:okunmamış');
  await expect(chips.getByRole('button', { name: 'Okunmuş', exact: true })).toHaveCount(0);

  // Boyut: değerli çip, × ile kalkar.
  await more.click();
  await menu.getByRole('menuitemcheckbox', { name: '5 MB üstü', exact: true }).click();
  await expect(title).toContainText('larger:5mb');
  await expect(chips.getByRole('button', { name: 'Boyut: 5 MB üstü' })).toBeVisible();
  await chips.getByRole('button', { name: 'Boyut süzgecini kaldır' }).click();
  await expect(title).not.toContainText('larger:');
  // Çipe basınca süzgeç kalkar.
  await codeChip.click();
  await expect(title).not.toContainText('has:kod');
});
