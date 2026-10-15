import { expect, test, type Page } from '@playwright/test';

// Masaüstü deneyimi: dar pencere (ray, katman, yığın), sütun ayırıcıları,
// odak kipi, kısayol öğretimi, iki tuşlu gezinme, seçiciler, arama çipleri,
// işleç tamamlama ve arayüz ölçeği.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const side = (page: Page) => page.getByRole('navigation', { name: 'Kutular ve hesaplar' });
const rail = (page: Page) => page.getByRole('navigation', { name: 'Görünümler' });
const palette = (page: Page) => page.getByRole('dialog', { name: 'Komut paleti' });

async function open(page: Page) {
  await page.goto('/#/main');
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await open(page);
});

// ------------------------------------------------------------ dar pencere

test('1000 px: kenar çubuğu 56 px raya iner, Ctrl+B tam kenar çubuğunu katman olarak açar', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await expect(side(page)).toHaveCount(0);
  await expect(rail(page)).toBeVisible();
  expect((await rail(page).boundingBox())!.width).toBe(56);
  // Görünümler sayaçlı; ad ve sayı ipucunda.
  const inbox = rail(page).getByRole('button', { name: 'Gelen Kutusu · 3' });
  await expect(inbox).toHaveAttribute('title', 'Gelen Kutusu · 3');
  await expect(inbox).toHaveAttribute('aria-current', 'page');
  await expect(inbox.locator('.count')).toHaveText('3');
  // Okuma alanı yerinde kalır.
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();

  await rail(page).getByRole('button', { name: 'Yıldızlılar' }).click();
  await expect(list(page).getByRole('heading', { name: 'Yıldızlılar' })).toBeVisible();

  await page.keyboard.press('Control+b');
  await expect(side(page)).toBeVisible();
  await expect(side(page).getByRole('button', { name: 'Tüm hesaplar 3 hesap' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(side(page)).toHaveCount(0);

  // Katmandan görünüm seçince katman kapanır.
  await rail(page).getByRole('button', { name: 'Kenar çubuğunu aç' }).click();
  await side(page).getByRole('button', { name: /Gelen Kutusu/ }).click();
  await expect(side(page)).toHaveCount(0);
  await expect(list(page).getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
});

test('dar okuma bölmesi: postanın üstü sıkı, hızlı yanıt odaklanınca açılır', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 760 });
  await page.keyboard.press('Control+k');
  await page.keyboard.type('Arşive git');
  await palette(page).getByRole('option', { name: /^Arşive git/ }).first().click();
  await list(page).getByText('Haftalık özet: üç görev tamamlandı').click();
  const reader = page.getByRole('main');
  const subject = reader.getByRole('heading', { level: 2, name: 'Haftalık özet: üç görev tamamlandı' });
  await expect(subject).toBeVisible();

  // Konu 19 px; tek iletide "1 ileti" satırı ve araç çubuğundakini yineleyen
  // yanıtla / ilet düğmeleri yok; tarih alıcı satırında, ad kesilmeden.
  await expect(subject).toHaveCSS('font-size', '19px');
  await expect(reader.getByText('1 ileti', { exact: true })).toHaveCount(0);
  const sender = reader.locator('.sender');
  for (const name of ['Yanıtla', 'İlet']) await expect(sender.getByRole('button', { name, exact: true })).toHaveCount(0);
  await expect(sender.getByRole('button', { name: 'Yıldızla' })).toBeVisible();
  await expect(sender.locator('.who > .meta')).toHaveText(/^\d{1,2} \S+ \d{4} \d{2}:\d{2} · kime: sen$/);
  const name = sender.locator('.name');
  expect(await name.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);

  // Hızlı yanıt boşken tek satır: hazır yanıtlar ve gönder çubuğu yok.
  const send = reader.getByRole('button', { name: 'Gönder', exact: true });
  const box = page.getByRole('textbox', { name: 'Hızlı yanıt yaz…' });
  const chips = page.getByRole('group', { name: 'Hızlı yanıtlar' });
  await expect(chips).toHaveCount(0);
  await expect(send).toHaveCount(0);
  expect((await box.boundingBox())!.height).toBeLessThanOrEqual(44);

  // Posta çerçevesi üstten 270 px'ten önce başlar; altında en az 400 px gövde görünür.
  const frame = (await page.locator('iframe[title="Posta içeriği"]').boundingBox())!;
  expect(frame.y).toBeLessThan(270);
  expect((await box.boundingBox())!.y - frame.y).toBeGreaterThan(400);

  // Odakla açılır; çip kutuyu doldurur, odak dışarı çıkınca metin varken açık kalır.
  await box.click();
  await expect(chips).toBeVisible();
  await expect(send).toBeDisabled();
  await chips.getByRole('button', { name: 'Tamam, uygun.' }).click();
  await expect(box).toHaveValue('Tamam, uygun.');
  await subject.click();
  await expect(send).toBeEnabled();
  await box.fill('');
  await subject.click();
  await expect(send).toHaveCount(0);
});

