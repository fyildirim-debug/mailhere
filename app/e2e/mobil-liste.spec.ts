import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil kabuk (Pixel 7, `?kabuk=mobil`): açılış, çekmece (kenardan açılmaz),
// görünüm ve klasör, ayarlanabilir kaydırma eylemleri, uzun basış ve avatarla
// seçim, başparmak bölgesinde arama ve son aramalar, daralan yüzen düğme,
// kategori sayıları ve hesap noktası, çevrimdışı satırı, yeni posta, geri tuşu,
// koyu tema, sistem yazı ölçeği.

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
  // Alan altta (başparmak bölgesi, klavyenin hemen üstü).
  const field = (await box.boundingBox())!;
  expect(field.y).toBeGreaterThan((await page.evaluate(() => innerHeight)) - 100);
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

const h1 = (page: Page, name: string | RegExp) => page.getByRole('heading', { level: 1, name });
const haptics = (page: Page) => page.evaluate(() => window.__mh.state.haptics);
/** Liste kaydırıcısını verilen konuma getirir (scroll olayı üretir). */
const scrollTo = (page: Page, top: number) => page.locator('.scroller').evaluate((el, y) => el.scrollTo({ top: y }), top);

async function openGeneral(page: Page) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: 'Genel', exact: true }).click();
  await expect(h1(page, 'Genel')).toBeVisible();
}

test('kaydırma eylemleri ayarlanır: sola sil, sağa okundu; eşiğe kadar nötr, eşikte renk ve titreşim', async ({ page }) => {
  await openGeneral(page);
  const left = page.getByRole('button', { name: /^Sola kaydırma/ });
  const right = page.getByRole('button', { name: /^Sağa kaydırma/ });
  await expect(left).toContainText('Arşivle');
  await expect(right).toContainText('Yarına ertele');
  await left.click();
  const leftSheet = page.getByRole('dialog', { name: 'Sola kaydırma' });
  await expect(leftSheet.getByRole('menuitemcheckbox', { name: 'Arşivle' })).toHaveAttribute('aria-checked', 'true');
  // Her yön için sekiz seçenek, ikonlarıyla.
  await expect(leftSheet.getByRole('menuitemcheckbox')).toHaveText(['Arşivle', 'Sil', 'İstenmeyen', 'Klasöre taşı', 'Yarına ertele', 'Okundu / okunmadı', 'Sabitle', 'Yok']);
  await expect(leftSheet.getByRole('menuitemcheckbox').locator('svg:first-child')).toHaveCount(8);
  await leftSheet.getByRole('menuitemcheckbox', { name: 'Sil' }).click();
  await expect(left).toContainText('Sil');
  await right.click();
  await page.getByRole('dialog', { name: 'Sağa kaydırma' }).getByRole('menuitemcheckbox', { name: 'Okundu / okunmadı' }).click();
  await expect(right).toContainText('Okundu / okunmadı');
  await page.goBack();
  await page.goBack();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();

  // Eşiğe kadar nötr (kâğıt zemin, titreşim yok); eşik geçilince tehlike rengi ve tek titreşim.
  const box = (await row(page, 'Cumartesi kahvaltı?').boundingBox())!;
  const y = box.y + box.height / 2;
  const x0 = box.x + 300;
  await page.mouse.move(x0, y);
  await page.mouse.down();
  await page.mouse.move(x0 - 20, y, { steps: 3 });
  await page.mouse.move(x0 - 90, y, { steps: 4 });
  const under = page.locator('.under[data-action="trash"]');
  await expect(under).toBeVisible();
  await expect(under).toContainText('Sil');
  expect(await under.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe('rgb(255, 255, 255)');
  expect(await haptics(page)).toEqual([]);
  await page.mouse.move(x0 - 260, y, { steps: 8 });
  await expect.poll(() => under.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe('rgb(194, 55, 46)');
  await expect.poll(() => haptics(page)).toEqual(['tick']);
  await page.mouse.up();
  await expect(page.getByRole('status')).toContainText('Çöp kutusuna taşındı');
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();

  // Sağa: okundu sayar; satır listede kalır, geri alınabilir.
  await expect(list(page).getByText('3 okunmamış')).toBeVisible();
  await swipe(page, 'Gönderiniz dağıtıma çıktı', 260);
  await expect(page.getByRole('status')).toContainText('1 posta okundu sayıldı');
  await expect(row(page, 'Gönderiniz dağıtıma çıktı')).toBeVisible();
  await expect(list(page).getByText('2 okunmamış')).toBeVisible();

  // "Hiçbir şey": o yönde satır kaymaz, eylem çalışmaz.
  await page.evaluate(() => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, swipeLeft: 'none' } }));
  await swipe(page, 'Sözleşme imzaya hazır', -260);
  await page.waitForTimeout(300);
  await expect(row(page, 'Sözleşme imzaya hazır')).toBeVisible();
  expect(await page.evaluate(() => window.__mh.state.messages.find((m) => m.subject === 'Sözleşme imzaya hazır')!.mailboxId)).toMatch(/INBOX$/);
});

