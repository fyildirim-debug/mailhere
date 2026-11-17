import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Kendi tarih-saatini seçme ("Sonra gönder" ve "Ertele"), hazır seçeneklerin
// menü her açıldığında yeniden hesaplanması, geçmiş anın seçilememesi ve
// menülerde ok tuşlarıyla gezinme. Saat sabitlenir (Europe/Istanbul, salı 10:00).

const TUESDAY_10 = new Date('2026-11-17T10:00:00+03:00');
const at = (local: string) => new Date(`${local}:00+03:00`).getTime();

const body = (page: Page) => page.getByRole('textbox', { name: 'Posta metni' });
const laterMenu = (page: Page) => page.getByRole('menu', { name: 'Sonra gönder' });
const snoozeMenu = (page: Page) => page.getByRole('menu', { name: 'Ne zaman geri gelsin?' });
const outbox = (page: Page) => page.evaluate(() => window.__mh.call('outbox_list'));

async function compose(page: Page) {
  await page.clock.install({ time: TUESDAY_10 });
  await page.goto('/#/compose/new/0');
  // Pencere hazır olunca odağı Kime alanına verir; yazmaya ondan sonra başlanır
  // (erken yazılan metin, geciken odakla başka alana düşebilir).
  await expect(page.locator('#compose-to')).toBeFocused();
  await page.locator('#compose-to').fill('kerem.aydin@ornek.com.tr');
  await page.locator('#compose-to').press('Enter');
  await page.locator('#compose-subject').fill('Plan');
  await body(page).fill('Merhaba');
}

test('sonra gönder: kendi tarih-saatini seçer; geçmiş bir an seçilemez', async ({ page }) => {
  await compose(page);
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  await laterMenu(page).getByRole('menuitem', { name: 'Tarih ve saat seç…' }).click();

  // Girdi ilk hazır seçenekle (bu akşam 18:00) açılır ve odaklanır.
  const input = laterMenu(page).getByLabel('Gönderim tarihi ve saati');
  await expect(input).toBeFocused();
  await expect(input).toHaveValue('2026-11-17T18:00');
  const set = laterMenu(page).getByRole('button', { name: 'Zamanla', exact: true });
  await expect(set).toBeEnabled();

  // Geçmiş: ne olduğu ve ne yapılacağı yazar, düğme kapanır.
  await input.fill('2026-11-17T09:59');
  await expect(laterMenu(page).getByRole('alert')).toHaveText('Bu an geçmişte kaldı. İleri bir tarih ve saat seç.');
  await expect(set).toBeDisabled();
  await input.press('Enter');
  await expect(laterMenu(page)).toBeVisible();
  await expect(page.getByText(/^Zamanlandı · /)).toHaveCount(0);

  // İleri bir an: Enter ile de onaylanır.
  await input.fill('2026-11-20T14:30');
  await expect(laterMenu(page).getByRole('alert')).toHaveCount(0);
  await input.press('Enter');
  await expect(laterMenu(page)).toHaveCount(0);
  await expect(page.getByText(/^Zamanlandı · /)).toBeVisible();

  await page.getByRole('button', { name: /^Zamanla/ }).click();
  await expect.poll(async () => (await outbox(page)).length).toBe(1);
  expect((await outbox(page))[0].sendAt).toBe(at('2026-11-20T14:30'));
});

