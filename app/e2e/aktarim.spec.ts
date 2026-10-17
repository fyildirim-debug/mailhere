import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
    __aktarim?: string[];
  }
}

// Masaüstü: Ayarlar › Cihazlar › "Telefona aktar". Sahte arka uç olayları 0 / 1 / 2 / 4 s'de
// (waiting → connected → delivered → applied) üretir; teklif 180 s yaşar.

const nav = (page: Page) => page.getByRole('navigation', { name: 'Ayarlar' });
const qr = (page: Page) => page.getByRole('img', { name: /Aktarım QR kodu/ });
/** Durum satırı (toast da `status` rolündedir; metinle ayrılır). */
const status = (page: Page) => page.getByRole('status').filter({ hasText: /Telefon|Aktarıldı|Süre doldu|Vazgeçildi|başarısız/ });

/** QR kartın içinde kalır (taşmaz, kırpılmaz); yanındaki düğmeler sayfa dışına çıkmaz. */
async function expectQrInsideCard(page: Page) {
  const svg = (await qr(page).boundingBox())!;
  const card = (await page.locator('.qr-card').boundingBox())!;
  // İç alan: 1 px kenarlık + 12 px iç boşluk; QR bu alanı tam doldurur.
  const inset = 13;
  expect(svg.x).toBeGreaterThanOrEqual(card.x + inset - 0.5);
  expect(svg.y).toBeGreaterThanOrEqual(card.y + inset - 0.5);
  expect(svg.x + svg.width).toBeLessThanOrEqual(card.x + card.width - inset + 0.5);
  expect(svg.y + svg.height).toBeLessThanOrEqual(card.y + card.height - inset + 0.5);
  // Kart ayar sayfasının içerik alanında; solundan kırpılmaz.
  const page2 = (await page.getByRole('heading', { level: 2, name: 'Cihazlar' }).boundingBox())!;
  expect(card.x).toBeGreaterThanOrEqual(page2.x - 1);
  const viewport = page.viewportSize()!;
  for (const name of ['Kodu metin olarak göster', 'Kopyala', 'Vazgeç']) {
    const b = (await page.getByRole('button', { name, exact: true }).boundingBox())!;
    expect(b.x + b.width, name).toBeLessThanOrEqual(viewport.width);
  }
}

async function openDevices(page: Page) {
  await page.goto('/#/main');
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await nav(page).getByRole('button', { name: 'Cihazlar', exact: true }).click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cihazlar' })).toBeVisible();
}

