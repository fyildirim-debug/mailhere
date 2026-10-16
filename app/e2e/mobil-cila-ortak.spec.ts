import { expect, test, type Locator, type Page } from '@playwright/test';
import { contrast, hitsAbove } from './kontrast';

// Mobil kabukta (Pixel 7, `?kabuk=mobil`) ortak koddan gelen cila kabulleri:
// görünüme göre boş durum, uyarı ve başarı metni karşıtlığı, metin bağlantısının
// dokunma hedefi, yazmada Bilgi/Gizli düğmesi ve dar ekranda imza seçici.

const CODE = 'mailhere://aktar?v=1&h=192.168.1.20&p=51234&t=oKGio6SlpqeoqaqrrK2urw&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8';

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });

async function openSettings(page: Page, section: string) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: section, exact: true }).click();
  await expect(h1(page, section)).toBeVisible();
}

async function expectAA(loc: Locator) {
  await expect(loc).toBeVisible();
  expect(await contrast(loc)).toBeGreaterThanOrEqual(4.5);
}

async function newMail(page: Page) {
  await page.goto('#/main');
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await expect(h1(page, 'Yeni posta')).toBeVisible();
}

test('boş durum görünümü tanır: Ertelenenler kendi metnini söyler', async ({ page }) => {
  await page.goto('#/main');
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await drawer(page).getByRole('button', { name: /Ertelenenler/ }).click();
  await expect(h1(page, 'Ertelenenler')).toBeVisible();
  await expect(page.locator('.m-empty-title')).toHaveText('Ertelenmiş posta yok');
  await expect(page.locator('.m-empty-body')).toHaveText('Ertelediğin postalar zamanı gelince gelen kutusuna döner.');
});

test('karşıtlık (açık tema): aktarım önizlemesindeki uyarı ve bildirim izni "Açık" AA', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('#/main');
  await openSettings(page, 'Bildirimler');
  await expectAA(page.locator('.perm-on').first());

  await page.goto('/?kabuk=mobil&bos#/main');
  await page.getByRole('button').filter({ hasText: /Bilgisayardan QR ile aktar/ }).click();
  await page.getByLabel('Aktarım kodu').fill(CODE);
  await page.getByRole('button', { name: 'Bağlan', exact: true }).click();
  await expect(h1(page, 'Gelenler')).toBeVisible();
  await expectAA(page.locator('.warn').filter({ hasText: 'parola sonra sorulacak' }));
});

test('metin bağlantısının dokunma hedefi görünmez kenarla 44 px’i geçer', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page, 'Kurallar');
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  const add = page.getByRole('button', { name: 'Koşul ekle' });
  await expect(add).toBeVisible();
  expect(await hitsAbove(add, 9)).toBe(true);
  expect(await hitsAbove(add, -9)).toBe(true);
  expect((await add.boundingBox())!.height + 18).toBeGreaterThanOrEqual(44);
});

test('yazma: Bilgi ve Gizli tek ikon düğmede; Kime satırı boşken tek satır, basınca iki alan birlikte açılır', async ({ page }) => {
  await page.setViewportSize({ width: 393, height: 855 });
  await newMail(page);
  await expect(page.getByRole('button', { name: 'Bilgi', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Gizli', exact: true })).toHaveCount(0);
  const ccBcc = page.getByRole('button', { name: 'Bilgi ve Gizli alanlarını ekle' });
  await expect(ccBcc).toBeVisible();
  const box = (await ccBcc.boundingBox())!;
  expect([Math.round(box.width), Math.round(box.height)]).toEqual([44, 44]);
  // Düğme Kime satırının sağında, girdiyle aynı satırda: tek satır = 6 + 44 + 6 + 1 px alt çizgi.
  const row = page.locator('.field.wrap').filter({ has: page.locator('#compose-to') });
  expect((await row.boundingBox())!.height).toBeLessThanOrEqual(57);
  const rb = (await row.boundingBox())!;
  expect(Math.abs(box.x + box.width - (rb.x + rb.width))).toBeLessThanOrEqual(1);

  await ccBcc.click();
  await expect(page.locator('#compose-cc')).toBeVisible();
  await expect(page.locator('#compose-bcc')).toBeVisible();
  await expect(ccBcc).toHaveCount(0);
});

test('yazma: telefon genişliğinde biçim çubuğu taşmaz, imza seçici yalnız ikon ve ekranda', async ({ page }) => {
  for (const width of [360, 393, 412]) {
    await page.setViewportSize({ width, height: 800 });
    await newMail(page);
    const tools = page.locator('.tools');
    const sig = page.getByRole('button', { name: /^İmza:/ });
    await expect(sig).toBeVisible();
    await expect(sig.locator('.sig-lbl')).toBeHidden();
    expect(await tools.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    const b = (await sig.boundingBox())!;
    expect(b.x + b.width).toBeLessThanOrEqual(width);
  }
});
