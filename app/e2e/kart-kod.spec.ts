import { expect, test, type Page } from '@playwright/test';
import { CODE, state } from './koruma';

// 0.11 (masaüstü): bildirim kartında büyük, öbekli doğrulama kodu; kopyalayınca
// "Kod kopyalandı" durumu ve "kopyalayınca okundu say".

async function notifyCode(page: Page, unread = true, copied = false) {
  await page.evaluate(
    async ([subject, u, c]) => {
      const mh = window.__mh;
      const m = mh.state.messages.find((x) => x.subject === subject)!;
      m.unread = u as boolean;
      const d = await mh.call('message_get', { id: m.id });
      mh.emit('mh:notify', { message: d, accountName: 'Kişisel', accountColor: '#2E9A61', otp: d.otp, otpCopied: c as boolean });
    },
    [CODE, unread, copied] as const,
  );
}

test('bildirim kartı: kod büyük ve öbekli; kopyalayınca "Kod kopyalandı" durumu ve okundu', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/#/notify');
  await expect(page.getByRole('alertdialog', { name: 'Yeni posta: Ayşe Demir' })).toBeVisible();
  // Kodsuz kartta kod satırı yok.
  await expect(page.locator('.code')).toHaveCount(0);
  await notifyCode(page);
  const card = page.getByRole('alertdialog', { name: 'Yeni posta: Bulut Fatura' });
  const code = card.locator('.code-value');
  await expect(code).toHaveText('731 004');
  expect(await code.evaluate((el) => [getComputedStyle(el).fontSize, getComputedStyle(el).fontWeight, getComputedStyle(el).fontVariantNumeric])).toEqual([
    '24px',
    '600',
    'tabular-nums',
  ]);
  // Kod kartın içinde kalır.
  const cardBox = (await card.boundingBox())!;
  const codeBox = (await code.boundingBox())!;
  expect(codeBox.x + codeBox.width).toBeLessThanOrEqual(cardBox.x + cardBox.width - 12);
  await expect(card.getByRole('status')).toHaveCount(0);

  await card.getByRole('button', { name: 'Kodu kopyala' }).click();
  await expect(card.getByRole('status')).toHaveText('Kod kopyalandı');
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('731004');
  // "Kopyalayınca okundu say" (varsayılan açık).
  await expect.poll(async () => (await state(page, CODE))?.unread).toBe(false);
  // Düğme yeniden "Kodu kopyala"ya döner; durum kartta kalır.
  await expect(card.getByRole('button', { name: 'Kodu kopyala' })).toBeVisible({ timeout: 4000 });
  await expect(card.getByRole('status')).toHaveText('Kod kopyalandı');
});

test('bildirim kartı: "okundu say" kapalıysa kopyalamak postayı okundu yapmaz; kendiliğinden kopyalamada durum hazır gelir', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/#/notify');
  await expect(page.getByRole('alertdialog', { name: 'Yeni posta: Ayşe Demir' })).toBeVisible();
  await page.evaluate(() => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, otpMarkRead: false, otpAutoCopy: true } }));
  await notifyCode(page, true, true);
  const card = page.getByRole('alertdialog', { name: 'Yeni posta: Bulut Fatura' });
  // Çekirdek kodu kart çıkmadan kopyalamıştır: durum baştan yazar, düğme yeniden kopyalamaya hazır.
  await expect(card.getByRole('status')).toHaveText('Kod kopyalandı');
  const copy = card.getByRole('button', { name: 'Kodu kopyala' });
  await expect(copy).toBeVisible();
  await copy.click();
  await expect(card.getByRole('button', { name: 'Kod kopyalandı' })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('731004');
  expect((await state(page, CODE))?.unread).toBe(true);
});