test('geniş pencerede Ctrl+B kenar çubuğunu raya indirir; tercih kalıcıdır', async ({ page }) => {
  await expect(side(page)).toBeVisible();
  await page.keyboard.press('Control+b');
  await expect(side(page)).toHaveCount(0);
  await expect(rail(page)).toBeVisible();
  await page.reload();
  await expect(rail(page)).toBeVisible();
  await rail(page).getByRole('button', { name: 'Kenar çubuğunu aç' }).click();
  await expect(side(page)).toBeVisible();
  await expect(rail(page)).toHaveCount(0);
  // Kenar çubuğundaki düğme de daraltır.
  await side(page).getByRole('button', { name: 'Kenar çubuğunu daralt' }).click();
  await expect(rail(page)).toBeVisible();
});

test('800 px: liste → okuma yığını; posta açılınca okuma tam genişlik, Esc ve geri düğmesi listeye döner', async ({ page }) => {
  await page.setViewportSize({ width: 800, height: 700 });
  await expect(list(page)).toBeVisible();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  const listBox = (await list(page).boundingBox())!;
  expect(listBox.width).toBeGreaterThan(700);

  await list(page).getByText('Cumartesi kahvaltı?').click();
  const heading = page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' });
  await expect(heading).toBeVisible();
  await expect(list(page)).toHaveCount(0);
  const reader = page.locator('main.reader');
  expect((await reader.boundingBox())!.width).toBeGreaterThan(700);

  // J/K okuma alanında gezinir.
  await page.keyboard.press('j');
  await expect(page.getByRole('heading', { level: 2, name: '[mailhere] Derleme başarılı · main #128' })).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(list(page)).toBeVisible();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);

  // Enter seçili postayı açar; geri düğmesi listeye döner.
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { level: 2, name: '[mailhere] Derleme başarılı · main #128' })).toBeVisible();
  await page.getByRole('button', { name: 'Listeye dön' }).click();
  await expect(list(page)).toBeVisible();

  // Başka görünüme geçince yığın listede başlar.
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await rail(page).getByRole('button', { name: 'Arşiv' }).click();
  await expect(list(page).getByRole('heading', { name: 'Arşiv' })).toBeVisible();

  // Okunan son posta arşivlenince liste (boş durum) görünür.
  await rail(page).getByRole('button', { name: 'Yıldızlılar' }).click();
  await list(page).getByText('Sözleşme imzaya hazır').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Sözleşme imzaya hazır' })).toBeVisible();
  await page.keyboard.press('e');
  await expect(list(page)).toBeVisible();
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toHaveCount(0);
});

// --------------------------------------------------------------- ayırıcılar

test('sütun ayırıcıları: sürükleme, ←/→ 8 px, çift tık varsayılan; genişlik kalıcı', async ({ page }) => {
  const sep = page.getByRole('separator', { name: 'Posta listesi genişliği' });
  await expect(sep).toHaveAttribute('aria-valuenow', '404');
  const b = (await sep.boundingBox())!;
  await page.mouse.move(b.x + b.width / 2, b.y + 300);
  await page.mouse.down();
  await page.mouse.move(b.x + b.width / 2 + 30, b.y + 300);
  await page.mouse.move(b.x + b.width / 2 + 60, b.y + 300);
  await page.mouse.up();
  await expect(sep).toHaveAttribute('aria-valuenow', '464');
  expect(Math.round((await list(page).boundingBox())!.width)).toBe(464);

  await sep.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(sep).toHaveAttribute('aria-valuenow', '456');
  await page.reload();
  await expect(sep).toHaveAttribute('aria-valuenow', '456');

  await sep.dblclick();
  await expect(sep).toHaveAttribute('aria-valuenow', '404');

  // Kenar çubuğu: sınırlar içinde.
  const sideSep = page.getByRole('separator', { name: 'Kenar çubuğu genişliği' });
  await expect(sideSep).toHaveAttribute('aria-valuenow', '248');
  await sideSep.focus();
  await page.keyboard.press('ArrowRight');
  await expect(sideSep).toHaveAttribute('aria-valuenow', '256');
  expect(Math.round((await side(page).boundingBox())!.width)).toBe(256);
  await page.keyboard.press('End');
  await expect(sideSep).toHaveAttribute('aria-valuenow', '360');
  await expect(sideSep).toHaveAttribute('aria-valuemax', '360');
  await page.keyboard.press('Home');
  await expect(sideSep).toHaveAttribute('aria-valuenow', '200');
  // Ayırıcı odaktayken ok tuşları listede gezinmez.
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
});

