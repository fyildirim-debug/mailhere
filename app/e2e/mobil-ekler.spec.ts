import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil 0.11 · ek kaydetme ve paylaşma: okuma ekranında eklerin altındaki
// düğme ve "Diğer eylemler" menüsü "Ekler" alt sayfasını açar; her ek için Aç
// (satır), Kaydet ve Paylaş (44 px ikon düğmeler). Kaydedilince "Kaydedildi",
// vazgeçilince sessizlik, hata olursa ne olduğu söylenir.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const strip = (page: Page) => page.getByRole('toolbar', { name: 'Posta eylemleri' });
const sheet = (page: Page) => page.getByRole('dialog', { name: 'Ekler' });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });
const opener = (page: Page) => page.getByRole('button', { name: 'Ekleri kaydet ya da paylaş' });

/** Ek komutlarının çağrılarını kaydeder; `save` verilirse kaydetmenin sonucu o olur. */
async function spy(page: Page, save?: 'cancel' | 'fail') {
  await page.evaluate((mode) => {
    const w = window as unknown as { __att: string[] };
    w.__att = [];
    const mh = window.__mh;
    const real = mh.call.bind(mh);
    mh.call = ((name: string, args: { id: number; index: number }) => {
      if (name.startsWith('attachment_')) w.__att.push(`${name}:${args.index}`);
      if (name === 'attachment_save' && mode === 'cancel') return Promise.resolve(null);
      if (name === 'attachment_save' && mode === 'fail') {
        return Promise.reject({ code: 'io', message: 'Dosya yazılamadı; depolama dolu olabilir. Yer açıp yeniden dene.' });
      }
      return real(name as never, args as never);
    }) as typeof mh.call;
  }, save);
}

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __att: string[] }).__att);

async function openMail(page: Page) {
  await row(page, 'Re: Q4 bütçe taslağı').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('ekler alt sayfası: her ek için aç, kaydet ve paylaş; hedefler 44 px, 360 px’te taşmaz', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  await openMail(page);
  await opener(page).scrollIntoViewIfNeeded();
  expect(Math.round((await opener(page).boundingBox())!.height)).toBeGreaterThanOrEqual(44);
  await opener(page).click();
  await expect(sheet(page)).toBeVisible();
  const items = sheet(page).getByRole('listitem');
  await expect(items).toHaveCount(2);
  await expect(items.nth(0)).toContainText('Butce_Q4_taslak.xlsx');
  await expect(items.nth(1)).toContainText('Sunum_ozeti.pdf');
  for (const name of ['Butce_Q4_taslak.xlsx dosyasını aç', 'Butce_Q4_taslak.xlsx dosyasını kaydet', 'Butce_Q4_taslak.xlsx dosyasını paylaş']) {
    const b = sheet(page).getByRole('button', { name });
    const box = (await b.boundingBox())!;
    expect(Math.round(box.height), name).toBeGreaterThanOrEqual(44);
    expect(Math.round(box.width), name).toBeGreaterThanOrEqual(44);
    expect(box.x + box.width, name).toBeLessThanOrEqual(360);
  }
  // Uzun ad kısalır, düğmeler ekranda kalır.
  expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)).toBe(false);

  // Geri tuşu önce alt sayfayı kapatır.
  await page.goBack();
  await expect(sheet(page)).toBeHidden();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
});

test('kaydet: yer seçilince "Kaydedildi" ve sayfa kapanır; vazgeçilince sessizce açık kalır', async ({ page }) => {
  await openMail(page);
  await spy(page);
  await opener(page).click();
  await sheet(page).getByRole('button', { name: 'Sunum_ozeti.pdf dosyasını kaydet' }).click();
  await expect(toast(page, 'Kaydedildi')).toBeVisible();
  await expect(sheet(page)).toBeHidden();
  expect(await calls(page)).toEqual(['attachment_save:1']);

  // Kullanıcı sistemin kaydetme ekranından vazgeçti (null): toast yok, sayfa yerinde.
  await page.reload();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await openMail(page);
  await spy(page, 'cancel');
  await opener(page).click();
  await sheet(page).getByRole('button', { name: 'Butce_Q4_taslak.xlsx dosyasını kaydet' }).click();
  await expect.poll(() => calls(page)).toEqual(['attachment_save:0']);
  await expect(sheet(page)).toBeVisible();
  await expect(page.getByRole('status')).toHaveCount(0);
});

test('paylaş ve aç doğru eki verir; menüden de açılır', async ({ page }) => {
  await openMail(page);
  await spy(page);
  // "Diğer eylemler" menüsündeki giriş aynı sayfayı açar.
  await strip(page).getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'Ekleri kaydet ya da paylaş' }).click();
  await expect(sheet(page)).toBeVisible();
  await sheet(page).getByRole('button', { name: 'Sunum_ozeti.pdf dosyasını paylaş' }).click();
  await expect(sheet(page)).toBeHidden();
  await expect(page.getByRole('status')).toHaveCount(0);

  await opener(page).click();
  await sheet(page).getByRole('button', { name: 'Butce_Q4_taslak.xlsx dosyasını aç' }).click();
  await expect(sheet(page)).toBeHidden();
  expect(await calls(page)).toEqual(['attachment_share:1', 'attachment_open:0']);
});

test('hata: ne olduğu ve ne yapılabileceği toast’ta, dosya adıyla', async ({ page }) => {
  await openMail(page);
  await spy(page, 'fail');
  await opener(page).click();
  await sheet(page).getByRole('button', { name: 'Butce_Q4_taslak.xlsx dosyasını kaydet' }).click();
  await expect(toast(page, 'Butce_Q4_taslak.xlsx kaydedilemedi: Dosya yazılamadı; depolama dolu olabilir. Yer açıp yeniden dene.')).toBeVisible();
  await expect(sheet(page)).toBeHidden();
});

test('eksiz postada ekler düğmesi ve menü girişi yok', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  await expect(opener(page)).toHaveCount(0);
  await strip(page).getByRole('button', { name: 'Diğer eylemler' }).click();
  await expect(page.getByRole('menuitem', { name: 'Tümünü yanıtla' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Ekleri kaydet ya da paylaş' })).toHaveCount(0);
});
