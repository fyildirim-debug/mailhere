import { expect, test } from '@playwright/test';
import { nav, list, toast, openArchive } from './koruma';

// 0.11 (masaüstü): liste sonunda "Daha eski postalar", bekleme durumu ve
// "Hepsi bu kadar".

test('Daha eski postalar: liste sonunda düğme; bekleme durumu, yeni satırlar ve "Hepsi bu kadar"', async ({ page }) => {
  await openArchive(page);
  const more = list(page).getByRole('button', { name: 'Daha eski postalar' });
  await expect(more).toBeVisible();
  const before = await list(page).locator('.row').count();
  await more.click();
  await expect(list(page).getByText('Eski yazışma 1', { exact: true }).first()).toBeAttached();
  await expect.poll(() => page.evaluate(() => window.__mh.state.messages.filter((m) => m.subject.startsWith('Eski yazışma')).length)).toBeGreaterThan(0);
  expect(await page.evaluate(() => document.querySelectorAll('.list .row').length)).toBeGreaterThan(before);
  // Sunucuda bir parti daha var: düğme durur; sonuncudan sonra "Hepsi bu kadar".
  await expect(more).toBeVisible();
  await more.click();
  const end = list(page).getByRole('status').filter({ hasText: 'Hepsi bu kadar' });
  await expect(end).toBeVisible();
  await expect(more).toHaveCount(0);

  // Durum görünüme özgüdür: başka görünümde düğme yeniden çıkar, aramada hiç yoktur.
  await nav(page).getByRole('button', { name: /^Gönderilenler/ }).first().click();
  await expect(list(page).getByRole('heading', { level: 1 })).toContainText('Gönderilenler');
  await expect(more).toBeVisible();
  await expect(list(page).getByText('Hepsi bu kadar')).toHaveCount(0);
  await page.keyboard.press('/');
  await page.getByRole('combobox').fill('fatura');
  await page.keyboard.press('Enter');
  await expect(list(page).getByRole('heading', { level: 1 })).toContainText('fatura');
  await expect(more).toHaveCount(0);
});

test('Daha eski postalar: beklerken düğme meşgul görünür; çevrimdışıysa nedeni söylenir', async ({ page }) => {
  await openArchive(page);
  const more = list(page).getByRole('button', { name: /Daha eski postalar/ });
  // Yanıtı geciktir: bekleme durumu görünür.
  await page.evaluate(() => {
    const mh = window.__mh;
    const call = mh.call.bind(mh);
    (mh as unknown as { call: unknown }).call = (name: string, args: unknown) =>
      name === 'messages_older' ? new Promise((r) => setTimeout(r, 600)).then(() => call(name as never, args as never)) : call(name as never, args as never);
  });
  await more.click();
  await expect(more).toBeDisabled();
  await expect(more).toHaveAttribute('aria-busy', 'true');
  await expect(more).toContainText('Daha eski postalar alınıyor');
  await expect(more).toBeEnabled();
  // Hata: ne oldu + ne yapılabilir; düğme yerinde kalır.
  await page.evaluate(() => {
    const mh = window.__mh;
    const call = mh.call.bind(mh);
    (mh as unknown as { call: unknown }).call = (name: string, args: unknown) =>
      name === 'messages_older'
        ? Promise.reject({ code: 'network', message: 'Sunucuya ulaşılamadı. Bağlantını denetleyip yeniden dene.' })
        : call(name as never, args as never);
  });
  await more.click();
  await expect(toast(page)).toContainText('Sunucuya ulaşılamadı. Bağlantını denetleyip yeniden dene.');
  await expect(more).toBeEnabled();
});