// --------------------------------------------------------------- odak kipi

test('odak kipi: Shift+O, 3 posta arşivlenir, ilerleme ilerler, liste bitince boş durum, geri al postayı döndürür', async ({ page }) => {
  // Küçük bir liste: gelen kutusunun bildirimleri.
  await list(page).getByRole('button', { name: 'Bildirimler' }).click();
  const rows = await list(page).locator('.row').count();
  expect(rows).toBeGreaterThanOrEqual(3);

  await page.keyboard.press('Shift+O');
  const focus = page.getByRole('region', { name: 'Odak kipi' });
  await expect(focus).toBeVisible();
  await expect(list(page)).toHaveCount(0);
  await expect(side(page)).toHaveCount(0);
  const progress = focus.getByRole('status');
  await expect(progress).toHaveText(`1/${rows}`);
  await expect(progress).toHaveAttribute('aria-label', `1. posta, toplam ${rows}`);
  // Sağda kısayol çipli eylemler.
  const acts = focus.getByRole('complementary', { name: 'Bu postaya ne olsun?' });
  await expect(acts.getByRole('button', { name: 'Arşivle E' })).toBeVisible();
  await expect(acts.getByRole('button', { name: 'Atla J' })).toBeVisible();

  const subjects: string[] = [];
  for (let i = 0; i < 3; i++) {
    const h = focus.getByRole('heading', { level: 2 }).first();
    await expect(h).toBeVisible();
    // Sıradaki posta gelene kadar beklenir (opaklık geçişi + yükleme).
    if (i) await expect(h).not.toHaveText(subjects[i - 1]);
    subjects.push((await h.textContent())!.trim());
    await page.keyboard.press('e');
    await expect(progress).toHaveText(`${i + 2 > rows ? rows : i + 2}/${rows}`);
    await expect(page.getByRole('status').filter({ hasText: 'Arşivlendi' })).toBeVisible();
  }
  expect(new Set(subjects).size).toBe(3);

  // Kalanlar atlanır; liste bitince boş durum.
  for (let i = 3; i < rows; i++) await page.keyboard.press('j');
  await expect(focus.getByRole('heading', { name: 'Gelen kutusu bitti' })).toBeVisible();
  await expect(progress).toHaveText(`${rows}/${rows}`);

  // Geri al: son arşivlenen posta geri gelir ve gösterilir.
  await page.keyboard.press('Control+z');
  await expect(focus.getByRole('heading', { level: 2, name: subjects[2] })).toBeVisible();
  await expect(focus.getByRole('heading', { name: 'Gelen kutusu bitti' })).toHaveCount(0);

  await page.keyboard.press('Escape');
  await expect(focus).toHaveCount(0);
  await expect(list(page)).toBeVisible();
  await expect(list(page).getByText(subjects[2])).toBeVisible();
  await expect(list(page).getByText(subjects[0])).toHaveCount(0);
});

test('odak kipi paletten açılır; fareyle eylem sıradakine geçer, Odaktan çık düğmesi kapatır', async ({ page }) => {
  await page.keyboard.press('Control+k');
  await page.keyboard.type('odak');
  const opt = palette(page).getByRole('option', { name: /Odak kipi/ });
  await expect(opt).toContainText('Shift');
  await opt.click();
  const focus = page.getByRole('region', { name: 'Odak kipi' });
  await expect(focus).toBeVisible();
  const first = (await focus.getByRole('heading', { level: 2 }).first().textContent())!.trim();
  await focus.getByRole('button', { name: 'Atla J' }).click();
  await expect(focus.getByRole('heading', { level: 2 }).first()).not.toHaveText(first);
  await focus.getByRole('button', { name: 'Önceki posta K' }).click();
  await expect(focus.getByRole('heading', { level: 2 }).first()).toHaveText(first);
  await focus.getByRole('button', { name: 'Odaktan çık' }).click();
  await expect(focus).toHaveCount(0);
});

