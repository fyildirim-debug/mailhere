import { expect, test } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil kabukta postadaki `mailto:` bağlantısı yazma ekranını alıcıyla iter
// (dış e-posta uygulamasına gitmez); geri tuşu okunan postaya döner.

test('postadaki mailto bağlantısı yazma ekranını alıcıyla açar', async ({ page }) => {
  await page.goto('#/main');
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu', exact: true })).toBeVisible();
  // Bülten "Bültenler" kategorisinde; bildirimden açılır gibi `mh:open` ile açılır.
  const id = await page.evaluate(() => window.__mh.state.messages.find((m) => m.subject === 'Renk, ışık ve odak')!.id);
  await page.evaluate((mid) => window.__mh.emit('mh:open', { id: mid }), id);
  const link = page.frameLocator('iframe[title="Posta içeriği"]').getByRole('link', { name: 'Ayrıl' });
  await expect(link).toBeVisible();
  await link.click();

  await expect(page.getByRole('heading', { level: 1, name: 'Yeni posta', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'ayril@tasarimnotlari.co alıcısını kaldır' })).toBeVisible();
  expect(page.context().pages().length).toBe(1);

  await page.goBack();
  await expect(page.frameLocator('iframe[title="Posta içeriği"]').getByRole('link', { name: 'Ayrıl' })).toBeVisible();
});
