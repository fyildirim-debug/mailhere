import { expect, test, type Page } from '@playwright/test';

// Parola kilidi (mobil kabuk, Pixel 7): ayarlardan kurma, yeniden yükleme →
// kilit ekranı (tam ekran, 48 px hedefler), yanlış → doğru → liste; uygulamaya
// dönüşte yeniden kilitlenme; kapatma; unutma akışı.

const PASS = 'çok gizli parola';
const lockScreen = (page: Page) => page.getByTestId('lock-screen');
const passField = (page: Page) => lockScreen(page).getByLabel('Parola', { exact: true });
const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name });

async function openPrivacy(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await expect(h1(page, 'Ayarlar')).toBeVisible();
  await page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: 'Gizlilik ve güvenlik', exact: true }).click();
  await expect(h1(page, 'Gizlilik ve güvenlik')).toBeVisible();
  await expect(page.getByRole('heading', { level: 3, name: 'Parola kilidi' })).toBeVisible();
}

async function expectTouchHeight(locator: ReturnType<Page['getByRole']>, min: number) {
  const box = await locator.boundingBox();
  expect(box, 'öğe görünür olmalı').not.toBeNull();
  expect(Math.round(box!.height * 100) / 100).toBeGreaterThanOrEqual(min);
}

async function enableLock(page: Page) {
  await openPrivacy(page);
  await page.getByRole('button', { name: 'Kilidi kur' }).click();
  const dlg = page.getByRole('dialog', { name: 'Parola kilidini kur' });
  await dlg.getByLabel('Yeni parola', { exact: true }).fill(PASS);
  await dlg.getByLabel('Yeni parola (yeniden)').fill(PASS);
  await dlg.getByLabel(/Anladım/).check();
  await dlg.getByRole('button', { name: 'Kilidi kur' }).click();
  await expect(dlg).toHaveCount(0);
  await expect(page.getByRole('switch', { name: 'Uygulamaya dönünce parola sor' })).toBeChecked();
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('kur → yeniden başlat → kilit ekranı → yanlış → doğru → liste; dönüşte yeniden kilit; kapat', async ({ page }) => {
  await expect(lockScreen(page)).toHaveCount(0);
  await enableLock(page);

  await page.reload();
  await expect(lockScreen(page)).toBeVisible();
  await expect(h1(page, 'Mailhere kilitli')).toBeVisible();
  // Liste, çekmece düğmesi ve yüzen düğme yok; dokunma hedefleri 44/48.
  await expect(page.getByRole('button', { name: 'Menü', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Yeni posta' })).toHaveCount(0);
  await expectTouchHeight(lockScreen(page).getByRole('button', { name: 'Kilidi aç' }), 48);
  await expectTouchHeight(lockScreen(page).getByRole('button', { name: 'Parolayı göster' }), 44);
  await expectTouchHeight(lockScreen(page).getByRole('button', { name: 'Parolamı unuttum' }), 44);
  // Yatay taşma yok.
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);

  await passField(page).fill('yanlış parola');
  await lockScreen(page).getByRole('button', { name: 'Kilidi aç' }).click();
  await expect(lockScreen(page).getByRole('alert')).toHaveText('Parola yanlış. Yeniden dene.');
  await passField(page).fill(PASS);
  await lockScreen(page).getByRole('button', { name: 'Kilidi aç' }).click();
  await expect(lockScreen(page)).toHaveCount(0);
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();

  // Uygulamaya dönüş: arka planda 1 dk'dan uzun kalınca arayüz kilitlenir
  // (burada arka uç çağrısıyla benzetilir); çekirdek açık kalır.
  await page.evaluate(() => (window as unknown as { __mh: { call: (n: string) => Promise<unknown> } }).__mh.call('lock_ui_lock'));
  await expect(lockScreen(page)).toBeVisible();
  await expect(page.getByText(/Uygulamaya dönüldü/)).toBeVisible();
  await passField(page).fill(PASS);
  await lockScreen(page).getByRole('button', { name: 'Kilidi aç' }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();

  await openPrivacy(page);
  await expect(page.getByText(/^Açık\./)).toBeVisible();
  await page.getByRole('button', { name: 'Kilidi kapat' }).click();
  const off = page.getByRole('dialog', { name: 'Parola kilidini kapat' });
  await off.getByLabel('Mevcut parola').fill(PASS);
  await off.getByRole('button', { name: 'Kilidi kapat' }).click();
  await expect(off).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Kilidi kur' })).toBeVisible();
  await page.reload();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await expect(lockScreen(page)).toHaveCount(0);
});

test('parolamı unuttum: onayla yerel veri silinir, hesap ekleme ekranı gelir', async ({ page }) => {
  await enableLock(page);
  await page.reload();
  await expect(lockScreen(page)).toBeVisible();
  await lockScreen(page).getByRole('button', { name: 'Parolamı unuttum' }).click();
  await expect(h1(page, 'Parolamı unuttum')).toBeVisible();
  await expect(page.getByText(/Sunucudaki postalar etkilenmez/)).toBeVisible();
  await lockScreen(page).getByRole('button', { name: 'Yerel veriyi sil ve baştan başla' }).click();
  await expect(h1(page, 'Hesap ekle')).toBeVisible();
  await expect(lockScreen(page)).toHaveCount(0);
});
