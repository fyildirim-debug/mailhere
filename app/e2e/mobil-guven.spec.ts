import { expect, test, type Page } from '@playwright/test';

// Mobil okuma ekranında güven ve gizlilik öğeleri (dar ekran, 360 px): izleyici
// sayılı kısa şerit, gönderen uyarısı tek satır, kod kopyalama 44 px hedef,
// aldatıcı bağlantı onayı ekrana sığar ve geri tuşuyla kapanır.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });

async function openArchived(page: Page, subject: string) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await page.getByRole('dialog', { name: 'Kutular ve hesaplar' }).getByRole('button', { name: /^Arşiv/ }).first().click();
  await expect(page.getByRole('heading', { level: 1, name: 'Arşiv' })).toBeVisible();
  await list(page).getByText(subject, { exact: true }).click();
  await expect(page.getByRole('heading', { level: 2, name: subject })).toBeVisible();
}

async function noOverflow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)).toBe(false);
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  await page.goto('#/main');
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('şerit kısa metinde izleyici sayısını gösterir; dokununca açıklama açılır', async ({ page }) => {
  await openArchived(page, 'Hafta sonu için beş rota');
  const strip = page.locator('.remote');
  await expect(strip.getByText('3 izleyici engellendi', { exact: true })).toBeVisible();
  expect((await strip.boundingBox())!.height).toBeLessThanOrEqual(46);
  const toggle = strip.getByRole('button', { name: '3 izleyici engellendi' });
  expect(Math.round((await toggle.boundingBox())!.height)).toBeGreaterThanOrEqual(44);
  await toggle.click();
  await expect(page.getByText(/görünmez bir görseldir/)).toBeVisible();
  await noOverflow(page);
});

test('izleyici satırı ve gönderen uyarısı tek satır, kesilmeden', async ({ page }) => {
  await openArchived(page, 'Haftalık özet: üç görev tamamlandı');
  const line = page.getByRole('button', { name: '2 izleyici engellendi' });
  const box = (await line.boundingBox())!;
  expect(Math.round(box.height)).toBeGreaterThanOrEqual(44);
  await page.getByRole('button', { name: 'Geri', exact: true }).click();

  await list(page).getByText('Hesabınız 24 saat içinde askıya alınacak', { exact: true }).click();
  const short = page.getByText('Gönderen şüpheli; tıklamadan önce adresi denetle.');
  await expect(short).toBeVisible();
  // Kesilmemiş: metnin tam genişliği görünür alana sığar.
  expect(await short.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  expect((await page.locator('.trust-row').boundingBox())!.height).toBeLessThanOrEqual(22);
  await noOverflow(page);
});

test('kod satırı: Kopyala 44 px hedef, toast verir', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await openArchived(page, 'Giriş doğrulama kodun');
  await expect(page.locator('.otp-code')).toHaveText('482 913');
  const copy = page.getByRole('button', { name: 'Doğrulama kodunu kopyala' });
  expect(Math.round((await copy.boundingBox())!.height)).toBeGreaterThanOrEqual(44);
  await copy.click();
  await expect(page.getByRole('status')).toContainText('Kod kopyalandı');
  await noOverflow(page);
});

test('aldatıcı bağlantı onayı ekrana sığar; geri tuşu önce onayı kapatır', async ({ page }) => {
  await openArchived(page, 'Hesabınız 24 saat içinde askıya alınacak');
  await page.frameLocator('iframe[title="Posta içeriği"]').getByRole('link', { name: 'www.odememerkezi.com.tr' }).click();
  const dialog = page.getByRole('alertdialog', { name: 'Bağlantı başka bir siteye gidiyor' });
  await expect(dialog).toBeVisible();
  const box = (await dialog.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(360);
  for (const name of ['Vazgeç', 'Yine de aç']) {
    expect(Math.round((await dialog.getByRole('button', { name }).boundingBox())!.height)).toBeGreaterThanOrEqual(44);
  }
  await page.goBack();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 2, name: 'Hesabınız 24 saat içinde askıya alınacak' })).toBeVisible();
});