test('Cihazlar: QR oluşturulur, geri sayım işler, metin gösterilip kopyalanır, durum sırayla "Telefonda uygulandı"ya ulaşır', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await openDevices(page);
  // Gezinmede Hesaplar ile Gizlilik arasında durur.
  const labels = await nav(page).getByRole('button').allInnerTexts();
  const names = labels.map((s) => s.trim());
  expect(names.indexOf('Cihazlar')).toBeGreaterThan(names.indexOf('Hesaplar'));
  expect(names.indexOf('Cihazlar')).toBeLessThan(names.indexOf('Gizlilik ve güvenlik'));

  await expect(page.getByRole('heading', { level: 3, name: 'Telefona aktar' })).toBeVisible();
  await expect(page.getByText('Aktarılmaz: postalar, taslaklar')).toBeVisible();
  const secrets = page.getByRole('switch', { name: 'Parola ve jetonları da aktar' });
  await expect(secrets).toBeChecked();
  await expect(page.getByText('Ekran paylaşırken')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Android uygulamasını indir' })).toBeVisible();
  await expect(qr(page)).toHaveCount(0);

  await page.getByRole('button', { name: 'QR kodu oluştur' }).click();
  await expect(qr(page)).toBeVisible();
  // Modül başına bir rect + zemin; sessiz bölge viewBox'ta. Dört adresli kod sürüm 9'dur
  // (53 modül + 8); her modül tam sayı piksel (4 px) çizilir.
  await expect(qr(page)).toHaveAttribute('viewBox', '0 0 61 61');
  const box = await qr(page).boundingBox();
  expect(Math.round(box!.width)).toBe(244);
  await expectQrInsideCard(page);
  const cells = await qr(page).locator('rect.cell').count();
  const ones = await page.evaluate(async () => {
    const o = await window.__mh.call('transfer_begin', { includeSecrets: true });
    return o.qr.rows.join('').split('1').length - 1;
  });
  // Yeni teklif QR'ı yeniler; sayı aynı düzende kalır (bulucu desenler + veri).
  expect(cells).toBeGreaterThan(200);
  expect(ones).toBeGreaterThan(200);
  // Teklif açıkken onay kutusu değiştirilemez; geri sayım dakika:saniye.
  await expect(secrets).toBeDisabled();
  await expect(page.getByText('Kalan süre')).toContainText(/0[23]:[0-5]\d/);
  await expect(status(page)).toContainText('Telefonla QR’ı okut');

  // Metin varsayılan gizli; gösterilince anahtar uyarısıyla gelir ve kopyalanır.
  await expect(page.locator('code')).toHaveCount(0);
  await page.getByRole('button', { name: 'Kodu metin olarak göster' }).click();
  await expect(page.locator('code')).toContainText(/^mailhere:\/\/aktar\?v=1&h=192\.168\.1\.20,172\.17\.0\.1,100\.101\.102\.103,fd12:3456::20&p=51234&t=/);
  await expect(page.getByText('yalnız telefondaki Mailhere’ye yapıştır')).toBeVisible();
  await page.getByRole('button', { name: 'Kopyala' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'panoya kopyalandı' })).toBeVisible();
  const clip = await page.evaluate(() => navigator.clipboard.readText());
  expect(clip).toMatch(/^mailhere:\/\/aktar\?v=1&/);
  expect(clip).toBe(await page.locator('code').innerText());
  await page.getByRole('button', { name: 'Metni gizle' }).click();
  await expect(page.locator('code')).toHaveCount(0);

  // Olay sırası: bağlandı → teslim → uygulandı; QR kalkar, yeni kod düğmesi gelir.
  await expect(status(page)).toContainText('Telefon bağlandı');
  await expect(status(page)).toContainText('Aktarıldı, telefonda onay bekleniyor');
  await expect(qr(page)).toHaveCount(0);
  await expect(status(page)).toContainText('Telefonda uygulandı', { timeout: 8000 });
  await expect(page.getByRole('button', { name: 'Yeni kod oluştur' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Vazgeç' })).toHaveCount(0);
  await expect(secrets).toBeEnabled();
});

test('Vazgeç teklifi kapatır; bölümden çıkınca açık teklif iptal edilir ve sonraki olaylar gelmez', async ({ page }) => {
  await openDevices(page);
  await page.evaluate(() => {
    window.__aktarim = [];
    window.__mh.on('mh:transfer', (s) => window.__aktarim!.push(s.state));
  });
  await page.getByRole('button', { name: 'QR kodu oluştur' }).click();
  await expect(qr(page)).toBeVisible();
  await page.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(status(page)).toContainText('Vazgeçildi');
  await expect(qr(page)).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__aktarim)).toContain('cancelled');

  // Yeni kod, sonra başka bölüme geçiş: teklif kapanır, 4 s'deki "applied" hiç gelmez.
  await page.getByRole('button', { name: 'Yeni kod oluştur' }).click();
  await expect(qr(page)).toBeVisible();
  await page.evaluate(() => (window.__aktarim = []));
  await nav(page).getByRole('button', { name: 'Gizlilik ve güvenlik', exact: true }).click();
  await expect(page.getByRole('heading', { level: 2, name: 'Gizlilik ve güvenlik' })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__aktarim)).toContain('cancelled');
  await page.waitForTimeout(4500);
  const states = await page.evaluate(() => window.__aktarim);
  expect(states).not.toContain('applied');
  expect(states).not.toContain('delivered');

  // Bölüme dönünce teklif yok; baştan başlanır.
  await nav(page).getByRole('button', { name: 'Cihazlar', exact: true }).click();
  await expect(page.getByRole('button', { name: 'QR kodu oluştur' })).toBeVisible();
  await expect(qr(page)).toHaveCount(0);
});

test('"Android uygulamasını indir" dış tarayıcıda sürümler sayfasını açar', async ({ page, context }) => {
  await openDevices(page);
  const popup = context.waitForEvent('page');
  await page.getByRole('button', { name: 'Android uygulamasını indir' }).click();
  const opened = await popup;
  expect(opened.url()).toBe('https://github.com/fyildirim-debug/mailheremobile/releases');
  await opened.close();
});

for (const width of [1440, 1000]) {
  test(`Cihazlar: uzun kodun QR'ı kartın dışına taşmaz (${width} px)`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await openDevices(page);
    await page.getByRole('button', { name: 'QR kodu oluştur' }).click();
    await expect(qr(page)).toBeVisible();
    await expectQrInsideCard(page);
  });
}
