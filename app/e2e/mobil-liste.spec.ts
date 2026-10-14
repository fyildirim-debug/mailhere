import { expect, test, type Page } from '@playwright/test';

// Mobil kabuk (Pixel 7, `?kabuk=mobil`): açılış, çekmece, görünüm ve klasör,
// kaydırma eylemleri, uzun basışla seçim, arama, yeni posta, geri tuşu, koyu tema.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

/** Satırı yatay sürükler (fare; kaydırma mantığı pointer olaylarıyla çalışır). */
async function swipe(page: Page, text: string, dx: number) {
  const box = await row(page, text).boundingBox();
  if (!box) throw new Error(`satır bulunamadı: ${text}`);
  const y = box.y + box.height / 2;
  const x0 = dx < 0 ? box.x + 300 : box.x + 20;
  await page.mouse.move(x0, y);
  await page.mouse.down();
  await page.mouse.move(x0 + Math.sign(dx) * 20, y, { steps: 3 });
  await page.mouse.move(x0 + dx, y, { steps: 8 });
  await page.mouse.up();
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('açılış: uygulama çubuğu, liste, yüzen düğme; kenar çubuğu ve okuma alanı yok', async ({ page }) => {
  // Okunmamış sayısı büyük başlığın altında (çubuktaki kopya yalnız kaydırınca görünür).
  await expect(list(page).getByText('3 okunmamış')).toBeVisible();
  await expect(row(page, 'Re: Q4 bütçe taslağı')).toHaveCount(1);
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Yeni posta' })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Kutular ve hesaplar' })).toBeHidden();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  // Yatay taşma yok.
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});

test('çekmece açılır, görünüm değiştirir ve kapanır; Esc de kapatır', async ({ page }) => {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: /Ertelenenler/ }).click();
  await expect(drawer(page)).toBeHidden();
  await expect(page.getByRole('heading', { level: 1, name: 'Ertelenenler' })).toBeVisible();
  await openDrawer(page);
  await page.keyboard.press('Escape');
  await expect(drawer(page)).toBeHidden();
  await expect(page.getByRole('heading', { level: 1, name: 'Ertelenenler' })).toBeVisible();
});

test('çekmeceden hesap klasörüne geçilir', async ({ page }) => {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'İş klasörlerini göster' }).click();
  await drawer(page).getByRole('group', { name: 'İş klasörleri' }).getByRole('button', { name: 'Mailhere 1' }).click();
  await expect(drawer(page)).toBeHidden();
  await expect(page.getByRole('heading', { level: 1, name: 'Mailhere · İş' })).toBeVisible();
  await expect(row(page, 'Yol haritası notları')).toBeVisible();
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
});

test('sola kaydırma arşivler, Geri al geri getirir; sağa kaydırma erteler', async ({ page }) => {
  await swipe(page, 'Cumartesi kahvaltı?', -260);
  await expect(page.getByRole('status')).toContainText('Arşivlendi');
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();

  await swipe(page, 'Gönderiniz dağıtıma çıktı', 260);
  await expect(page.getByRole('status')).toContainText('Ertelendi');
  await expect(row(page, 'Gönderiniz dağıtıma çıktı')).toHaveCount(0);

  // Eşiğin altında kalan kaydırma hiçbir şey yapmaz.
  await swipe(page, 'Sözleşme imzaya hazır', -60);
  await expect(row(page, 'Sözleşme imzaya hazır')).toBeVisible();
});

