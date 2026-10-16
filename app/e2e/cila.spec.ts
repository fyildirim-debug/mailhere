import { expect, test, type Locator, type Page } from '@playwright/test';
import { contrast, hitsAbove } from './kontrast';

// Son dokunuş (cila) kabulleri: görünüme göre boş durum, kırpılmayan menü,
// metin karşıtlığı (≥ 4.5:1, iki temada), tık hedefleri (≥ 28 px) ve küçük
// düzen kuralları. Her test bir cila maddesinin kabul ölçütünü tutar.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const side = (page: Page) => page.getByRole('navigation', { name: 'Kutular ve hesaplar' });
const reader = (page: Page) => page.getByRole('main');

async function openSettings(page: Page, section: string) {
  const nav = page.getByRole('navigation', { name: 'Ayarlar' });
  if (!(await nav.isVisible())) await side(page).getByRole('button', { name: 'Ayarlar' }).click();
  await nav.getByRole('button', { name: section, exact: true }).click();
}

/** Arama paletinden görünüme gider (ör. "Arşive git"). */
async function goView(page: Page, command: string) {
  await page.keyboard.press('Control+k');
  await page.keyboard.type(command);
  await page.getByRole('dialog', { name: 'Komut paleti' }).getByRole('option', { name: new RegExp(`^${command}`) }).first().click();
}

async function open(page: Page) {
  await page.goto('/#/main');
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
}

/** Kutunun genişliği ve yüksekliği, piksel altı yuvarlamasız. */
async function size(loc: Locator): Promise<[number, number]> {
  const b = (await loc.boundingBox())!;
  return [Math.round(b.width), Math.round(b.height)];
}

async function expectAA(loc: Locator) {
  await expect(loc).toBeVisible();
  expect(await contrast(loc)).toBeGreaterThanOrEqual(4.5);
}

// ------------------------------------------------------------ boş durum

test('boş durum görünümü tanır: Ertelenenler, Çöp ve arama sonucu kendi metnini söyler', async ({ page }) => {
  await open(page);
  const empty = list(page).locator('.empty');

  await side(page).getByRole('button', { name: /^Ertelenenler/ }).click();
  await expect(empty.locator('.empty-title')).toHaveText('Ertelenmiş posta yok');
  await expect(empty.locator('.empty-body')).toHaveText('Ertelediğin postalar zamanı gelince gelen kutusuna döner.');

  await page.keyboard.press('Control+k');
  await page.keyboard.type('zzzz');
  await page.keyboard.press('Enter');
  await expect(list(page).getByRole('heading', { level: 1 })).toHaveText('“zzzz” için sonuçlar');
  await expect(empty.locator('.empty-title')).toHaveText('Sonuç yok');
  await expect(empty.locator('.empty-body')).toContainText('Bu aramayla eşleşen posta yok.');
  await expect(empty.locator('.empty-icon .icon')).toHaveCount(1);
});

// ------------------------------------------------------------ menü

test('menü etiketleri kırpılmaz ve pencerede kalır; kısayol kısayol çipinde', async ({ page }) => {
  await open(page);
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Bu göndericinin tüm postalarını arşivle' })).toBeVisible();

  const clipped = await menu.locator('.menu-item .trunc').evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent));
  expect(clipped).toEqual([]);
  const box = (await menu.boundingBox())!;
  const width = page.viewportSize()!.width;
  expect(box.x + box.width).toBeLessThanOrEqual(width - 8);

  // "Okunmadı say" kısayolu düz metin değil, 20 px kısayol çipi.
  const kbd = menu.getByRole('menuitem', { name: /^Okunmadı say/ }).locator('kbd.kbd');
  await expect(kbd).toHaveText('U');
  expect((await size(kbd))[1]).toBe(20);
});

test('Kimden çipi menüsünde ad ve adres kırpılmaz', async ({ page }) => {
  await open(page);
  await page.keyboard.press('Control+k');
  await page.keyboard.type('fatura');
  await page.keyboard.press('Enter');
  await list(page).getByRole('group', { name: 'Arama süzgeçleri' }).getByRole('button', { name: 'Kimden' }).click();
  const menu = page.getByRole('menu');
  await expect(menu.getByRole('menuitemcheckbox', { name: /fatura@bulutfatura\.com\.tr/ })).toBeVisible();
  const clipped = await menu.locator('.menu-item .trunc').evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent));
  expect(clipped).toEqual([]);
});

// ------------------------------------------------------------ karşıtlık

for (const scheme of ['light', 'dark'] as const) {
  test(`karşıtlık (${scheme}): seçili satırın saati ve önizlemesi, Yeni posta kısayol harfi`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await open(page);
    const row = list(page).locator('.row.selected');
    await expectAA(row.locator('.time'));
    await expectAA(row.locator('.snippet'));
    await expectAA(side(page).locator('.compose .key'));
  });
}

