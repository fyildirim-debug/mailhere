import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';
import { hitsAbove } from './kontrast';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil 0.11 · liste satırı: şüpheli postada kehribar uyarı ikonu; taze kod
// postasında kod çipi (dokununca kopyalar; postayı açmaz, kaydırma ve uzun
// basış başlatmaz). Çip satırın yerleşimini oynatmaz, 360 px'te ve büyük
// yazıda taşmaz, dokunma alanı 44 px.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const rowOf = (page: Page, text: string) => list(page).locator('.wrap', { hasText: text });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });
const chip = (page: Page) => page.getByRole('button', { name: 'Doğrulama kodu 731 004; kopyala' });

/** Çekmece kapandı ve örtüsü söndü. */
async function drawerGone(page: Page) {
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => page.locator('.scrim').evaluateAll((els) => els.every((el) => getComputedStyle(el).visibility === 'hidden'))).toBe(true);
}

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

async function openArchive(page: Page) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: /^Arşiv/ }).click();
  await expect(h1(page, 'Arşiv')).toBeVisible();
  await drawerGone(page);
}

const noOverflow = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)).toBe(false);

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('satır: şüpheli postada kehribar uyarı ikonu; taze kodda çip, eski kodda yok', async ({ page }) => {
  await openArchive(page);
  const icon = rowOf(page, 'Gönderiniz gümrükte bekliyor').getByRole('img', { name: 'Şüpheli posta' });
  await expect(icon).toBeVisible();
  const amber = await page.evaluate(() => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--amber)';
    document.body.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  });
  expect(await icon.evaluate((el) => getComputedStyle(el).color)).toBe(amber);
  // Şüpheli olmayan satırda ikon yok.
  await expect(rowOf(page, 'Hafta sonu için beş rota').getByRole('img', { name: 'Şüpheli posta' })).toHaveCount(0);

  // Kod çipi yalnız son 60 dakikada gelen postada.
  await expect(chip(page)).toBeVisible();
  await expect(chip(page)).toHaveText('731 004');
  await expect(rowOf(page, 'Giriş doğrulama kodun').locator('.otp-chip')).toHaveCount(0);
  await expect(list(page).locator('.otp-chip')).toHaveCount(1);
});

test('kod çipi: dokunma alanı 44 px; kopyalar, postayı açmaz; okunmamış posta okundu sayılır', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  // Posta okunmamış olsun: "kopyalayınca okundu say" varsayılan açık.
  await page.evaluate(() => {
    const m = window.__mh.state.messages.find((x) => x.subject === 'Yeni cihazdan giriş kodun')!;
    m.unread = true;
  });
  await openArchive(page);
  const line = rowOf(page, 'Yeni cihazdan giriş kodun');
  await expect(line.locator('.row')).toHaveClass(/unread/);

  // Görünüş 24 px; görünmez kenar hedefi 44 px'e çıkarır.
  expect(Math.round((await chip(page).boundingBox())!.height)).toBe(24);
  expect(await hitsAbove(chip(page), 9)).toBe(true);
  expect(await hitsAbove(chip(page), -9)).toBe(true);

  await chip(page).tap();
  await expect(toast(page, 'Kod kopyalandı')).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('731004');
  // Posta açılmadı, seçim kipi başlamadı.
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  await expect(h1(page, 'Arşiv')).toBeVisible();
  await expect(line.locator('.row')).not.toHaveClass(/unread/);
  expect(await page.evaluate(() => window.__mh.state.messages.find((x) => x.subject === 'Yeni cihazdan giriş kodun')!.unread)).toBe(false);
});

test('kod çipi: uzun basış seçim başlatmaz, yatay sürükleme satırı kaydırmaz; seçim kipinde çip yok', async ({ page }) => {
  await openArchive(page);
  const box = (await chip(page).boundingBox())!;
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;

  // Uzun basış (450 ms eşiği) çipin üstünde: seçim kipi açılmaz.
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.waitForTimeout(650);
  await page.mouse.up();
  await expect(h1(page, 'Arşiv')).toBeVisible();
  await expect(page.getByRole('heading', { level: 1, name: '1 seçili' })).toHaveCount(0);

  // Çipten başlayan yatay sürükleme: kaydırma paneli çıkmaz, posta arşivden çıkmaz.
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 40, y, { steps: 3 });
  await expect(list(page).locator('.under')).toHaveCount(0);
  await page.mouse.move(x - 260, y, { steps: 8 });
  await expect(list(page).locator('.under')).toHaveCount(0);
  await page.mouse.up();
  await expect(row(page, 'Yeni cihazdan giriş kodun')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);

  // Satırın geri kalanı her zamanki gibi çalışır: uzun basış seçer; seçimde çip gizlenir.
  await row(page, 'Yeni cihazdan giriş kodun').click({ delay: 650 });
  await expect(page.getByRole('heading', { level: 1, name: '1 seçili' })).toBeVisible();
  await expect(list(page).locator('.otp-chip')).toHaveCount(0);
});

test('satır 360 px’te taşmaz: çip konuyla çakışmaz, satır yüksekliği değişmez', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  await openArchive(page);
  await expect(chip(page)).toBeVisible();
  const line = rowOf(page, 'Yeni cihazdan giriş kodun');
  const chipBox = (await chip(page).boundingBox())!;
  const subject = (await line.locator('.subject').boundingBox())!;
  const rowBox = (await line.boundingBox())!;
  // Çip ekranın içinde ve kenar boşluğunda biter; konu çipin solunda kalır.
  expect(chipBox.x + chipBox.width).toBeLessThanOrEqual(360 - 16 + 0.5);
  expect(subject.x + subject.width).toBeLessThanOrEqual(chipBox.x - 7.5);
  // Çip konu satırının ortasında, satırın içinde.
  expect(Math.abs(chipBox.y + chipBox.height / 2 - (subject.y + subject.height / 2))).toBeLessThanOrEqual(1);
  expect(chipBox.y).toBeGreaterThanOrEqual(rowBox.y);
  expect(chipBox.y + chipBox.height).toBeLessThanOrEqual(rowBox.y + rowBox.height);
  // Çipli satır ötekilerle aynı yükseklikte (yerleşim oynamaz).
  const other = (await rowOf(page, 'Hafta sonu için beş rota').boundingBox())!;
  expect(Math.round(rowBox.height)).toBe(Math.round(other.height));
  expect(Math.round(rowBox.height)).toBe(72);
  // Şüpheli ikonu saatle çakışmaz.
  const warn = rowOf(page, 'Gönderiniz gümrükte bekliyor');
  const iconBox = (await warn.getByRole('img', { name: 'Şüpheli posta' }).boundingBox())!;
  const time = (await warn.locator('.time').boundingBox())!;
  expect(iconBox.x + iconBox.width).toBeLessThanOrEqual(time.x);
  await noOverflow(page);
});

test('sistem yazı ölçeği 1.3×: çip büyüyen satırda konu satırının ortasında kalır', async ({ page }) => {
  await openArchive(page);
  await page.addStyleTag({ content: ':root { --fs-meta: 15.6px; --fs-body: 19.5px; --fs-section: 24.7px; --fs-title: 31.2px; --fs-display: 39px; }' });
  const line = rowOf(page, 'Yeni cihazdan giriş kodun');
  await expect.poll(async () => Math.round((await line.boundingBox())!.height)).toBe(94);
  await expect
    .poll(async () => {
      const c = (await chip(page).boundingBox())!;
      const s = (await line.locator('.subject').boundingBox())!;
      return Math.abs(c.y + c.height / 2 - (s.y + s.height / 2));
    })
    .toBeLessThanOrEqual(1);
});