test('uzun basış seçim kipini açar; dokunma seçimi genişletir; toplu arşivle geri alınır', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click({ delay: 650 });
  await expect(page.getByRole('heading', { level: 1, name: '1 seçili' })).toBeVisible();
  await page.getByRole('button', { name: 'Sözleşme imzaya hazır postasını seç' }).click();
  await expect(page.getByRole('heading', { level: 1, name: '2 seçili' })).toBeVisible();
  await page.getByRole('toolbar', { name: '2 posta seçili' }).getByRole('button', { name: 'Arşivle' }).click();
  await expect(page.getByRole('status')).toContainText('2 posta arşivlendi');
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await expect(row(page, 'Sözleşme imzaya hazır')).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  await expect(row(page, 'Sözleşme imzaya hazır')).toBeVisible();

  // Seçimi bırak düğmesi kipi kapatır.
  await row(page, 'Cumartesi kahvaltı?').click({ delay: 650 });
  await page.getByRole('button', { name: 'Seçimi bırak' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('arama: yazdıkça sonuç, sonuca dokunmak postayı açar, Enter listeyi süzer', async ({ page }) => {
  await page.getByRole('button', { name: 'Ara', exact: true }).click();
  const box = page.getByRole('searchbox', { name: 'Postalarda ara' });
  await expect(box).toBeFocused();
  await box.fill('fatura');
  // Alttaki liste de aynı konuyu taşır (inert); sonuç arama bölgesinden seçilir.
  const hit = page.getByRole('search').getByRole('button', { name: /Ekim faturanız hazır/ });
  await expect(hit).toBeVisible();
  await hit.click();
  await expect(page.getByRole('heading', { level: 2, name: 'Ekim faturanız hazır' })).toBeVisible();
  await page.goBack();
  await expect(box).toBeVisible();
  await box.press('Enter');
  await expect(page.getByRole('heading', { level: 1, name: '“fatura” için sonuçlar' })).toBeVisible();
  await expect(row(page, 'Ekim faturanız hazır')).toBeVisible();
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await page.getByRole('button', { name: 'Aramayı kapat' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('yeni posta düğmesi yazma ekranını iter; kapat liste', async ({ page }) => {
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Yeni posta' })).toBeVisible();
  await page.getByRole('button', { name: 'Yazmayı kapat' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('geri tuşu: önce çekmece kapanır, sonra okuma ekranından listeye dönülür', async ({ page }) => {
  await row(page, 'Re: Q4 bütçe taslağı').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();

  await openDrawer(page);
  await page.goBack();
  await expect(drawer(page)).toBeHidden();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();

  // Kökte geri bizim değildir: önceki sayfa (about:blank) gelir, uygulama çökmez.
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  await page.getByRole('button', { name: 'Geri', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('koyu tema çekmeceden açılır; görüntü bozulmaz', async ({ page }) => {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Temayı değiştir' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.keyboard.press('Escape');
  await expect(drawer(page)).toBeHidden();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(bg).toBe('rgb(15, 17, 21)');
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
  // Koyu temada da satırlar ve yüzen düğme yerinde.
  await expect(page.getByRole('button', { name: 'Yeni posta' })).toBeVisible();
});

test('çok klasörlü hesapta çekmece satırları büzülmez; klasör simgeleri görünümlerle hizalı, son klasöre inilir', async ({ page }) => {
  // Gerçek hesaplarda klasörler ekrana sığmaz: taşan sütunda görünüm satırları
  // yazı yüksekliğine büzülüyor, son klasör alt çubuğun altında kalıyordu.
  await page.waitForFunction(() => !!window.__mh);
  await page.evaluate(() => {
    const acc = window.__mh.state.accounts[0].id;
    for (let i = 1; i <= 14; i++) {
      const name = `Klasör ${String(i).padStart(2, '0')}`;
      window.__mh.state.mailboxes.push({ id: `${acc}:${name}`, accountId: acc, path: name, name, role: 'other', unread: 0, total: 0, parentId: null, hidden: false });
    }
    // Arayüz klasörleri değişiklik olayıyla yeniden okur.
    window.__mh.emit('mh:changed', { mailboxIds: [] });
  });
  // İş kapsamında hesabın klasörleri açık listelenir.
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: /^İş/ }).first().click();
  await expect(drawer(page)).toBeHidden();
  await openDrawer(page);
  const d = drawer(page);
  for (const name of ['Gelen Kutusu', 'Çöp', 'İstenmeyen', 'Klasör 01']) {
    const box = await d.getByRole('button', { name, exact: true }).boundingBox();
    expect(Math.round(box!.height * 100) / 100, name).toBeGreaterThanOrEqual(48);
  }
  // Yaprak klasörün simgesi görünüm simgesiyle aynı sütunda (çekmece kayarken
  // de doğru olsun diye iki simge aynı karede ölçülür).
  const dx = await d.evaluate((el) => {
    const icon = (name: string) =>
      [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === name)!.querySelector('svg')!.getBoundingClientRect().x;
    return Math.abs(icon('Klasör 01') - icon('Gelen Kutusu'));
  });
  expect(dx).toBeLessThan(1);
  // Son klasöre kaydırılır ve alt çubuğun üstünde görünür.
  const last = d.getByRole('button', { name: 'Klasör 14', exact: true });
  await last.scrollIntoViewIfNeeded();
  await expect(last).toBeInViewport({ ratio: 1 });
  const nav = (await d.getByRole('navigation').boundingBox())!;
  const lastBox = (await last.boundingBox())!;
  expect(lastBox.y + lastBox.height).toBeLessThanOrEqual(nav.y + nav.height + 0.5);
});
