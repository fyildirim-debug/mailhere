import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil 0.11 · gönderici eylemleri: okuma menüsünde "İstenmeyen değil"
// (İstenmeyen kutusundaki posta), "Güvenli say" (şüpheli posta) ve "Göndereni
// engelle" (geri alınabilir); seçim çubuğunun "Diğer" menüsünde yalnız tek
// posta seçiliyken.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const rowOf = (page: Page, text: string) => list(page).locator('.wrap', { hasText: text });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const strip = (page: Page) => page.getByRole('toolbar', { name: 'Posta eylemleri' });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });

/** Çekmece kapandı ve örtüsü söndü. */
async function drawerGone(page: Page) {
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => page.locator('.scrim').evaluateAll((els) => els.every((el) => getComputedStyle(el).visibility === 'hidden'))).toBe(true);
}

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

async function openArchive(page: Page) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: /^Arşiv/ }).click();
  await expect(h1(page, 'Arşiv')).toBeVisible();
  await drawerGone(page);
}

async function openJunk(page: Page) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Kişisel klasörlerini göster' }).click();
  await drawer(page).getByRole('group', { name: 'Kişisel klasörleri' }).getByRole('button', { name: /İstenmeyen/ }).click();
  await expect(h1(page, 'İstenmeyen · Kişisel')).toBeVisible();
  await drawerGone(page);
}

async function openMore(page: Page) {
  await strip(page).getByRole('button', { name: 'Diğer eylemler' }).click();
  await expect(page.getByRole('dialog', { name: 'Diğer eylemler' })).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('okuma: İstenmeyen kutusundaki postada "İstenmeyen değil" postayı gelen kutusuna döndürür', async ({ page }) => {
  await openJunk(page);
  await row(page, 'Kartınız geçici olarak kapatıldı').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Kartınız geçici olarak kapatıldı' })).toBeVisible();
  await openMore(page);
  const menu = page.getByRole('menu');
  // Kutunun asıl eylemi menünün başında; "Güvenli say" aynı işi yapacağı için yinelenmez.
  await expect(menu.getByRole('menuitem').first()).toHaveText('İstenmeyen değil');
  await expect(menu.getByRole('menuitem', { name: 'Güvenli say' })).toHaveCount(0);
  await menu.getByRole('menuitem', { name: 'İstenmeyen değil' }).click();
  await expect(toast(page, 'Gönderen güvenli sayıldı; posta gelen kutusuna taşındı')).toBeVisible();
  const where = await page.evaluate(() => {
    const s = window.__mh.state;
    const m = s.messages.find((x) => x.subject === 'Kartınız geçici olarak kapatıldı')!;
    return { role: s.mailboxes.find((b) => b.id === m.mailboxId)!.role, spam: m.spam, trusted: s.trusted.some((k) => k.endsWith('|guvenlik@xn--bnka-53d.example')) };
  });
  expect(where).toEqual({ role: 'inbox', spam: false, trusted: true });
  // Listeye dönünce kutu boş.
  await page.goBack();
  await expect(h1(page, 'İstenmeyen · Kişisel')).toBeVisible();
  await expect(row(page, 'Kartınız geçici olarak kapatıldı')).toHaveCount(0);
});

test('okuma: şüpheli postada "Güvenli say" uyarıyı kaldırır; şüpheli olmayan postada yok', async ({ page }) => {
  await openArchive(page);
  await row(page, 'Hafta sonu için beş rota').click();
  await openMore(page);
  await expect(page.getByRole('menuitem', { name: 'Güvenli say' })).toHaveCount(0);
  await expect(page.getByRole('menuitem', { name: 'İstenmeyen değil' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page.goBack();

  await row(page, 'Gönderiniz gümrükte bekliyor').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Gönderiniz gümrükte bekliyor' })).toBeVisible();
  await openMore(page);
  await page.getByRole('menuitem', { name: 'Güvenli say' }).click();
  await expect(toast(page, 'Gönderen güvenli sayıldı; uyarı kaldırıldı')).toBeVisible();
  // Ekran postada kalır; listeye dönünce satırdaki ikon da kalkmıştır.
  await expect(page.getByRole('heading', { level: 2, name: 'Gönderiniz gümrükte bekliyor' })).toBeVisible();
  await page.goBack();
  await expect(row(page, 'Gönderiniz gümrükte bekliyor')).toBeVisible();
  await expect(rowOf(page, 'Gönderiniz gümrükte bekliyor').getByRole('img', { name: 'Şüpheli posta' })).toHaveCount(0);
});

test('okuma: "Göndereni engelle" adresi engellenenlere ekler, postaları İstenmeyen’e taşır; Geri al döndürür', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  const sender = await page.evaluate(() => window.__mh.state.messages.find((x) => x.subject === 'Cumartesi kahvaltı?')!.from.email);
  await openMore(page);
  // Ad kısa: uzun adres alt sayfada eylemi kırpmaz.
  const block = page.getByRole('menuitem', { name: 'Göndereni engelle' });
  expect(await block.locator('.trunc').evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await block.click();
  await expect(toast(page, `${sender} engellendi`)).toBeVisible();
  expect(await page.evaluate(() => window.__mh.state.settings.blocked)).toEqual([sender]);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect.poll(() => page.evaluate(() => window.__mh.state.settings.blocked)).toEqual([]);
});

test('seçim çubuğu: tek postada "Diğer"de gönderici eylemleri; iki postada yok; engelle seçimi bırakır', async ({ page }) => {
  await openArchive(page);
  await page.getByRole('button', { name: 'Gönderiniz gümrükte bekliyor postasını seç' }).click();
  await expect(page.getByRole('heading', { level: 1, name: '1 seçili' })).toBeVisible();
  const bar = page.getByRole('toolbar', { name: '1 posta seçili' });
  await bar.getByRole('button', { name: 'Diğer' }).click();
  await expect(page.getByRole('menuitem', { name: 'Güvenli say' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Göndereni engelle' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'İstenmeyen değil' })).toHaveCount(0);
  await page.keyboard.press('Escape');

  // İkinci posta seçilince gönderici eylemleri kalkar (kime uygulanacağı belirsiz).
  await page.getByRole('button', { name: 'Hafta sonu için beş rota postasını seç' }).click();
  await page.getByRole('toolbar', { name: '2 posta seçili' }).getByRole('button', { name: 'Diğer' }).click();
  await expect(page.getByRole('menuitem', { name: 'Ertele' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Güvenli say' })).toHaveCount(0);
  await expect(page.getByRole('menuitem', { name: 'Göndereni engelle' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Hafta sonu için beş rota postasını seç' }).click();

  await page.getByRole('toolbar', { name: '1 posta seçili' }).getByRole('button', { name: 'Diğer' }).click();
  await page.getByRole('menuitem', { name: 'Göndereni engelle' }).click();
  await expect(toast(page, 'bilgi@derinkargo.com.tr engellendi')).toBeVisible();
  await expect(h1(page, 'Arşiv')).toBeVisible();
  expect(await page.evaluate(() => window.__mh.state.settings.blocked)).toEqual(['bilgi@derinkargo.com.tr']);
});
