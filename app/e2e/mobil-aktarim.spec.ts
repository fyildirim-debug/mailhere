import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil kabuk (Pixel 7): bilgisayardan QR ile aktarım. Hesap ekleme akışının ilk
// seçeneği ve Ayarlar › Cihazlar; kod yapıştırma, sahte kamera, önizleme seçimleri,
// rapor, parolasız hesap için "Parola gir", bozuk kod hataları.

const CODE = 'mailhere://aktar?v=1&h=192.168.1.20&p=51234&t=oKGio6SlpqeoqaqrrK2urw&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8';

const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name });
const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });

async function openDevices(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await expect(h1(page, 'Ayarlar')).toBeVisible();
  const nav = page.getByRole('navigation', { name: 'Ayar bölümleri' });
  // Cihazlar satırı Hesap ekle ile Gizlilik arasında.
  const names = (await nav.getByRole('button').allInnerTexts()).map((s) => s.trim());
  expect(names.indexOf('Cihazlar')).toBeGreaterThan(names.indexOf('Hesap ekle'));
  expect(names.indexOf('Cihazlar')).toBeLessThan(names.indexOf('Gizlilik ve güvenlik'));
  await nav.getByRole('button', { name: 'Cihazlar', exact: true }).click();
  await expect(h1(page, 'Cihazlar')).toBeVisible();
  await expect(page.getByRole('heading', { level: 3, name: 'Bilgisayardan al' })).toBeVisible();
}

