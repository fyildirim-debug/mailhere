import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Gönderim öncesi denetimler (yazma penceresi): geçersiz alıcı parçası
// gönderimi durdurur; unutulmuş ek ve boş konu onay sorar (Enter gönderir,
// Esc vazgeçer); alıntılanan önceki ileti ek sözcüğü sayılmaz.

const body = (page: Page) => page.getByRole('textbox', { name: 'Posta metni' });
const dialog = (page: Page) => page.getByRole('alertdialog');
const sendButton = (page: Page) => page.getByRole('button', { name: /^Gönder/ });
const queued = (page: Page) => page.getByText('Giden kutusunda', { exact: false });
const outbox = (page: Page) => page.evaluate(() => window.__mh.call('outbox_list').then((l) => l.length));

async function compose(page: Page, fields: { to?: string; subject?: string; body?: string }) {
  await page.goto('/#/compose/new/0');
  if (fields.to) {
    await page.locator('#compose-to').fill(fields.to);
    await page.locator('#compose-to').press('Enter');
  }
  if (fields.subject) await page.locator('#compose-subject').fill(fields.subject);
  if (fields.body) await body(page).fill(fields.body);
}

test('geçersiz alıcı parçası gönderimi durdurur ve hangisi olduğunu söyler', async ({ page }) => {
  await compose(page, { to: 'kerem.aydin@ornek.com.tr', subject: 'Plan', body: 'Merhaba' });
  // Bilgi alanına adres olmayan bir parça yazılır; Enter'a basılmadan gönderilir.
  await page.getByRole('button', { name: 'Bilgi', exact: true }).click();
  await page.locator('#compose-cc').fill('ayse.demir@eposta.com, mehmet');
  await sendButton(page).click();

  const alert = page.getByRole('alert');
  await expect(alert).toContainText('Bilgi alanındaki "mehmet" geçerli bir adres değil');
  await expect(alert).toContainText('posta henüz gönderilmedi');
  await expect(queued(page)).toHaveCount(0);
  expect(await outbox(page)).toBe(0);
  // Geçerli parça çip oldu, geçersiz olan alanda kaldı ve odak orada.
  await expect(page.getByRole('button', { name: 'ayse.demir@eposta.com alıcısını kaldır' })).toBeVisible();
  await expect(page.locator('#compose-cc')).toHaveValue('mehmet');
  await expect(page.locator('#compose-cc')).toBeFocused();

  // Düzeltince uyarı kalkar ve posta gider.
  await page.locator('#compose-cc').fill('mehmet@ornek.com');
  await expect(alert).toHaveCount(0);
  await page.keyboard.press('Control+Enter');
  await expect(queued(page)).toBeVisible();
  await expect(page.getByRole('button', { name: 'mehmet@ornek.com alıcısını kaldır' })).toBeVisible();
  expect(await outbox(page)).toBe(1);
});

test('ek unutma: sözcük geçip ek yoksa onay sorar; Esc vazgeçer, Enter gönderir', async ({ page }) => {
  await compose(page, { to: 'kerem.aydin@ornek.com.tr', subject: 'Rapor', body: 'Merhaba,\n\nRaporu ekte bulabilirsin.' });
  await page.keyboard.press('Control+Enter');

  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page).getByRole('heading', { name: 'Ek eklemeden gönderilsin mi?' })).toBeVisible();
  await expect(dialog(page)).toContainText('Metinde "ekte" geçiyor ama postada ek yok');
  const anyway = dialog(page).getByRole('button', { name: 'Yine de gönder' });
  await expect(anyway).toBeFocused();
  await expect(dialog(page).getByRole('button', { name: 'Vazgeç' })).toBeVisible();
  expect(await outbox(page)).toBe(0);

  // Esc: gönderilmez, odak ek düğmesine gider.
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toHaveCount(0);
  await expect(queued(page)).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Dosya ekle' })).toBeFocused();
  expect(await outbox(page)).toBe(0);

  // Yeniden: Enter yine de gönderir.
  await sendButton(page).click();
  await expect(dialog(page)).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(queued(page)).toBeVisible();
  expect(await outbox(page)).toBe(1);
});