for (const scheme of ['light', 'dark'] as const) {
  test(`karşıtlık (${scheme}): Ayarlar › Hesaplar rozet harfleri, hesap sayfası rozeti`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto('/#/main');
    await openSettings(page, 'Hesaplar');
    const badges = page.locator('.acc .badge');
    await expect(badges).toHaveCount(3);
    for (const b of await badges.all()) await expectAA(b);
    await page.locator('button.acc').filter({ has: page.locator('.badge') }).first().click();
    await expectAA(page.locator('.badge').first());
  });
}

test('karşıtlık: kural uyarısı (amber) ve hesap ayarındaki güvenlik etiketi (başarı) AA', async ({ page }) => {
  await page.goto('/#/main');
  await openSettings(page, 'Kurallar');
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  await page.getByRole('combobox', { name: /^Hesap/ }).selectOption({ label: 'Tüm hesaplar' });
  await page.getByLabel('Eylem 1').selectOption({ label: 'Klasöre taşı' });
  await expectAA(page.locator('.hint.warn'));

  await page.goto('/?bos#/main');
  await page.getByLabel('E-posta adresi').fill('furkan@ornek.com.tr');
  await page.getByRole('button', { name: 'Devam' }).click();
  await expect(page.getByRole('heading', { name: 'Ayarlar bulundu' })).toBeVisible();
  await expectAA(page.locator('.sec').first());
});

// ------------------------------------------------------------ tık hedefleri

test('tık hedefleri: metin bağlantısı görünmez kenarla ≥ 28, çip kaldır 28×28, anahtar ve renk seçici', async ({ page }) => {
  await open(page);
  await list(page).getByText('Sayı 142 · Sakin arayüzler üzerine').click();
  const unsub = reader(page).getByRole('button', { name: 'Abonelikten çık' });
  await expect(unsub).toBeVisible();
  expect(await hitsAbove(unsub, 5)).toBe(true);
  expect(await hitsAbove(unsub, -5)).toBe(true);
  // Uzak içerik şeridinin metni şeridin tüm yüksekliğini kaplar.
  const toggle = reader(page).locator('.remote-toggle');
  expect((await toggle.boundingBox())!.height).toBeGreaterThanOrEqual(28);

  await page.keyboard.press('Control+k');
  await page.keyboard.type('fatura');
  await page.keyboard.press('Enter');
  const chips = list(page).getByRole('group', { name: 'Arama süzgeçleri' });
  await chips.getByRole('button', { name: 'Kimden' }).click();
  await page.getByRole('menuitemcheckbox', { name: /fatura@bulutfatura\.com\.tr/ }).click();
  const x = chips.getByRole('button', { name: 'Kimden süzgecini kaldır' });
  expect(await size(x)).toEqual([28, 28]);
  // Çip 28 px kalır.
  expect((await size(chips.locator('span.chip.on')))[1]).toBe(28);

  await openSettings(page, 'Genel');
  const sw = page.getByRole('switch', { name: 'Postaları koyu temaya uyarla' });
  expect(await hitsAbove(sw, 8)).toBe(true);

  await openSettings(page, 'Hesaplar');
  await page.locator('button.acc').filter({ has: page.locator('.badge') }).first().click();
  const swatch = page.getByRole('radio', { name: 'Renk 1' });
  expect(await size(swatch)).toEqual([28, 28]);

  await openSettings(page, 'Kurallar');
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  const stop = page.locator('.stop input');
  expect(await size(stop)).toEqual([16, 16]);
});

// ------------------------------------------------------------ okuma alanı

test('okuma: araç çubuğunda listeden çıkaran eylemlerden sonra ayraç; ikon düğmeler 34×34', async ({ page }) => {
  await open(page);
  const bar = reader(page).locator('.toolbar');
  const sep = bar.locator('.tsep');
  expect(await size(sep)).toEqual([1, 18]);
  const b = (await sep.boundingBox())!;
  // Ayraç Sil ile Klasöre taşı arasında.
  const del = (await bar.getByRole('button', { name: 'Sil', exact: true }).boundingBox())!;
  const move = (await bar.getByRole('button', { name: 'Klasöre taşı' }).boundingBox())!;
  expect(b.x).toBeGreaterThan(del.x + del.width);
  expect(b.x).toBeLessThan(move.x);
  for (const btn of await bar.locator('.btn-icon').all()) expect(await size(btn)).toEqual([34, 34]);
});

