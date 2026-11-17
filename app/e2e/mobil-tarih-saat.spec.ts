import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil kabukta kendi tarih-saatini seçme (Pixel 7, `?kabuk=mobil`): "Sonra
// gönder" ve "Ertele" alt sayfalarında satır yerinde girdiye döner, ekrana
// sığar; geçmiş bir an seçilemez.

const TUESDAY_10 = new Date('2026-11-17T10:00:00+03:00');
const at = (local: string) => new Date(`${local}:00+03:00`).getTime();
const heading = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const sheet = (page: Page, name: string) => page.getByRole('dialog', { name });

test.beforeEach(async ({ page }) => {
  await page.clock.install({ time: TUESDAY_10 });
  await page.goto('#/main');
  await expect(heading(page, 'Gelen Kutusu')).toBeVisible();
});

test('sonra gönder alt sayfasında tarih ve saat seçilir; geçmiş seçilemez', async ({ page }) => {
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await expect(page.locator('#compose-to')).toBeFocused();
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  const later = sheet(page, 'Sonra gönder');
  await later.getByRole('menuitem', { name: 'Tarih ve saat seç…' }).click();

  const input = later.getByLabel('Gönderim tarihi ve saati');
  await expect(input).toBeVisible();
  const set = later.getByRole('button', { name: 'Zamanla', exact: true });
  // Girdi ve düğme ekranın içinde, dokunma ölçüsünde.
  const view = page.viewportSize()!;
  for (const el of [input, set]) {
    const box = (await el.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(view.width);
    expect(await el.evaluate((n) => (n as HTMLElement).offsetHeight)).toBeGreaterThanOrEqual(44);
  }

  await input.fill('2026-11-17T09:00');
  await expect(later.getByRole('alert')).toContainText('Bu an geçmişte kaldı');
  await expect(set).toBeDisabled();

  await input.fill('2026-11-21T08:30');
  await set.click();
  await expect(later).toBeHidden();
  await expect(page.getByText(/^Zamanlandı · /)).toBeVisible();
});

test('ertele alt sayfasında tarih ve saat seçilir', async ({ page }) => {
  await page.getByRole('region', { name: 'Posta listesi' }).getByText('Sözleşme imzaya hazır', { exact: true }).click();
  const id = await page.evaluate(() => window.__mh.state.messages.find((m) => m.subject === 'Sözleşme imzaya hazır')!.id);
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Diğer eylemler' }).click();
  await sheet(page, 'Diğer eylemler').getByRole('menuitem', { name: 'Ertele' }).click();
  const snooze = sheet(page, 'Ne zaman geri gelsin?');
  await snooze.getByRole('menuitem', { name: 'Tarih ve saat seç…' }).click();
  await snooze.getByLabel('Erteleme tarihi ve saati').fill('2026-12-01T08:15');
  await snooze.getByRole('button', { name: 'Ertele', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Ertelendi' })).toBeVisible();
  expect(await page.evaluate((mid) => window.__mh.state.messages.find((m) => m.id === mid)!.snoozedUntil, id)).toBe(at('2026-12-01T08:15'));
});