/** Kaydırma ayarını sahte arka uçtan yazar (ayar ekranı ayrı testte). */
async function setSwipe(page: Page, side: 'swipeLeft' | 'swipeRight', action: string) {
  await page.evaluate(
    ([k, v]) => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, [k]: v } }),
    [side, action] as const,
  );
}

test('kaydırma: İstenmeyen hesabın İstenmeyen kutusuna taşır, geri alınır; kutusu olmayan hesapta satır yerinde kalır, ne yapılacağı söylenir', async ({ page }) => {
  await setSwipe(page, 'swipeLeft', 'spam');
  // Kişisel hesabın İstenmeyen kutusu var.
  const box = (await row(page, 'Cumartesi kahvaltı?').boundingBox())!;
  await page.mouse.move(box.x + 300, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 280, box.y + box.height / 2, { steps: 3 });
  await page.mouse.move(box.x + 40, box.y + box.height / 2, { steps: 8 });
  const under = page.locator('.under[data-action="spam"]');
  await expect(under).toContainText('İstenmeyen');
  await page.mouse.up();
  await expect(page.getByRole('status')).toContainText('İstenmeyen klasörüne taşındı');
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  const boxOf = (subject: string) => page.evaluate((s) => window.__mh.state.messages.find((m) => m.subject === s)!.mailboxId, subject);
  expect(await boxOf('Cumartesi kahvaltı?')).toBe('demo-kisisel:Junk');
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  expect(await boxOf('Cumartesi kahvaltı?')).toBe('demo-kisisel:INBOX');

  // İş hesabının İstenmeyen kutusu yok (rolü eşlenmemiş): satır geri yaylanır, posta yerinde.
  await page.evaluate(() => {
    window.__mh.state.mailboxes = window.__mh.state.mailboxes.filter((b) => !(b.accountId === 'demo-is' && b.role === 'junk'));
    window.__mh.emit('mh:changed', { mailboxIds: [] });
  });
  await page.waitForTimeout(400);
  await swipe(page, 'Re: Q4 bütçe taslağı', -260);
  await expect(page.getByRole('status')).toContainText('Bu hesapta İstenmeyen klasörü yok. Hesap ayarları › Klasörler’den');
  await expect(row(page, 'Re: Q4 bütçe taslağı')).toBeVisible();
  expect(await boxOf('Re: Q4 bütçe taslağı')).toBe('demo-is:INBOX');
  expect(await haptics(page)).toContain('reject');
});