// ------------------------------------------------------------ kısayol öğretimi

test('kısayol ipucu: fareyle arşivleyince toast kısayolu söyler, en çok 3 kez; ayardan kapatılır', async ({ page }) => {
  const archive = page.locator('main.reader .toolbar').getByRole('button', { name: 'Arşivle' });
  for (let i = 0; i < 3; i++) {
    await archive.click();
    const t = page.getByRole('status').filter({ hasText: 'Arşivlendi' });
    await expect(t).toContainText('Kısayol');
    await expect(t.locator('kbd')).toHaveText('E');
    await t.getByRole('button', { name: 'Geri al' }).click();
    await expect(t).toHaveCount(0);
  }
  await archive.click();
  const fourth = page.getByRole('status').filter({ hasText: 'Arşivlendi' });
  await expect(fourth).toBeVisible();
  await expect(fourth).not.toContainText('Kısayol');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('mh.teach') ?? '{}').n.archive)).toBe(3);

  // Klavyeyle basılan düğme öğretmez; yalnız toast'sız eylemlerde kısa ipucu çıkar.
  await page.locator('main.reader .toolbar').getByRole('button', { name: 'Okunmadı say' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Okunmadı say' })).toContainText('U');

  // Ayarlar › Genel: kapatılınca ipucu çıkmaz.
  await page.keyboard.press('Control+,');
  await page.getByRole('switch', { name: 'Kısayol ipuçları' }).uncheck();
  await page.keyboard.press('Escape');
  await page.locator('main.reader .toolbar').getByRole('button', { name: 'Sil' }).click();
  const del = page.getByRole('status').filter({ hasText: 'Çöp kutusuna taşındı' });
  await expect(del).toBeVisible();
  await expect(del).not.toContainText('Kısayol');
});

test('palet: her komutta kısayol, TR/EN eşanlamlı bulanık eşleşme, sık kullanılan üste', async ({ page }) => {
  await page.keyboard.press('Control+k');
  // İngilizce eşanlamlı Türkçe arayüzde de bulur.
  await page.keyboard.type('trash');
  await expect(palette(page).getByRole('option', { name: /^Sil/ })).toContainText('#');
  await expect(palette(page).getByRole('option', { name: /Çöpe git/ })).toContainText('G#');
  // Aksansız ve harf atlamalı yazım.
  await palette(page).getByRole('combobox').fill('kisayol');
  await expect(palette(page).getByRole('option', { name: /Klavye kısayolları/ })).toContainText('?');
  await palette(page).getByRole('combobox').fill('gnderilen');
  await expect(palette(page).getByRole('option', { name: /Gönderilenlere git/ })).toBeVisible();

  // Sık kullanılan boş palette en üstte.
  // Arama her zaman ilk sıradadır (Enter arar); komut bir alt satırda.
  await palette(page).getByRole('combobox').fill('yıldızlılara');
  await expect(palette(page).getByRole('option').nth(1)).toContainText('Yıldızlılara git');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(list(page).getByRole('heading', { name: 'Yıldızlılar', exact: true })).toBeVisible();
  await page.keyboard.press('Control+k');
  await expect(palette(page).getByRole('option').first()).toContainText('Yıldızlılara git');
});

// ------------------------------------------------------ iki tuşlu gezinme