test('ilk kurulum: ilk seçenek → kodu yapıştır → önizleme → kural seçimi kaldır → uygula → rapor → gelen kutusu', async ({ page }) => {
  await page.goto('/?kabuk=mobil&bos#/main');
  await expect(page.getByRole('heading', { name: 'Hesap ekle' })).toBeVisible();
  // Seçeneklerin ilki; Google'ın üstünde.
  const alts = page.getByRole('button').filter({ hasText: /QR ile aktar|Google hesabı|Microsoft hesabı/ });
  await expect(alts.first()).toHaveText(/Bilgisayardan QR ile aktar/);
  const first = await alts.first().boundingBox();
  const google = await page.getByRole('button', { name: 'Google hesabı' }).boundingBox();
  expect(first!.y).toBeLessThan(google!.y);
  // Yüzde bire yuvarlanır: yüksek piksel oranında 44 px 43.99998 ölçülebilir.
  expect(Math.round(first!.height * 100) / 100).toBeGreaterThanOrEqual(44);

  await alts.first().click();
  await expect(h1(page, 'Bilgisayardan al')).toBeVisible();
  await expect(page.getByText('aynı Wi-Fi ağında olmalı')).toBeVisible();
  await expect(page.getByRole('button', { name: 'QR’ı okut' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Bağlan', exact: true })).toBeDisabled();
  await page.getByLabel('Aktarım kodu').fill(CODE);
  await page.getByRole('button', { name: 'Bağlan', exact: true }).click();

  // Önizleme: kaynak, iki hesap (ikisi de eklenecek), parolasız olan işaretli, sayılar.
  await expect(h1(page, 'Gelenler')).toBeVisible();
  await expect(page.getByText('Mailhere 0.4.0 · Linux')).toBeVisible();
  const work = page.getByRole('checkbox', { name: 'İş hesabını aktar' });
  const google2 = page.getByRole('checkbox', { name: 'Google hesabını aktar' });
  await expect(work).toBeChecked();
  await expect(google2).toBeChecked();
  await expect(page.getByText('eklenecek')).toHaveCount(2);
  await expect(page.getByText('güncellenecek')).toHaveCount(0);
  await expect(page.getByText('parola sonra sorulacak')).toBeVisible();
  await expect(page.getByText('furkan.yildirim@gmail.com · Google ile giriş')).toBeVisible();
  await expect(page.getByText('3 kural')).toBeVisible();
  await expect(page.getByText('2 etiket')).toBeVisible();
  for (const name of ['Kurallar', 'Etiketler', 'Ayarlar']) await expect(page.getByRole('checkbox', { name, exact: true })).toBeChecked();
  // Kod metni ekranda görünmez (yalnız girdide).
  await expect(page.getByText(/&k=/)).toHaveCount(0);

  await page.getByRole('checkbox', { name: 'Kurallar', exact: true }).uncheck();
  await page.getByRole('button', { name: 'Uygula' }).click();

  // Rapor: 2 hesap eklendi, kural yok, etiketler, ayarlar; parolasız Google.
  await expect(h1(page, 'Aktarıldı')).toBeVisible();
  await expect(page.getByText('2 hesap eklendi')).toBeVisible();
  await expect(page.getByText('2 etiket eklendi')).toBeVisible();
  await expect(page.getByText('Ayarlar uygulandı')).toBeVisible();
  await expect(page.getByText(/kural eklendi|kural atlandı/)).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 3, name: 'Parolası gelmeyen hesaplar' })).toBeVisible();
  await expect(page.getByText('furkan.yildirim@gmail.com')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Yeniden giriş yap' })).toBeVisible();
  const st = await page.evaluate(() => ({ accounts: window.__mh.state.accounts.length, rules: window.__mh.state.rules.length, tags: window.__mh.state.tags.length, theme: window.__mh.state.settings.theme }));
  expect(st).toEqual({ accounts: 2, rules: 0, tags: 2, theme: 'dark' });
  // Ayarlar geldi: koyu tema uygulandı.
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');

  await page.getByRole('button', { name: 'Gelen kutusuna git' }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Yeni posta' })).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});

test('Ayarlar › Cihazlar: sahte kamerayla okut → var olan hesap güncellenecek → uygula → "Parola gir" hesabın sunucu sekmesine gider; gelen kutusu köke döner', async ({ page }) => {
  await page.goto('#/main');
  await openDevices(page);
  await page.getByRole('button', { name: 'QR’ı okut' }).click();
  await expect(page.getByRole('heading', { level: 3, name: 'Gelenler' })).toBeVisible();
  await expect(page.getByText('güncellenecek')).toHaveCount(1);
  await expect(page.getByText('eklenecek')).toHaveCount(1);
  // Önizlemeden geri → kod adımı; kod girdide durur, yeniden bağlanılır.
  await page.getByRole('button', { name: 'Geri', exact: true }).last().click();
  await expect(page.getByRole('heading', { level: 3, name: 'Bilgisayardan al' })).toBeVisible();
  await expect(page.getByLabel('Aktarım kodu')).toHaveValue(/^mailhere:\/\/aktar\?v=1/);
  await page.getByRole('button', { name: 'Bağlan', exact: true }).click();
  await expect(page.getByRole('heading', { level: 3, name: 'Gelenler' })).toBeVisible();

  await page.getByRole('button', { name: 'Uygula' }).click();
  await expect(page.getByRole('heading', { level: 3, name: 'Aktarıldı' })).toBeVisible();
  await expect(page.getByText('1 hesap eklendi')).toBeVisible();
  await expect(page.getByText('1 hesap güncellendi')).toBeVisible();
  await expect(page.getByText('2 kural eklendi')).toBeVisible();
  await expect(page.getByText('1 kural atlandı: hesabı bu telefonda yok')).toBeVisible();
  await expect(page.getByText('1 etiket eklendi')).toBeVisible();

  // Parolasız hesap: düğme hesabın "Sunucu ve parola" sekmesini açar.
  await page.getByRole('button', { name: 'Yeniden giriş yap' }).click();
  await expect(h1(page, 'Google')).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Sunucu ve parola' })).toHaveAttribute('aria-selected', 'true');
  await page.goBack();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('Ayarlar › Cihazlar: "Gelen kutusuna git" yığını tek seferde köke indirir', async ({ page }) => {
  await page.goto('#/main');
  await openDevices(page);
  await page.getByLabel('Aktarım kodu').fill(CODE);
  await page.getByRole('button', { name: 'Bağlan', exact: true }).click();
  await page.getByRole('button', { name: 'Uygula' }).click();
  await expect(page.getByRole('heading', { level: 3, name: 'Aktarıldı' })).toBeVisible();
  await page.getByRole('button', { name: 'Gelen kutusuna git' }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Geri', exact: true })).toHaveCount(0);
  // Geçmiş imleci kökte: ayarlar ekranları geride kalmadı.
  const depth = await page.evaluate(() => (history.state as { mh?: number } | null)?.mh ?? 0);
  expect(depth).toBe(0);
});

test('bozuk, eski sürümlü ve ulaşılamayan kod: ne oldu + ne yapılabilir', async ({ page }) => {
  await page.goto('#/main');
  await openDevices(page);
  const code = page.getByLabel('Aktarım kodu');
  const connect = page.getByRole('button', { name: 'Bağlan', exact: true });

  await code.fill('https://ornek.com/abc');
  await connect.click();
  await expect(page.getByRole('alert')).toContainText('Aktarım kodu tanınmadı');
  await expect(page.getByRole('alert')).toContainText('yeniden okut');

  await code.fill(CODE.replace('v=1', 'v=2'));
  await connect.click();
  await expect(page.getByRole('alert')).toContainText('daha yeni bir Mailhere sürümüyle');
  await expect(page.getByRole('alert')).toContainText('güncelle');

  await code.fill(CODE.replace('192.168.1.20', '10.255.255.1'));
  await connect.click();
  await expect(page.getByRole('alert')).toContainText('Bilgisayara bağlanılamadı');
  await expect(page.getByRole('alert')).toContainText('aynı Wi-Fi');

  // Hata önizlemeye geçirmez; doğru kodla geçer.
  await expect(page.getByRole('heading', { level: 3, name: 'Bilgisayardan al' })).toBeVisible();
  await code.fill(CODE);
  await connect.click();
  await expect(page.getByRole('heading', { level: 3, name: 'Gelenler' })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
});