test('okuma: adı olmayan gönderende adres yinelenmez; adlı gönderende adres yanında', async ({ page }) => {
  await open(page);
  await expect(reader(page).locator('.sender .addr').first()).toHaveText('kerem.aydin@ornek.com.tr');
  await goView(page, 'Arşive git');
  await list(page).getByText('Hesabınız 24 saat içinde askıya alınacak').click();
  await expect(reader(page).getByRole('heading', { level: 2, name: 'Hesabınız 24 saat içinde askıya alınacak' })).toBeVisible();
  await expect(reader(page).locator('.sender .name').first()).toHaveText('bildirim@odememerkezi-hesap.example');
  await expect(reader(page).locator('.sender .addr')).toHaveCount(0);
});

test('okuma: toplu gönderim ve uzak içerik şeritleri tek kutuda, aralarında 1 px çizgi; tek şerit eskisi gibi', async ({ page }) => {
  await open(page);
  await list(page).getByText('Sayı 142 · Sakin arayüzler üzerine').click();
  const box = reader(page).locator('.notices');
  await expect(box).toBeVisible();
  const outer = await box.evaluate((el) => getComputedStyle(el).borderTopWidth);
  expect(outer).toBe('1px');
  const kids = await box.evaluate((el) =>
    [...el.children].map((c) => {
      const cs = getComputedStyle(c);
      return [cs.borderTopWidth, cs.borderLeftWidth, cs.borderBottomWidth];
    }),
  );
  expect(kids).toEqual([
    ['0px', '0px', '0px'],
    ['1px', '0px', '0px'],
  ]);

  // Uzak içeriğe izin verilince tek şerit kalır: kendi kenarı ve köşesiyle.
  await reader(page).getByRole('button', { name: 'Bu göndericiye izin ver' }).click();
  await expect(reader(page).locator('.notices')).toHaveCount(0);
  const unsub = reader(page).locator('.unsub');
  await expect(unsub).toBeVisible();
  expect(await unsub.evaluate((el) => [getComputedStyle(el).borderTopWidth, getComputedStyle(el).borderTopLeftRadius])).toEqual(['1px', '10px']);
});

// ------------------------------------------------------------ hesap ekleme

test('hesap ekleme: indirme ilerlemesi genişlikle değil ölçekle canlanır', async ({ page }) => {
  await page.goto('/?bos#/main');
  await page.getByLabel('E-posta adresi').fill('furkan@ornek.com.tr');
  await page.getByRole('button', { name: 'Devam' }).click();
  await page.getByLabel('Parola', { exact: true }).fill('dogru-parola');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(page.getByRole('heading', { name: 'Hazırsın' })).toBeVisible();
  const fill = page.locator('.bar .fill').first();
  await expect(fill).toBeAttached();
  expect(await fill.evaluate((el) => getComputedStyle(el).transitionProperty)).toBe('transform');
});

// ------------------------------------------------------------ yazma ve hızlı okuma

test('yazma: alt çubukta Ctrl Enter yalnız Gönder düğmesinde; alıcı eklenince alıcı uyarısı kalkar', async ({ page }) => {
  await page.goto('/#/compose/new/0');
  const foot = page.locator('footer');
  await expect(foot.getByRole('button', { name: /^Gönder/ })).toBeVisible();
  const n = await foot.evaluate((el) => (el.textContent ?? '').split('Ctrl Enter').length - 1);
  expect(n).toBe(1);
  await expect(foot.locator('kbd, .kbd').filter({ hasText: 'Ctrl Enter' })).toHaveCount(1);

  await foot.getByRole('button', { name: /^Gönder/ }).click();
  await expect(page.getByRole('alert')).toHaveText('En az bir alıcı ekle.');
  const to = page.locator('#compose-to');
  await to.fill('kerem.aydin@ornek.com.tr');
  await to.press('Enter');
  await expect(page.getByRole('alert')).toHaveCount(0);
  // Masaüstünde Bilgi ve Gizli ayrı metin düğmeleri kalır.
  await expect(page.getByRole('button', { name: 'Bilgi', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Gizli', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Bilgi ve Gizli alanlarını ekle' })).toHaveCount(0);
});

test('hızlı okuma 440 px: başlıkta tek boşluk, hazır yanıt düzenle düğmesi tamamen görünür', async ({ page }) => {
  await page.setViewportSize({ width: 440, height: 700 });
  await page.goto('/#/quick/4');
  const title = page.locator('.titlebar .title');
  await expect(title).toHaveText(/^Hızlı okuma · \S/);
  expect(await title.evaluate((el) => (el as HTMLElement).innerText)).not.toMatch(/\s{2}/);
  const edit = page.getByRole('button', { name: 'Hızlı yanıtları düzenle' });
  await expect(edit).toBeVisible();
  const b = (await edit.boundingBox())!;
  expect(b.x + b.width).toBeLessThanOrEqual(440);
});