test('sonra gönder: hazır seçenekler menü her açıldığında o ana göre hesaplanır', async ({ page }) => {
  await compose(page);
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  await expect(laterMenu(page).getByRole('menuitem', { name: /Bu akşam/ })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(laterMenu(page)).toHaveCount(0);

  // Pencere ertesi akşama kadar açık kaldı (çarşamba 19:00).
  await page.clock.setSystemTime(new Date('2026-11-18T19:00:00+03:00'));
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  await expect(laterMenu(page).getByRole('menuitem', { name: /Bu akşam/ })).toHaveCount(0);
  await laterMenu(page).getByRole('menuitem', { name: /Yarın sabah/ }).click();
  await page.getByRole('button', { name: /^Zamanla/ }).click();
  await expect.poll(async () => (await outbox(page)).length).toBe(1);
  // Dünün "yarın sabah"ı (çarşamba 09:00, geçmiş) değil: perşembe 09:00.
  expect((await outbox(page))[0].sendAt).toBe(at('2026-11-19T09:00'));
});

test('sonra gönder: seçilen an bu arada geçtiyse gönderilmez, zamanlama kalkar', async ({ page }) => {
  await compose(page);
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  await laterMenu(page).getByRole('menuitem', { name: /Bu akşam/ }).click();
  await expect(page.getByText(/^Zamanlandı · /)).toBeVisible();

  await page.clock.setSystemTime(new Date('2026-11-17T19:00:00+03:00'));
  await page.getByRole('button', { name: /^Zamanla/ }).click();
  await expect(page.getByRole('alert')).toHaveText('Seçtiğin gönderim zamanı geçti; zamanlama kaldırıldı. Yeni bir zaman seç ya da şimdi gönder.');
  expect((await outbox(page)).length).toBe(0);
  await expect(page.getByRole('button', { name: /^Gönder/ })).toBeVisible();
});

test('sonra gönder menüsü: odak ilk seçenekte, ok tuşlarıyla gezilir, Esc düğmeye döner', async ({ page }) => {
  await compose(page);
  const more = page.getByRole('button', { name: 'Sonra gönder' });
  await more.click();
  const items = laterMenu(page).locator('button');
  await expect(items.first()).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(items.nth(1)).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('ArrowUp');
  await expect(items.last()).toBeFocused();
  await page.keyboard.press('Home');
  await expect(items.first()).toBeFocused();
  // Gömülü "Yanıt gelmezse hatırlat" satırları da aynı gezinmenin parçası (adım atlanmaz).
  await page.keyboard.press('End');
  await expect(laterMenu(page).getByRole('menuitem', { name: 'Özel tarih…' })).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(laterMenu(page).getByRole('menuitemcheckbox', { name: /1 hafta sonra/ })).toBeFocused();

  await page.keyboard.press('Escape');
  await expect(laterMenu(page)).toHaveCount(0);
  await expect(more).toBeFocused();
});

test('ertele: ok tuşlarıyla gezilir; kendi tarih-saati seçilir, geçmiş seçilemez', async ({ page }) => {
  await page.clock.install({ time: TUESDAY_10 });
  await page.goto('/#/main');
  await page.getByRole('region', { name: 'Posta listesi' }).getByText('Sözleşme imzaya hazır').click();
  const id = await page.evaluate(() => window.__mh.state.messages.find((m) => m.subject === 'Sözleşme imzaya hazır')!.id);
  await page.getByRole('main').getByRole('button', { name: 'Ertele' }).click();

  const items = snoozeMenu(page).getByRole('menuitem');
  await expect(items.first()).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(items.nth(1)).toBeFocused();
  await page.keyboard.press('Home');
  await page.keyboard.press('ArrowUp');
  const custom = snoozeMenu(page).getByRole('menuitem', { name: 'Tarih ve saat seç…' });
  await expect(custom).toBeFocused();
  await page.keyboard.press('Enter');

  const input = snoozeMenu(page).getByLabel('Erteleme tarihi ve saati');
  await expect(input).toBeFocused();
  // Girdide ok tuşu tarihi değiştirir; menü gezinmesi odağı almaz.
  await page.keyboard.press('ArrowDown');
  await expect(input).toBeFocused();

  const set = snoozeMenu(page).getByRole('button', { name: 'Ertele', exact: true });
  await input.fill('2026-11-16T08:00');
  await expect(snoozeMenu(page).getByRole('alert')).toContainText('Bu an geçmişte kaldı');
  await expect(set).toBeDisabled();

  await input.fill('2026-12-01T08:15');
  await set.click();
  await expect(snoozeMenu(page)).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('Ertelendi');
  expect(await page.evaluate((mid) => window.__mh.state.messages.find((m) => m.id === mid)!.snoozedUntil, id)).toBe(at('2026-12-01T08:15'));
});

test('yanıt hatırlatması menüsü (okuma): ok tuşlarıyla gezilir, özel tarihte geçmiş seçilemez', async ({ page }) => {
  await page.clock.install({ time: TUESDAY_10 });
  await page.goto('/#/main');
  await page.getByRole('navigation', { name: 'Kutular ve hesaplar' }).getByRole('button', { name: /^Gönderilenler/ }).click();
  await page.getByRole('region', { name: 'Posta listesi' }).locator('.row').first().click();
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'Yanıt gelmezse hatırlat' }).click();

  const menu = page.getByRole('menu', { name: 'Yanıt gelmezse hatırlat' });
  await expect(menu.getByRole('menuitemcheckbox', { name: /Yarın/ })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(menu.getByRole('menuitemcheckbox', { name: /3 gün sonra/ })).toBeFocused();
  await page.keyboard.press('End');
  await expect(menu.getByRole('menuitem', { name: 'Özel tarih…' })).toBeFocused();
  await page.keyboard.press('Enter');

  const input = menu.getByLabel('Hatırlatma tarihi ve saati');
  await input.fill('2026-11-10T09:00');
  await expect(menu.getByRole('alert')).toContainText('Bu an geçmişte kaldı');
  await expect(menu.getByRole('button', { name: 'Ayarla' })).toBeDisabled();
  await input.fill('2026-11-25T09:00');
  await menu.getByRole('button', { name: 'Ayarla' }).click();
  await expect(page.getByRole('status')).toContainText('Yanıt gelmezse hatırlatılır');
});