test('G zinciri: ipucu sağ altta, G S / G T / G I görünüm değiştirir; tanınmayan ikinci tuş zinciri bitirir', async ({ page }) => {
  await page.keyboard.press('g');
  const hint = page.getByRole('status', { name: 'G sonra' });
  await expect(hint).toBeVisible();
  const box = (await hint.boundingBox())!;
  const vp = page.viewportSize()!;
  expect(box.x + box.width).toBeGreaterThan(vp.width - 40);
  expect(box.y + box.height).toBeGreaterThan(vp.height - 40);
  await page.keyboard.press('s');
  await expect(hint).toHaveCount(0);
  await expect(list(page).getByRole('heading', { name: 'Yıldızlılar' })).toBeVisible();

  await page.keyboard.press('g');
  await page.keyboard.press('t');
  await expect(list(page).getByRole('heading', { name: 'Gönderilenler' })).toBeVisible();

  // Tanınmayan tuş: yer değişmez, tek harf eylemi de çalışmaz.
  await page.keyboard.press('g');
  await page.keyboard.press('e');
  await expect(hint).toHaveCount(0);
  await expect(list(page).getByRole('heading', { name: 'Gönderilenler' })).toBeVisible();
  await expect(page.getByRole('status').filter({ hasText: 'Arşivlendi' })).toHaveCount(0);

  await page.keyboard.press('g');
  await page.keyboard.press('i');
  await expect(list(page).getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();

  // Kısayol sayfası (?) zinciri ve seçicileri listeler; Hakkında'da da aynı liste.
  await page.keyboard.press('Shift+?');
  const keys = page.getByRole('dialog', { name: 'Klavye kısayolları' });
  await expect(keys).toBeVisible();
  await expect(keys.getByText('Gönderilenlere git')).toBeVisible();
  await expect(keys.getByText('Klasöre taşı')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(keys).toHaveCount(0);
  await page.keyboard.press('Control+,');
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Güncellemeler' }).click();
  await expect(page.getByText('Ertelenenlere git')).toBeVisible();
});

// ------------------------------------------------------------- seçiciler

test('V klasör seçicisi yazarak süzülür, Enter taşır; L etiket seçicisi etiketi takar', async ({ page }) => {
  await page.keyboard.press('v');
  const move = page.getByRole('dialog', { name: 'Klasöre taşı' });
  await expect(move).toBeVisible();
  await expect(move.getByRole('combobox')).toBeFocused();
  await page.keyboard.type('hkk');
  await expect(move.getByRole('option')).toHaveCount(1);
  await expect(move.getByRole('option', { name: 'Hukuk' })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(move).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('Hukuk klasörüne taşındı');
  await expect(list(page).getByText('Re: Q4 bütçe taslağı')).toHaveCount(0);
  await page.keyboard.press('Control+z');
  await expect(list(page).getByText('Re: Q4 bütçe taslağı')).toHaveCount(1);

  await list(page).getByText('Cumartesi kahvaltı?').click();
  await page.keyboard.press('l');
  const tags = page.getByRole('dialog', { name: 'Etiketle' });
  await expect(tags).toBeVisible();
  await page.keyboard.type('fat');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('status')).toContainText('Fatura etiketi eklendi');
  await expect(page.locator('main.reader')).toContainText('Fatura');
  // Esc kapatır, posta yerinde kalır.
  await page.keyboard.press('v');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'Klasöre taşı' })).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
});

// ----------------------------------------------------------- arama çipleri

