import { expect, test } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Postadaki `mailto:` bağlantısı işletim sisteminin e-posta programına gitmez:
// Mailhere'nin yazma penceresi alıcısı dolu açılır (gönderen, okunan postanın hesabı).

test('postadaki mailto bağlantısı Mailhere yazma penceresini alıcıyla açar', async ({ page }) => {
  await page.goto('/#/main');
  await page.getByRole('region', { name: 'Posta listesi' }).getByText('Renk, ışık ve odak').click();
  const link = page.frameLocator('iframe[title="Posta içeriği"]').getByRole('link', { name: 'Ayrıl' });
  await expect(link).toHaveAttribute('href', /^mailto:ayril@tasarimnotlari\.co/);

  const popupPromise = page.waitForEvent('popup');
  await link.click();
  const popup = await popupPromise;
  await popup.waitForLoadState();

  // Yazma rotası: yeni posta, alıcı sorguda; dış program adresi (mailto:) açılmadı.
  expect(popup.url()).toContain('#/compose/new/0?');
  expect(new URL(popup.url().replace('/#/', '/')).searchParams.getAll('to')).toEqual(['ayril@tasarimnotlari.co']);
  await expect(popup.getByRole('button', { name: 'ayril@tasarimnotlari.co alıcısını kaldır' })).toBeVisible();
  // Gönderen: okunan postanın hesabı.
  const accountId = await page.evaluate(() => window.__mh.state.messages.find((m) => m.subject === 'Renk, ışık ve odak')!.accountId);
  expect(new URL(popup.url().replace('/#/', '/')).searchParams.get('from')).toBe(accountId);
  await expect(popup.locator('#compose-from')).toHaveValue(accountId);
  // Konu şimdilik taşınmaz (ComposePrefill yalnız alıcı taşıyor).
  await expect(popup.locator('#compose-subject')).toHaveValue('');
});