test('kaydırma: Klasöre taşı satırı yerine döndürür ve klasör seçiciyi açar; seçilen klasöre taşınır, geri alınır', async ({ page }) => {
  await setSwipe(page, 'swipeRight', 'move');
  await swipe(page, 'Ekim faturanız hazır', 260);
  const sheet = page.getByRole('dialog', { name: 'Klasöre taşı' });
  await expect(sheet).toBeVisible();
  // Satır listede (seçim bitmeden taşınmaz); seçici hesabın klasörlerini gösterir, bulunduğu kutu yok.
  await expect(row(page, 'Ekim faturanız hazır')).toBeVisible();
  await expect(sheet.getByRole('menuitem', { name: 'Faturalar' })).toBeVisible();
  await expect(sheet.getByRole('menuitem', { name: 'Gelen Kutusu' })).toHaveCount(0);
  // Vazgeçmek bir şey yapmaz.
  await page.keyboard.press('Escape');
  await expect(sheet).toBeHidden();
  await expect(row(page, 'Ekim faturanız hazır')).toBeVisible();

  await swipe(page, 'Ekim faturanız hazır', 260);
  await sheet.getByRole('menuitem', { name: 'Faturalar' }).click();
  await expect(page.getByRole('status')).toContainText('Faturalar klasörüne taşındı');
  await expect(row(page, 'Ekim faturanız hazır')).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, 'Ekim faturanız hazır')).toBeVisible();
  // Alt sayfa geri tuşu girdisini geri aldı: geri yığında bir şey bırakmaz (kökte liste kalır).
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('başparmak bölgesi: arama ve yeni posta altta; aşağı kaydırınca yeni posta ikona iner, yukarı kaydırınca adı gelir', async ({ page }) => {
  // Üst çubukta arama yok; arama 48 px yuvarlak, yeni postanın solunda ve aynı hizada.
  await expect(page.getByRole('banner').getByRole('button', { name: 'Ara' })).toHaveCount(0);
  const search = page.getByRole('button', { name: 'Ara', exact: true });
  const fab = page.getByRole('button', { name: 'Yeni posta' });
  const s = (await search.boundingBox())!;
  const f = (await fab.boundingBox())!;
  expect([Math.round(s.width), Math.round(s.height)]).toEqual([48, 48]);
  expect(s.x + s.width).toBeLessThanOrEqual(f.x);
  expect(Math.abs(s.y + s.height / 2 - (f.y + f.height / 2))).toBeLessThan(2);
  expect(f.y + f.height).toBeGreaterThan((await page.evaluate(() => innerHeight)) - 40);
  expect(f.width).toBeGreaterThan(120);
  await expect(fab).toContainText('Yeni posta');

  await scrollTo(page, 200);
  await scrollTo(page, 420);
  await expect.poll(async () => Math.round((await fab.boundingBox())!.width)).toBe(56);
  // Ad gizlense de düğmenin adı okunur; sağ kenar yerinde kalır.
  await expect(fab).toHaveAccessibleName('Yeni posta');
  expect(Math.round((await fab.boundingBox())!.x + 56)).toBe(Math.round(f.x + f.width));
  // Küçük geri kaydırma (titreme) yönü değiştirmez; belirgin yukarı kaydırma adı geri getirir.
  await scrollTo(page, 414);
  await page.waitForTimeout(250);
  expect(Math.round((await fab.boundingBox())!.width)).toBe(56);
  await scrollTo(page, 300);
  await expect.poll(async () => (await fab.boundingBox())!.width).toBeGreaterThan(120);
});

