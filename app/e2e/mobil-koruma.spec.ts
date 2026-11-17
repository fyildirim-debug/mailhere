import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';
import { hitsAbove } from './kontrast';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil 0.11 · Ayarlar › Gizlilik ve güvenlik: istenmeyen koruması (Kapalı /
// Uyar / İstenmeyen'e taşı), engellenen göndericiler (boş durum, elle ekleme
// ve doğrulaması, kaldır ve geri al) ve doğrulama kodu ayarları.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });

async function openPrivacy(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await page.getByRole('button', { name: 'Gizlilik ve güvenlik' }).click();
  await expect(h1(page, 'Gizlilik ve güvenlik')).toBeVisible();
}

const noOverflow = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)).toBe(false);

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('ayarlar: istenmeyen koruması alt sayfadan seçilir, açıklama seçime göre değişir', async ({ page }) => {
  await openPrivacy(page);
  const pick = page.getByRole('button', { name: /Şüpheli posta gelince/ });
  await expect(pick).toContainText('Uyar');
  await expect(pick).toContainText('Posta gelen kutusunda kalır');
  expect(Math.round((await pick.boundingBox())!.height)).toBeGreaterThanOrEqual(56);
  await pick.click();
  const menu = page.getByRole('menu', { name: 'Şüpheli posta gelince' });
  await expect(menu.getByRole('menuitemcheckbox')).toHaveText(['Kapalı', 'Uyar', "İstenmeyen'e taşı"]);
  await expect(menu.getByRole('menuitemcheckbox', { name: 'Uyar' })).toBeChecked();
  await menu.getByRole('menuitemcheckbox', { name: "İstenmeyen'e taşı" }).click();
  await expect(pick).toContainText("İstenmeyen'e taşı");
  await expect(pick).toContainText('Posta İstenmeyen\'e taşınır');
  expect(await page.evaluate(() => window.__mh.state.settings.spamFilter)).toBe('move');
  await pick.click();
  await page.getByRole('menuitemcheckbox', { name: 'Kapalı' }).click();
  await expect(pick).toContainText('Uyarı gösterilmez');
  expect(await page.evaluate(() => window.__mh.state.settings.spamFilter)).toBe('off');
  await noOverflow(page);
});

test('ayarlar: engellenenler boş durumu, ekleme doğrulaması ve hata metinleri, kaldır ve geri al', async ({ page }) => {
  await openPrivacy(page);
  await expect(page.getByText(/Engellenen gönderici yok\. Bir postanın/)).toBeVisible();
  const input = page.getByRole('textbox', { name: 'Adres ya da alan adı' });
  const add = page.getByRole('button', { name: 'Engelle', exact: true });
  const blockedList = page.getByRole('list', { name: 'Engellenen göndericiler' });
  const error = page.locator('#mob-block-error');
  await expect(add).toBeDisabled();
  expect(Math.round((await input.boundingBox())!.height)).toBeGreaterThanOrEqual(44);
  expect(Math.round((await add.boundingBox())!.height)).toBeGreaterThanOrEqual(44);

  // Geçersiz girdi: ne oldu + nasıl yazılır.
  await input.fill('ornek');
  await add.click();
  await expect(error).toHaveText('Bu bir adres ya da alan adı değil. "ad@alan.com" ya da "@alan.com" biçiminde yaz.');
  await expect(input).toHaveAttribute('aria-invalid', 'true');
  // Yazmaya başlayınca hata kalkar.
  await input.fill('Reklam@Ornek.com ');
  await expect(error).toHaveCount(0);
  await input.press('Enter');
  await expect(blockedList.getByRole('listitem')).toHaveText([/reklam@ornek\.com/]);
  await expect(input).toHaveValue('');
  await expect(page.getByText(/Engellenen gönderici yok/)).toHaveCount(0);

  await input.fill('@Spam-Alani.example');
  await add.click();
  await expect(blockedList.getByRole('listitem')).toHaveText([/reklam@ornek\.com/, /@spam-alani\.example/]);
  expect(await page.evaluate(() => window.__mh.state.settings.blocked)).toEqual(['reklam@ornek.com', '@spam-alani.example']);

  // Yinelenen ve kendi adresimiz.
  await input.fill('REKLAM@ornek.com');
  await add.click();
  await expect(error).toHaveText('reklam@ornek.com zaten engelli; yeniden eklemene gerek yok.');
  await input.fill('furkan@eposta.com');
  await add.click();
  await expect(error).toHaveText('Kendi adresini engelleyemezsin. Başka bir adres yaz.');
  expect(await page.evaluate(() => window.__mh.state.settings.blocked.length)).toBe(2);
  await input.fill('');

  // Kaldır: 44 px hedef, toast ve geri alma (girdi eski sırasına döner).
  const remove = page.getByRole('button', { name: 'reklam@ornek.com engelini kaldır' });
  expect(Math.round((await remove.boundingBox())!.height)).toBeGreaterThanOrEqual(44);
  await remove.click();
  await expect(toast(page, 'reklam@ornek.com artık engelli değil')).toBeVisible();
  await expect(blockedList.getByRole('listitem')).toHaveText([/@spam-alani\.example/]);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(blockedList.getByRole('listitem')).toHaveText([/reklam@ornek\.com/, /@spam-alani\.example/]);
  await noOverflow(page);
});

