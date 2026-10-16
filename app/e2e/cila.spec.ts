import { expect, test, type Locator, type Page } from '@playwright/test';
import { contrast, hitsAbove } from './kontrast';

// Son dokunuş (cila) kabulleri: görünüme göre boş durum, kırpılmayan menü,
// metin karşıtlığı (≥ 4.5:1, iki temada), tık hedefleri (≥ 28 px) ve küçük
// düzen kuralları. Her test bir cila maddesinin kabul ölçütünü tutar.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const side = (page: Page) => page.getByRole('navigation', { name: 'Kutular ve hesaplar' });
const palette = (page: Page) => page.getByRole('dialog', { name: 'Komut paleti' });

async function open(page: Page) {
  await page.goto('/#/main');
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
}

async function expectAA(loc: Locator) {
  await expect(loc).toBeVisible();
  expect(await contrast(loc)).toBeGreaterThanOrEqual(4.5);
}

// ------------------------------------------------------------ boş durum

test('boş durum görünümü tanır: Ertelenenler, Çöp ve arama sonucu kendi metnini söyler', async ({ page }) => {
  await open(page);
  const empty = list(page).locator('.empty');

  await side(page).getByRole('button', { name: /^Ertelenenler/ }).click();
  await expect(empty.locator('.empty-title')).toHaveText('Ertelenmiş posta yok');
  await expect(empty.locator('.empty-body')).toHaveText('Ertelediğin postalar zamanı gelince gelen kutusuna döner.');

  await page.keyboard.press('Control+k');
  await page.keyboard.type('zzzz');
  await page.keyboard.press('Enter');
  await expect(list(page).getByRole('heading', { level: 1 })).toHaveText('“zzzz” için sonuçlar');
  await expect(empty.locator('.empty-title')).toHaveText('Sonuç yok');
  await expect(empty.locator('.empty-body')).toContainText('Bu aramayla eşleşen posta yok.');
  await expect(empty.locator('.empty-icon .icon')).toHaveCount(1);
});

// ------------------------------------------------------------ menü

test('menü etiketleri kırpılmaz ve pencerede kalır; kısayol kısayol çipinde', async ({ page }) => {
  await open(page);
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Bu göndericinin tüm postalarını arşivle' })).toBeVisible();

  const clipped = await menu.locator('.menu-item .trunc').evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent));
  expect(clipped).toEqual([]);
  const box = (await menu.boundingBox())!;
  const width = page.viewportSize()!.width;
  expect(box.x + box.width).toBeLessThanOrEqual(width - 8);

  // "Okunmadı say" kısayolu düz metin değil, 20 px kısayol çipi.
  const kbd = menu.getByRole('menuitem', { name: /^Okunmadı say/ }).locator('kbd.kbd');
  await expect(kbd).toHaveText('U');
  expect((await kbd.boundingBox())!.height).toBe(20);
});

test('Kimden çipi menüsünde ad ve adres kırpılmaz', async ({ page }) => {
  await open(page);
  await page.keyboard.press('Control+k');
  await page.keyboard.type('fatura');
  await page.keyboard.press('Enter');
  await list(page).getByRole('group', { name: 'Arama süzgeçleri' }).getByRole('button', { name: 'Kimden' }).click();
  const menu = page.getByRole('menu');
  await expect(menu.getByRole('menuitemcheckbox', { name: /fatura@bulutfatura\.com\.tr/ })).toBeVisible();
  const clipped = await menu.locator('.menu-item .trunc').evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent));
  expect(clipped).toEqual([]);
});

// ------------------------------------------------------------ karşıtlık

for (const scheme of ['light', 'dark'] as const) {
  test(`karşıtlık (${scheme}): seçili satırın saati ve önizlemesi, Yeni posta kısayol harfi`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await open(page);
    const row = list(page).locator('.row.selected');
    await expectAA(row.locator('.time'));
    await expectAA(row.locator('.snippet'));
    await expectAA(side(page).locator('.compose .key'));
  });
}