test('avatara dokunmak seçimi başlatır; uzun basış titreşir; "Tümünü seç" görünenlerin hepsini işaretler', async ({ page }) => {
  // Avatar ayrı düğme: dokunmak postayı açmaz, seçer.
  const avatar = page.getByRole('button', { name: 'Cumartesi kahvaltı? postasını seç' });
  const box = (await avatar.boundingBox())!;
  expect(Math.min(box.width, box.height)).toBeGreaterThanOrEqual(44);
  await avatar.click();
  await expect(h1(page, '1 seçili')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  await page.getByRole('button', { name: 'Sözleşme imzaya hazır postasını seç' }).click();
  await expect(h1(page, '2 seçili')).toBeVisible();

  const all = page.getByRole('button', { name: 'Tümünü seç' });
  await expect(all).toBeVisible();
  await all.click();
  await expect(h1(page, /^\d+ seçili$/)).not.toHaveText('2 seçili');
  const n = Number((await h1(page, /^\d+ seçili$/).textContent())!.split(' ')[0]);
  expect(n).toBeGreaterThan(8);
  await expect(page.getByRole('toolbar', { name: `${n} posta seçili` })).toBeVisible();
  await expect(all).toBeDisabled();
  await page.getByRole('button', { name: 'Seçimi bırak' }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();

  // Uzun basış da seçer; onay titreşimi istenir.
  await row(page, 'Cumartesi kahvaltı?').click({ delay: 650 });
  await expect(h1(page, '1 seçili')).toBeVisible();
  expect(await haptics(page)).toContain('confirm');
});

test('kategori çiplerinde okunmamış sayısı (12/500), okununca azalır; birleşik görünümde satırda hesap noktası', async ({ page }) => {
  const people = page.getByRole('button', { name: 'Kişiler, 1 okunmamış' });
  await expect(people).toBeVisible();
  await expect(page.getByRole('button', { name: 'Tümü, 3 okunmamış' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Bildirimler, 1 okunmamış' })).toBeVisible();
  const count = people.locator('.m-chip-n');
  await expect(count).toHaveText('1');
  expect(await count.evaluate((el) => [getComputedStyle(el).fontSize, getComputedStyle(el).fontWeight])).toEqual(['12px', '500']);

  // Birleşik görünüm (üç hesap): saatin yanında 8 px hesap rengi noktası; renkli kenar çizgisi yok.
  const dot = list(page).getByRole('img', { name: 'Hesap: Kişisel' }).first();
  await expect(dot).toBeVisible();
  const d = (await dot.boundingBox())!;
  expect([Math.round(d.width), Math.round(d.height)]).toEqual([8, 8]);
  const rowEl = list(page).locator('.row', { hasText: 'Cumartesi kahvaltı?' });
  expect(await rowEl.evaluate((el) => getComputedStyle(el).borderLeftWidth)).toBe('0px');
  // Nokta saatle aynı satırda, hemen solunda.
  const time = (await rowEl.locator('.time').boundingBox())!;
  expect(Math.abs(d.y + d.height / 2 - (time.y + time.height / 2))).toBeLessThan(2);

  // Okununca sayı azalır; sıfırsa çipte sayı yazılmaz.
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole('button', { name: 'Tümü, 2 okunmamış' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Kişiler', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Kişiler', exact: true }).locator('.m-chip-n')).toHaveCount(0);

  // Tek hesap kapsamında nokta yok.
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: /^İş/ }).first().click();
  await expect(drawer(page)).toBeHidden();
  await expect(row(page, 'Re: Q4 bütçe taslağı')).toBeVisible();
  await expect(list(page).getByRole('img', { name: /^Hesap: / })).toHaveCount(0);
});

test('çevrimdışı: başlığın altında durum ve bekleyen gönderim sayısı; bağlantı gelince söner', async ({ page, context }) => {
  const status = list(page).getByRole('status');
  await expect(status).toHaveCount(0);
  await context.setOffline(true);
  await expect(status).toHaveText('Çevrimdışı · değişiklikler bağlantı gelince eşitlenir');
  expect(await status.evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px');

  // Çevrimdışı gönderilen hızlı yanıt giden kutusunda bekler ve sayılır.
  await page.evaluate(() => {
    window.__mh.state.settings.undoSendSeconds = 0;
  });
  await row(page, 'Cumartesi kahvaltı?').click();
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Yanıtla', exact: true }).click();
  await page.getByRole('textbox', { name: 'Hızlı yanıt yaz…' }).fill('Geliyorum.');
  await page.getByRole('region', { name: 'Yanıt' }).getByRole('button', { name: 'Gönder' }).click();
  await expect(page.getByRole('region', { name: 'Yanıt' })).toBeHidden();
  await page.goBack();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await expect(status).toHaveText('Çevrimdışı · 1 gönderim bekliyor', { timeout: 10_000 });

  // Bağlantı gelince satır söner, gönderim gider.
  await context.setOffline(false);
  await expect(status).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__mh.call('outbox_list').then((l) => l.length))).toBe(0);
});

test('arama ekranı: boşken işleçler; son aramalar cihazda tutulur, dokununca yeniden aranır, silinir', async ({ page }) => {
  const search = page.getByRole('button', { name: 'Ara', exact: true });
  await search.click();
  const box = page.getByRole('searchbox', { name: 'Postalarda ara' });
  const recent = page.getByRole('group', { name: 'Son aramalar' });
  await expect(page.getByText('Arama işleçleri')).toBeVisible();
  await expect(recent).toHaveCount(0);
  // İşleç ipucu sorguya eklenir.
  await page.getByRole('button', { name: /^from:/ }).click();
  await expect(box).toHaveValue('from:');
  await box.fill('fatura');
  await box.press('Enter');
  await expect(h1(page, '“fatura” için sonuçlar')).toBeVisible();
  await page.getByRole('button', { name: 'Aramayı kapat' }).click();

  await search.click();
  await expect(recent.getByRole('button', { name: 'fatura' })).toBeVisible();
  await recent.getByRole('button', { name: 'fatura' }).click();
  await expect(box).toHaveValue('fatura');
  await expect(page.getByRole('search').getByRole('button', { name: /Ekim faturanız hazır/ })).toBeVisible();
  await box.fill('');
  await page.getByRole('button', { name: 'Geçmişi sil' }).click();
  await expect(recent).toHaveCount(0);
  await expect(box).toBeFocused();
  // Yeniden açılışta da yok (cihazdan silindi).
  await page.getByRole('button', { name: 'Geri', exact: true }).click();
  await search.click();
  await expect(page.getByText('Arama işleçleri')).toBeVisible();
  await expect(recent).toHaveCount(0);
});

test('çekmece kenardan kaydırmayla açılmaz (sistemin geri hareketi); menü düğmesiyle açılır, sola kaydırınca kapanır', async ({ page }) => {
  // Büyük başlığın hizasında sol kenardan sağa sürükleme.
  await page.mouse.move(3, 130);
  await page.mouse.down();
  await page.mouse.move(40, 130, { steps: 3 });
  await page.mouse.move(220, 130, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  await expect(drawer(page)).toBeHidden();

  await openDrawer(page);
  // Açılış hareketi (280 ms) bitsin: sürükleme çekmecenin üstünde başlasın.
  await page.waitForTimeout(400);
  await page.mouse.move(250, 500);
  await page.mouse.down();
  await page.mouse.move(230, 500, { steps: 3 });
  await page.mouse.move(40, 500, { steps: 8 });
  await page.mouse.up();
  await expect(drawer(page)).toBeHidden();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('sistem yazı ölçeği 1.3×: satırlar büyür; liste, çipler, çubuklar ve şeritte metin kırpılmaz', async ({ page }) => {
  // Android WebView kullanıcının yazı büyütmesini px yazılara uygular; ölçek jetonlarıyla taklit edilir.
  await page.addStyleTag({ content: ':root { --fs-meta: 15.6px; --fs-body: 19.5px; --fs-section: 24.7px; --fs-title: 31.2px; --fs-display: 39px; }' });
  const slot = page.locator('.slot:not(.group-slot)').first();
  await expect.poll(() => slot.evaluate((el) => Math.round(el.getBoundingClientRect().height))).toBe(94);

  /**
   * Görünür metni kutusunun dışına taşan öğeler: her metin parçasının satır
   * kutuları öğenin dikey sınırları içinde mi (scrollHeight süs katmanlarını,
   * ör. çipin görünmez dokunma kenarını da sayar; burada yalnız metne bakılır).
   */
  const clipped = (selectors: string[]) =>
    page.evaluate((sel) => {
      const bad: string[] = [];
      const hiddenUnder = (node: Element | null, root: Element) => {
        for (let p = node; p && p !== root.parentElement; p = p.parentElement) {
          const cs = getComputedStyle(p);
          if (cs.opacity === '0' || cs.visibility === 'hidden' || cs.display === 'none') return true;
        }
        return false;
      };
      for (const s of sel) {
        for (const el of document.querySelectorAll<HTMLElement>(s)) {
          if (!el.offsetParent || el.closest('[inert]')) continue;
          const box = el.getBoundingClientRect();
          const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
          for (let n = walker.nextNode(); n; n = walker.nextNode()) {
            if (!n.textContent?.trim() || hiddenUnder(n.parentElement, el)) continue;
            const range = document.createRange();
            range.selectNodeContents(n);
            const out = [...range.getClientRects()].some((r) => r.top < box.top - 0.5 || r.bottom > box.bottom + 0.5);
            if (out) {
              bad.push(`${s}: ${n.textContent.trim().slice(0, 30)}`);
              break;
            }
          }
        }
      }
      return bad;
    }, selectors);

  expect(await clipped(['.row', '.m-chip', '.m-fab', '.m-search-fab', '.appbar', '.group'])).toEqual([]);
  // Satır metninin son satırı satırın içinde.
  const r = list(page).locator('.row', { hasText: 'Cumartesi kahvaltı?' });
  const rowBox = (await r.boundingBox())!;
  const snippet = (await r.locator('.snippet').boundingBox())!;
  expect(snippet.y + snippet.height).toBeLessThanOrEqual(rowBox.y + rowBox.height + 0.5);

  // Okuma ekranı: şerit ve yanıt alanı.
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  expect(await clipped(['.strip', '.strip button', '.appbar'])).toEqual([]);
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Yanıtla', exact: true }).click();
  expect(await clipped(['.replying', '.replying .chip', '.replying .head'])).toEqual([]);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});
