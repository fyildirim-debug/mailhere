import { expect, test, type Page } from '@playwright/test';

// Mobil okuma ekranı (Pixel 7, `?kabuk=mobil`): satırdan açılış, alt eylem
// çubuğu, arşivleyince listeye dönüş, diğer eylemler alt sayfası (taşı, etiket,
// ertele), kişi kartı alt sayfası, geri tuşu sırası (alt sayfa → ekran).

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const bar = (page: Page) => page.getByRole('toolbar', { name: 'Posta eylemleri' });
const sheet = (page: Page, name: string) => page.getByRole('dialog', { name });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });
/** Üstteki ekranın uygulama çubuğu (alttaki liste ekranı inert ama DOM'da durur). */
const appbar = (page: Page) => page.getByRole('banner').last();

async function openMore(page: Page) {
  await appbar(page).getByRole('button', { name: 'Diğer eylemler' }).click();
  await expect(sheet(page, 'Diğer eylemler')).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('satır okuma ekranını açar: konu, alt çubuk, hızlı yanıt; arşivle listeye döner ve geri alınır', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  // Uygulama çubuğunda başlık yok; konu gövdede 19 px.
  await expect(page.getByRole('heading', { level: 1 })).toHaveCount(1);
  const subjectSize = await page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' }).evaluate((el) => getComputedStyle(el).fontSize);
  expect(subjectSize).toBe('19px');

  for (const name of ['Yanıtla', 'Tümüne', 'İlet', 'Arşivle', 'Sil']) {
    await expect(bar(page).getByRole('button', { name, exact: true })).toBeVisible();
  }
  const barBox = await bar(page).boundingBox();
  expect(barBox?.height).toBe(56);
  // Hızlı yanıt çipleri değişkeni doldurur; Yanıtla kutuya odaklanır.
  await page.getByRole('group', { name: 'Hızlı yanıtlar' }).getByRole('button', { name: 'Teşekkürler!' }).click();
  await expect(page.getByRole('textbox', { name: 'Hızlı yanıt yaz…' })).toHaveValue('Teşekkürler!');
  await expect(page.getByText('Kime: ayse.demir@eposta.com')).toBeVisible();
  // Yatay taşma yok.
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);

  await bar(page).getByRole('button', { name: 'Arşivle' }).click();
  await expect(toast(page, 'Arşivlendi')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
});

test('yıldız ekranda kalır; okunmadı say listeye döner ve sayıyı artırır', async ({ page }) => {
  await expect(page.getByText('3 okunmamış')).toBeVisible();
  await row(page, 'Sözleşme imzaya hazır').click();
  const star = appbar(page).getByRole('button', { name: 'Yıldızı kaldır' });
  await expect(star).toHaveAttribute('aria-pressed', 'true');
  await star.click();
  await expect(appbar(page).getByRole('button', { name: 'Yıldızla' })).toHaveAttribute('aria-pressed', 'false');
  // İletideki yıldız da aynı duruma düşer.
  await expect(page.getByRole('list', { name: 'Konuşmadaki iletiler' }).getByRole('button', { name: 'Yıldızla' })).toBeVisible();
  await expect(page.getByRole('heading', { level: 2, name: 'Sözleşme imzaya hazır' })).toBeVisible();

  await appbar(page).getByRole('button', { name: 'Okunmadı say' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
  await expect(page.getByText('4 okunmamış')).toBeVisible();
});

test('diğer eylemler alt sayfası: yazdırma yok; etiket işaretlenir, klasöre taşı listeye döner', async ({ page }) => {
  await row(page, 'Ekim faturanız hazır').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Ekim faturanız hazır' })).toBeVisible();
  await openMore(page);
  const more = sheet(page, 'Diğer eylemler');
  await expect(more.getByRole('menuitem', { name: 'Yazdır' })).toHaveCount(0);
  await expect(more.getByRole('menuitem', { name: 'Hızlı okumada aç' })).toHaveCount(0);
  await expect(more.getByRole('menuitem', { name: 'Farklı kaydet (.eml)' })).toBeVisible();
  // Alt sayfa satırları 48 px.
  const item = await more.getByRole('menuitem', { name: 'Klasöre taşı' }).boundingBox();
  expect(item?.height).toBe(48);

  // Etiketler: aynı alt sayfa içerik değiştirir; işaret okuma ekranına çip olarak düşer.
  await more.getByRole('menuitem', { name: 'Etiketler' }).click();
  const tags = sheet(page, 'Etiketler');
  await expect(tags.getByRole('menuitemcheckbox', { name: 'Fatura' })).toHaveAttribute('aria-checked', 'true');
  await tags.getByRole('menuitemcheckbox', { name: 'Proje' }).click();
  await expect(tags).toBeHidden();
  await expect(page.getByRole('group', { name: 'Etiketler: Fatura, Proje' })).toBeVisible();
  await expect(page.getByRole('heading', { level: 2, name: 'Ekim faturanız hazır' })).toBeVisible();

  // Klasöre taşı: liste gelir, toast geri alır.
  await openMore(page);
  await sheet(page, 'Diğer eylemler').getByRole('menuitem', { name: 'Klasöre taşı' }).click();
  await sheet(page, 'Klasöre taşı').getByRole('menuitem', { name: 'Faturalar' }).click();
  await expect(toast(page, 'Faturalar klasörüne taşındı')).toBeVisible();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
  await expect(row(page, 'Ekim faturanız hazır')).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, 'Ekim faturanız hazır')).toBeVisible();
});

