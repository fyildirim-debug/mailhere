import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil yazma ekranı (Pixel 7, `?kabuk=mobil`): yüzen düğme → yaz → gönder →
// geri alma toast'ı (taslak olarak yeniden açılır), yanıt ön dolgusu, iletme
// ekleri ve ek ekleme düğmesi, geri tuşunda taslak, Android `mh:navigate` rotası.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const bar = (page: Page) => page.getByRole('toolbar', { name: 'Posta eylemleri' });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });
const body = (page: Page) => page.getByRole('textbox', { name: 'Posta metni' });
const heading = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(heading(page, 'Gelen Kutusu')).toBeVisible();
});

test('yeni posta: yaz, gönder, listeye dön; geri al taslak olarak yeniden açar', async ({ page }) => {
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await expect(heading(page, 'Yeni posta')).toBeVisible();
  // Gönder uygulama çubuğunda, pencere düğmeleri yok.
  const send = page.getByRole('button', { name: 'Gönder', exact: true });
  await expect(send).toBeVisible();
  await expect(page.getByRole('button', { name: 'Kapat', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Sonra gönder' })).toBeVisible();

  await send.click();
  await expect(page.getByRole('alert')).toContainText('En az bir alıcı');
  const to = page.locator('#compose-to');
  await to.fill('kerem.aydin@ornek.com.tr');
  await to.press('Enter');
  await expect(page.getByRole('button', { name: 'kerem.aydin@ornek.com.tr alıcısını kaldır' })).toBeVisible();
  await page.locator('#compose-subject').fill('Plan');
  await body(page).fill('Merhaba');
  await send.click();

  await expect(toast(page, 'Giden kutusunda')).toBeVisible();
  await expect(heading(page, 'Yeni posta')).toHaveCount(0);
  await expect(heading(page, 'Gelen Kutusu')).toBeVisible();

  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(heading(page, 'Taslak')).toBeVisible();
  await expect(page.locator('#compose-subject')).toHaveValue('Plan');
  await expect(body(page)).toHaveValue('Merhaba');
  await expect(page.getByRole('button', { name: 'Kerem Aydın alıcısını kaldır' }).or(page.getByRole('button', { name: 'kerem.aydin@ornek.com.tr alıcısını kaldır' }))).toBeVisible();
  await expect(toast(page, 'taslak olarak açıldı')).toBeVisible();
});

test('tümüne yanıt alıcıları ve alıntıyı doldurur; değişiklik yoksa kapatınca taslak oluşmaz', async ({ page }) => {
  await row(page, 'Re: Q4 bütçe taslağı').click();
  await bar(page).getByRole('button', { name: 'Tümüne' }).click();
  await expect(heading(page, 'Yanıt')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Kerem Aydın alıcısını kaldır' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Ayşe Demir alıcısını kaldır' })).toBeVisible();
  await expect(page.locator('#compose-subject')).toHaveValue('Re: Q4 bütçe taslağı');
  await expect(body(page)).toHaveValue(/> Merhaba,/);

  await page.getByRole('button', { name: 'Yazmayı kapat' }).click();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  const drafts = await page.evaluate(() => window.__mh.state.messages.filter((m) => m.subject === 'Re: Q4 bütçe taslağı').length);
  // Konuşmanın bu konulu iki iletisi; yeni taslak yok.
  expect(drafts).toBe(2);
});

test('iletme özgün ekleri taşır; ek ekleme düğmesi çalışır', async ({ page }) => {
  await row(page, 'Re: Q4 bütçe taslağı').click();
  await bar(page).getByRole('button', { name: 'İlet' }).click();
  await expect(heading(page, 'İlet')).toBeVisible();
  await expect(page.locator('#compose-subject')).toHaveValue('Fwd: Q4 bütçe taslağı');
  await expect(page.getByText('Butce_Q4_taslak.xlsx')).toBeVisible();
  await expect(page.getByText('Sunum_ozeti.pdf')).toBeVisible();
  const attach = page.getByRole('button', { name: 'Dosya ekle' });
  await expect(attach).toBeEnabled();
  // Sahte arka uçta seçici boş döner; hata gösterilmez.
  await attach.click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(heading(page, 'İlet')).toBeVisible();
});

test('sonra gönder alt sayfada; zamanlanınca durum satırı gösterir', async ({ page }) => {
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  const sheet = page.getByRole('dialog', { name: 'Sonra gönder' });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole('group', { name: 'Yanıt gelmezse hatırlat' })).toBeVisible();
  await sheet.getByRole('menuitem', { name: /Yarın sabah/ }).click();
  await expect(sheet).toBeHidden();
  await expect(page.getByText(/^Zamanlandı · /)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Vazgeç' })).toBeVisible();
});

test('geri tuşu: değişiklik taslağa yazılır; taslak listeden açılıp düzenlenir', async ({ page }) => {
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await page.locator('#compose-subject').fill('Taslak deneme');
  await body(page).fill('Yarın devam ederim.');
  await page.goBack();
  await expect(toast(page, 'Taslaklara kaydedildi')).toBeVisible();
  await expect(heading(page, 'Gelen Kutusu')).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__mh.state.messages.filter((m) => m.subject === 'Taslak deneme').length)).toBe(1);

  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await page.getByRole('dialog', { name: 'Kutular ve hesaplar' }).getByRole('button', { name: /Taslaklar/ }).click();
  await expect(heading(page, 'Taslaklar')).toBeVisible();
  await row(page, 'Taslak deneme').click();
  // Taslağın okuma ekranı: alt çubukta Düzenle ve Sil.
  await bar(page).getByRole('button', { name: 'Düzenle' }).click();
  await expect(heading(page, 'Taslak')).toBeVisible();
  await expect(page.locator('#compose-subject')).toHaveValue('Taslak deneme');
  await expect(body(page)).toHaveValue('Yarın devam ederim.');
});

test('mh:navigate olayı (Android open_compose) yazma rotasını açar', async ({ page }) => {
  await page.evaluate(() =>
    window.__mh.emit('mh:navigate', { route: 'compose/new/0?from=demo-is&to=Elif%20%C5%9Eahin%20%3Celif.sahin%40hukukbiz.com.tr%3E' }),
  );
  await expect(heading(page, 'Yeni posta')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Elif Şahin alıcısını kaldır' })).toBeVisible();
  await expect(page.locator('#compose-from')).toHaveValue('demo-is');

  // Yazma açıkken gelen istek ekranın yerine geçer; geri tek adımda listeye döner.
  await page.evaluate(() => window.__mh.emit('mh:navigate', { route: 'compose/forward/3' }));
  await expect(heading(page, 'İlet')).toBeVisible();
  await expect(page.locator('#compose-subject')).toHaveValue('Fwd: Q4 bütçe taslağı');
  await page.goBack();
  await expect(heading(page, 'Gelen Kutusu')).toBeVisible();
});
