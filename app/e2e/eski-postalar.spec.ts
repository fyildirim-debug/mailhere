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

test('Daha eski postalar: kutu henüz hazır değilse düğme kalır ve yeniden denemek söylenir; sunucuda kalmadıysa "Hepsi bu kadar"', async ({ page }) => {
  await openArchive(page);
  const more = list(page).getByRole('button', { name: 'Daha eski postalar' });
  const before = await list(page).locator('.row').count();
  /** `messages_older` yanıtını sabitler (çekirdek: kutu eşitlenmedi ya da bir hesap yanıt vermedi). */
  const stub = (result: { fetched: number; more: boolean }) =>
    page.evaluate((r) => {
      const mh = window.__mh as unknown as { call: (name: string, args: unknown) => Promise<unknown>; __call?: (name: string, args: unknown) => Promise<unknown> };
      mh.__call ??= mh.call.bind(mh);
      const real = mh.__call;
      mh.call = (name, args) => (name === 'messages_older' ? Promise.resolve(r) : real(name, args));
    }, result);

  // Posta inmedi ama sunucuda daha eskisi var: "yok" denmez, düğme yerinde kalır.
  await stub({ fetched: 0, more: true });
  await more.click();
  await expect(toast(page)).toContainText('Daha eski postalar henüz hazır değil. Biraz sonra yeniden dene.');
  await expect(more).toBeEnabled();
  await expect(list(page).getByText('Hepsi bu kadar')).toHaveCount(0);
  expect(await list(page).locator('.row').count()).toBe(before);

  // Posta inmedi ve sunucuda da kalmadı: nedeni söylenir, liste sonu "Hepsi bu kadar".
  await stub({ fetched: 0, more: false });
  await more.click();
  await expect(toast(page)).toContainText('Sunucuda daha eski posta yok');
  await expect(list(page).getByRole('status').filter({ hasText: 'Hepsi bu kadar' })).toBeVisible();
  await expect(more).toHaveCount(0);
});

test('Daha eski postalar: inen postalar listenin sonuna eklenir; çok sayfalı liste başa dönmez', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 620 });
  await page.goto('/#/main');
  // Tek hesap (inen postalar o kutunun en eskisinden de eskidir) ve iki sayfalık arşiv.
  await nav(page).getByRole('button', { name: /^Kişisel eposta/ }).click();
  await nav(page).getByRole('button', { name: /^Arşiv/ }).first().click();
  await expect(list(page).getByRole('heading', { level: 1 })).toContainText('Arşiv');
  await page.evaluate(() => {
    const mh = window.__mh;
    const box = mh.state.mailboxes.find((b) => b.accountId === 'demo-kisisel' && b.role === 'archive')!;
    const mine = mh.state.messages.filter((m) => m.mailboxId === box.id);
    const sample = mine[mine.length - 1];
    const oldest = Math.min(...mine.map((m) => m.date));
    let id = Math.max(...mh.state.messages.map((m) => m.id)) + 1;
    for (let i = 1; i <= 150; i++) {
      mh.state.messages.push({ ...structuredClone(sample), id: id++, threadId: `dolgu-${i}`, subject: `Dolgu ${i}`, date: oldest - i * 3_600_000, unread: false, otp: null, spam: false, spamVerdict: null });
    }
    mh.emit('mh:changed', { mailboxIds: [box.id] });
  });
  await expect(list(page).getByText('Dolgu 1', { exact: true })).toBeAttached();
  const scroller = list(page).locator('.scroller');
  const more = list(page).getByRole('button', { name: 'Daha eski postalar' });
  // Sona kadar kaydır: ikinci yerel sayfa yüklenir, düğme ancak ondan sonra çıkar.
  await expect(async () => {
    await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight));
    await expect(more).toBeVisible({ timeout: 500 });
  }).toPass();
  // Düğme içeriğin sonundadır; görünür alana gelene dek kaydırılır (son yerel satır da görünür).
  await more.scrollIntoViewIfNeeded();
  await expect(list(page).getByText('Dolgu 150', { exact: true })).toBeInViewport();
  const top = await scroller.evaluate((el) => el.scrollTop);
  expect(top).toBeGreaterThan(8000);

  await more.click();
  // İlk inen posta, kullanıcının bulunduğu yerin hemen altında görünür; liste başa dönmedi.
  await expect(list(page).getByText('Eski yazışma 1', { exact: true })).toBeInViewport();
  await expect(list(page).getByText('Dolgu 150', { exact: true })).toBeInViewport();
  expect(await scroller.evaluate((el) => el.scrollTop)).toBeGreaterThanOrEqual(top - 1);
  // Yenileme olayı (mh:changed) gecikmeli gelir; ondan sonra da konum korunur.
  await page.waitForTimeout(400);
  expect(await scroller.evaluate((el) => el.scrollTop)).toBeGreaterThanOrEqual(top - 1);
  await expect(list(page).getByText('Eski yazışma 1', { exact: true })).toBeInViewport();
});