test('ertele alt sayfası (menüden menüye) listeye döner', async ({ page }) => {
  await row(page, 'Gönderiniz dağıtıma çıktı').click();
  await openMore(page);
  await sheet(page, 'Diğer eylemler').getByRole('menuitem', { name: 'Ertele' }).click();
  const snooze = sheet(page, 'Ne zaman geri gelsin?');
  await expect(snooze).toBeVisible();
  await snooze.getByRole('menuitem', { name: /Yarın/ }).click();
  await expect(toast(page, 'Ertelendi')).toBeVisible();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
  await expect(row(page, 'Gönderiniz dağıtıma çıktı')).toHaveCount(0);
});

test('kişi kartı alt sayfada açılır; yeni posta alıcıyı doldurur', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click();
  await page.getByRole('button', { name: 'Ayşe Demir kişi kartını aç' }).click();
  const card = sheet(page, 'Kişi kartı');
  await expect(card).toBeVisible();
  await expect(card.getByText('ayse.demir@eposta.com')).toBeVisible();
  await expect(card.getByRole('button', { name: 'Önemli kişi yap' })).toBeVisible();
  // Esc alt sayfayı kapatır, okuma ekranı kalır.
  await page.keyboard.press('Escape');
  await expect(card).toBeHidden();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();

  await page.getByRole('button', { name: 'Ayşe Demir kişi kartını aç' }).click();
  await sheet(page, 'Kişi kartı').getByRole('button', { name: 'Yeni posta yaz' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Yeni posta' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Ayşe Demir alıcısını kaldır' })).toBeVisible();
  await expect(page.locator('#compose-from')).toHaveValue('demo-kisisel');
});

test('geri tuşu sırası: önce alt sayfa kapanır, sonra okuma ekranı', async ({ page }) => {
  await row(page, 'Re: Q4 bütçe taslağı').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await openMore(page);
  await page.goBack();
  await expect(sheet(page, 'Diğer eylemler')).toBeHidden();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();

  // Örtüye dokunma da kapatır; ekran yerinde kalır ve ardından geri tek adımda listeye döner.
  await row(page, 'Re: Q4 bütçe taslağı').click();
  await openMore(page);
  await page.mouse.click(200, 80);
  await expect(sheet(page, 'Diğer eylemler')).toBeHidden();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await page.getByRole('button', { name: 'Geri', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});

test('konuşma görünümü: iletiler katlı gelir, komşuya kaydırma sonraki postayı açar', async ({ page }) => {
  await row(page, 'Re: Yayın planı').click();
  const article = page.getByRole('heading', { level: 2, name: 'Re: Yayın planı' });
  await expect(article).toBeVisible();
  await expect(page.getByText('4 ileti')).toBeVisible();
  await expect(page.getByRole('button', { name: /iletisini aç$/ })).toHaveCount(3);

  // Sola kaydırma: listede bir sonraki posta aynı ekranda.
  const box = await article.boundingBox();
  if (!box) throw new Error('konu bulunamadı');
  const y = box.y + box.height / 2;
  await page.mouse.move(360, y);
  await page.mouse.down();
  await page.mouse.move(330, y, { steps: 3 });
  await page.mouse.move(120, y, { steps: 8 });
  await page.mouse.up();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Yayın planı' })).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(1);
  // Geri tek adımda listeye döner (değiştirme yeni geçmiş girdisi üretmez).
  await page.goBack();
  await expect(page.getByRole('heading', { level: 1, name: 'Gelen Kutusu' })).toBeVisible();
});
