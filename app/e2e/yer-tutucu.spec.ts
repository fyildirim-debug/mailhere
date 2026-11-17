import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Girdi yer tutucuları sözlükten gelir: hesap ekleme (adres, JMAP) ve kural
// düzenleyici arayüz dilini izler.

const setLanguage = (page: Page, language: 'tr' | 'en') =>
  page.evaluate(async (lang) => {
    await window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, language: lang } });
  }, language);

test('hesap ekleme: adres ve JMAP oturum adresi yer tutucuları dile göre değişir', async ({ page }) => {
  await page.goto('/?bos#/main');
  await expect(page.getByRole('heading', { name: 'Hesap ekle' })).toBeVisible();
  await expect(page.locator('#onb-email')).toHaveAttribute('placeholder', 'ad@alanadi.com');
  await page.getByRole('button', { name: 'JMAP' }).click();
  await expect(page.locator('#jmap-email')).toHaveAttribute('placeholder', 'ad@alanadi.com');
  await expect(page.locator('#jmap-url')).toHaveAttribute('placeholder', 'https://posta.alanadi.com');

  await setLanguage(page, 'en');
  await expect(page.locator('#jmap-email')).toHaveAttribute('placeholder', 'name@example.com');
  await expect(page.locator('#jmap-url')).toHaveAttribute('placeholder', 'https://mail.example.com');
  await setLanguage(page, 'tr');
});

test('kural düzenleyici: gönderen koşulu ve iletme adresi yer tutucuları dile göre değişir', async ({ page }) => {
  await page.goto('/#/main');
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Kurallar', exact: true }).click();
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  await expect(page.getByLabel('Değer')).toHaveAttribute('placeholder', 'ad@alanadi.com');
  await page.getByLabel('Eylem 1').selectOption('forward');
  await expect(page.getByLabel('İletilecek adres')).toHaveAttribute('placeholder', 'ad@alanadi.com');
  // Konu koşulunda örnek adres gösterilmez.
  await page.getByLabel('Koşul 1').selectOption('subject');
  await expect(page.getByLabel('Değer')).toHaveAttribute('placeholder', '');

  await setLanguage(page, 'en');
  await expect(page.getByLabel('Forward to')).toHaveAttribute('placeholder', 'name@example.com');
  await setLanguage(page, 'tr');
});