test('ayarlar: liste doluyken ekleme reddedilir ve ne yapılacağı söylenir', async ({ page }) => {
  await page.evaluate(() =>
    window.__mh.call('settings_set', {
      settings: { ...window.__mh.state.settings, blocked: Array.from({ length: 500 }, (_, i) => `k${i}@ornek.com`) },
    }),
  );
  await openPrivacy(page);
  await page.getByRole('textbox', { name: 'Adres ya da alan adı' }).fill('yeni@ornek.com');
  await page.getByRole('button', { name: 'Engelle', exact: true }).click();
  await expect(page.locator('#mob-block-error')).toHaveText('Liste dolu (en çok 500). Yeni girdi eklemek için önce birini kaldır.');
  expect(await page.evaluate(() => window.__mh.state.settings.blocked.length)).toBe(500);
});

test('ayarlar: doğrulama kodları: kendiliğinden kopyala, okundu say ve kod postalarını sil', async ({ page }) => {
  await openPrivacy(page);
  const auto = page.getByRole('switch', { name: 'Kodu kendiliğinden kopyala' });
  const read = page.getByRole('switch', { name: 'Kopyalayınca okundu say' });
  await expect(auto).not.toBeChecked();
  await expect(read).toBeChecked();
  // Anahtarın dokunma alanı görünmez kenarla 44 px.
  await auto.scrollIntoViewIfNeeded();
  expect(await hitsAbove(auto, 9)).toBe(true);
  await auto.click();
  await read.click();
  await expect(auto).toBeChecked();
  await expect(read).not.toBeChecked();
  expect(await page.evaluate(() => [window.__mh.state.settings.otpAutoCopy, window.__mh.state.settings.otpMarkRead])).toEqual([true, false]);

  const expire = page.getByRole('button', { name: /Kod postalarını sil/ });
  await expect(expire).toContainText('Kapalı');
  await expire.click();
  const menu = page.getByRole('menu', { name: 'Kod postalarını sil' });
  await expect(menu.getByRole('menuitemcheckbox')).toHaveText(['Kapalı', '1 saat sonra', '1 gün sonra']);
  await menu.getByRole('menuitemcheckbox', { name: '1 gün sonra' }).click();
  await expect(expire).toContainText('1 gün sonra');
  expect(await page.evaluate(() => window.__mh.state.settings.otpExpireHours)).toBe(24);
  await expire.click();
  await page.getByRole('menuitemcheckbox', { name: '1 saat sonra' }).click();
  expect(await page.evaluate(() => window.__mh.state.settings.otpExpireHours)).toBe(1);

  // Üç seçenek dışındaki süre (başka cihazdan gelen ayar) saat olarak yazılır.
  await page.evaluate(() => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, otpExpireHours: 48 } }));
  await expect(expire).toContainText('48 saat sonra');
  await noOverflow(page);
});