test('arama çipleri sorgu metnine işleç yazar; metin doğruluk kaynağıdır', async ({ page }) => {
  await page.keyboard.press('Control+k');
  await page.keyboard.type('fatura');
  await page.keyboard.press('Enter');
  const h = list(page).getByRole('heading', { level: 1 });
  await expect(h).toHaveText('“fatura” için sonuçlar');
  const chips = list(page).getByRole('group', { name: 'Arama süzgeçleri' });

  await chips.getByRole('button', { name: 'Ekli' }).click();
  await expect(h).toHaveText('“fatura has:ek” için sonuçlar');
  await expect(chips.getByRole('button', { name: 'Ekli' })).toHaveAttribute('aria-pressed', 'true');
  await expect(list(page).getByText('Ekim faturanız hazır')).toBeVisible();

  await chips.getByRole('button', { name: 'Okunmamış' }).click();
  await expect(h).toHaveText('“fatura has:ek is:okunmamış” için sonuçlar');
  await chips.getByRole('button', { name: 'Okunmamış' }).click();
  await chips.getByRole('button', { name: 'Ekli' }).click();
  await expect(h).toHaveText('“fatura” için sonuçlar');

  // Tarih: hazır tarih after: olarak yazılır; Eylül faturası düşer.
  const monthStart = await page.evaluate(() => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-01`;
  });
  await chips.getByRole('button', { name: 'Tarih' }).click();
  await page.getByRole('menuitemcheckbox', { name: /Bu ay/ }).click();
  await expect(h).toHaveText(`“fatura after:${monthStart}” için sonuçlar`);
  await expect(chips.getByRole('button', { name: 'Tarih: Bu ay' })).toBeVisible();
  await expect(list(page).getByText('Eylül faturanız hazır')).toHaveCount(0);
  await chips.getByRole('button', { name: 'Tarih süzgecini kaldır' }).click();
  await expect(h).toHaveText('“fatura” için sonuçlar');

  // Kimden: sonuçlardaki göndericiler.
  await chips.getByRole('button', { name: 'Kimden' }).click();
  await page.getByRole('menuitemcheckbox', { name: /fatura@bulutfatura\.com\.tr/ }).click();
  await expect(h).toHaveText('“fatura from:fatura@bulutfatura.com.tr” için sonuçlar');
  await expect(chips.getByRole('button', { name: 'Kimden: Bulut Fatura' })).toBeVisible();

  // Hesap: arayüzde çözülür; başka hesapta sonuç kalmaz.
  await chips.getByRole('button', { name: 'Hesap' }).click();
  await page.getByRole('menuitemcheckbox', { name: /^İş/ }).click();
  await expect(h).toContainText('hesap:');
  await expect(list(page).getByText('Ekim faturanız hazır')).toHaveCount(0);
  await chips.getByRole('button', { name: 'Hesap süzgecini kaldır' }).click();
  await expect(list(page).getByText('Ekim faturanız hazır')).toBeVisible();

  // Başlık sorguyu palette düzenlemeye açar.
  await h.getByRole('button').click();
  await expect(palette(page).getByRole('combobox')).toHaveValue('fatura from:fatura@bulutfatura.com.tr');
});

test('işleç tamamlama: from: kişi, after: hazır tarih önerir; Tab ilk öneriyi yazar', async ({ page }) => {
  await page.keyboard.press('Control+k');
  const input = palette(page).getByRole('combobox');
  await page.keyboard.type('from:');
  await expect(palette(page).getByRole('option', { name: /Kerem Aydın/ })).toBeVisible();
  await page.keyboard.type('ayse');
  // Değer yazılınca Enter arar (ilk satır), öneriler hemen altında.
  await expect(palette(page).getByRole('option').first()).toContainText('Tüm postalarda ara');
  await expect(palette(page).getByRole('option', { name: 'Ayşe Demir ayse.demir@eposta.com' })).toBeVisible();
  await page.keyboard.press('Tab');
  await expect(input).toHaveValue('from:ayse.demir@eposta.com ');

  await page.keyboard.type('after:');
  const today = await page.evaluate(() => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  });
  await expect(palette(page).getByRole('option', { name: /Bu hafta/ })).toBeVisible();
  await palette(page).getByRole('option', { name: /Bugün/ }).click();
  await expect(input).toHaveValue(`from:ayse.demir@eposta.com after:${today} `);
  await page.keyboard.press('Enter');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await expect(list(page).getByText('Re: Kitap kulübü')).toHaveCount(0);
});

// ------------------------------------------------------------------ ölçek

test('arayüz ölçeği: Ctrl+= büyütür, Ctrl+- küçültür, Ctrl+0 sıfırlar; kalıcı ve Ayarlar › Genel’de seçilir', async ({ page }) => {
  const zoom = () => page.evaluate(() => document.documentElement.style.zoom);
  await page.keyboard.press('Control+Equal');
  await expect(page.getByRole('status').filter({ hasText: 'Arayüz ölçeği %110' })).toBeVisible();
  expect(await zoom()).toBe('1.1');
  // Pencere ölçekten sonra da taşmaz.
  const shell = await page.locator('.shell').boundingBox();
  expect(Math.round(shell!.height)).toBe(900);

  await page.keyboard.press('Control+Equal');
  expect(await zoom()).toBe('1.2');
  await page.keyboard.press('Control+Minus');
  expect(await zoom()).toBe('1.1');
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
  expect(await zoom()).toBe('1.1');

  await page.keyboard.press('Control+,');
  const group = page.getByRole('radiogroup', { name: 'Arayüz ölçeği' });
  await expect(group.getByRole('radio', { name: '%110' })).toHaveAttribute('aria-checked', 'true');
  await group.getByRole('radio', { name: '%150' }).click();
  expect(await zoom()).toBe('1.5');

  await page.keyboard.press('Control+0');
  expect(await zoom()).toBe('');
  await expect(group.getByRole('radio', { name: '%100' })).toHaveAttribute('aria-checked', 'true');
  expect(await page.evaluate(() => localStorage.getItem('mh.desk.scale'))).toBeNull();
});