test('boş konu onay sorar; vazgeçince odak konuya gider, konu yazılınca sorulmaz', async ({ page }) => {
  await compose(page, { to: 'kerem.aydin@ornek.com.tr', body: 'Merhaba' });
  await sendButton(page).click();
  await expect(dialog(page).getByRole('heading', { name: 'Konu olmadan gönderilsin mi?' })).toBeVisible();
  await expect(dialog(page)).toContainText('Konu satırı boş');
  // Sekme iki düğme arasında döner; pencerenin dışına çıkmaz.
  await page.keyboard.press('Tab');
  await expect(dialog(page).getByRole('button', { name: 'Vazgeç' })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(dialog(page).getByRole('button', { name: 'Yine de gönder' })).toBeFocused();

  await dialog(page).getByRole('button', { name: 'Vazgeç' }).click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(page.locator('#compose-subject')).toBeFocused();
  expect(await outbox(page)).toBe(0);

  await page.locator('#compose-subject').fill('Plan');
  await page.keyboard.press('Control+Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(queued(page)).toBeVisible();
});

test('konu da ek de eksikse tek pencerede ikisi birden sorulur', async ({ page }) => {
  await compose(page, { to: 'kerem.aydin@ornek.com.tr', body: 'Please see the attached file.' });
  await sendButton(page).click();
  await expect(dialog(page).getByRole('heading', { name: 'Konu ve ek olmadan gönderilsin mi?' })).toBeVisible();
  await expect(dialog(page)).toContainText('Metinde "attached" geçiyor');
  await expect(dialog(page)).toContainText('Konu satırı boş');
  await dialog(page).getByRole('button', { name: 'Yine de gönder' }).click();
  await expect(queued(page)).toBeVisible();
});

test('alıntılanan önceki ileti ek sözcüğü sayılmaz; iletilen ek varken sorulmaz', async ({ page }) => {
  await page.goto('/#/main');
  await page.waitForFunction(() => !!window.__mh);
  // Özgün postanın metninde "ekte" geçsin: yanıtta alıntı olarak durur.
  await page.evaluate(async () => {
    const m = window.__mh.state.messages.find((x) => x.id === 3)!;
    m.bodyText = 'Merhaba,\n\nBütçe taslağı ekte.';
    m.subject = 'Q4 bütçe taslağı ekte';
  });
  await page.goto('/#/compose/reply/3');
  await expect(body(page)).toHaveValue(/> Bütçe taslağı ekte\./);
  await body(page).press('Control+Home');
  await body(page).pressSequentially('Teşekkürler, aldım.');
  await page.keyboard.press('Control+Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(queued(page)).toBeVisible();

  // İletmede özgün ekler taşınır: metinde sözcük geçse de sorulmaz.
  await page.goto('/#/compose/forward/3');
  await expect(page.getByText('Butce_Q4_taslak.xlsx')).toBeVisible();
  await page.locator('#compose-to').fill('ayse.demir@eposta.com');
  await page.locator('#compose-to').press('Enter');
  await body(page).press('Control+Home');
  await body(page).pressSequentially('Dosyalar ekte.');
  await page.keyboard.press('Control+Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(queued(page)).toBeVisible();
});

test('zamanlanmış gönderimde de sorulur; onay düğmesi "Yine de zamanla" der', async ({ page }) => {
  await compose(page, { to: 'kerem.aydin@ornek.com.tr', subject: 'Rapor', body: 'Dosyayı ekledim.' });
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  await page.getByRole('menuitem', { name: /Yarın sabah/ }).click();
  await expect(page.getByText(/^Zamanlandı · /)).toBeVisible();
  await page.getByRole('button', { name: /^Zamanla/ }).click();

  await expect(dialog(page)).toContainText('Metinde "ekledim" geçiyor');
  expect(await outbox(page)).toBe(0);
  await dialog(page).getByRole('button', { name: 'Yine de zamanla' }).click();
  await expect(dialog(page)).toHaveCount(0);
  await expect.poll(() => outbox(page)).toBe(1);
  const at = await page.evaluate(() => window.__mh.call('outbox_list').then((l) => l[0].sendAt));
  expect(at).toBeGreaterThan(Date.now() + 60_000);
});
