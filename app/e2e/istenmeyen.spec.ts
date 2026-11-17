import { expect, test } from '@playwright/test';
import { nav, list, toast, row, notice, SUSPECT, CUSTOMS, token, openJunk, openArchive, state } from './koruma';

// 0.11 (masaüstü): şüpheli posta uyarı kutusu, "İstenmeyen değil", göndereni
// engelleme, satırdaki uyarı işareti ve menüler. Örnek postalar demo-data.ts'tedir.

test('şüpheli posta: uyarı kutusu nedenleri sırayla yazar; İstenmeyen kutusunda "İstenmeyen değil" gelen kutusuna döndürür', async ({ page }) => {
  await openJunk(page);
  await row(page, SUSPECT).click();
  const box = notice(page);
  await expect(box).toBeVisible();
  await expect(box.getByRole('listitem')).toHaveText([
    'Posta sunucun bu postayı istenmeyen olarak işaretledi.',
    'Gönderenin alan adı tanıdık bir adrese benzetilmiş.',
    'Yanıtın gönderene değil, başka bir adrese gidecek.',
  ]);
  // Uyarı kutusu tek satırlık gönderen uyarısının yerine geçer.
  await expect(page.locator('.trust')).toHaveCount(0);
  // Posta zaten İstenmeyen'de: taşıma sunulmaz.
  await expect(box.getByRole('button')).toHaveText(['Göndereni engelle', 'Güvenli']);
  // Başlık --amber-text, ikon --amber; renkli sol kenar yok (dört kenar aynı).
  expect(await box.getByRole('heading').evaluate((el) => getComputedStyle(el).color)).toBe(await token(page, '--amber-text'));
  expect(await box.locator('.mark').evaluate((el) => getComputedStyle(el).color)).toBe(await token(page, '--amber'));
  const borders = await box.evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.borderLeftColor, s.borderLeftWidth, s.borderTopColor, s.borderTopWidth];
  });
  expect(borders[0]).toBe(borders[2]);
  expect(borders[1]).toBe(borders[3]);

  // Çubuktaki "İstenmeyen değil": gönderen güvenli sayılır, posta gelen kutusuna döner.
  await page.getByRole('main').getByRole('button', { name: 'İstenmeyen değil' }).click();
  await expect(toast(page)).toContainText('Gönderen güvenli sayıldı; posta gelen kutusuna taşındı');
  await expect(row(page, SUSPECT)).toHaveCount(0);
  expect(await state(page, SUSPECT)).toMatchObject({ role: 'inbox', spam: false });
});

test('şüpheli posta (arşiv): üç eylem; Güvenli uyarıyı ve satır işaretini kaldırır', async ({ page }) => {
  await openArchive(page);
  // Satırda uyarı işareti: amber, erişilebilir adlı.
  const mark = row(page, CUSTOMS).getByRole('img', { name: 'Şüpheli posta' });
  await expect(mark).toBeVisible();
  expect(await mark.evaluate((el) => getComputedStyle(el).color)).toBe(await token(page, '--amber'));
  await row(page, CUSTOMS).click();
  const box = notice(page);
  await expect(box.getByRole('listitem')).toHaveText(['Posta, yazdığı adresten geldiğini kanıtlayamadı.']);
  await expect(box.getByRole('button')).toHaveText(["İstenmeyen'e taşı", 'Göndereni engelle', 'Güvenli']);

  await box.getByRole('button', { name: 'Güvenli' }).click();
  await expect(toast(page)).toContainText('Gönderen güvenli sayıldı; uyarı kaldırıldı');
  await expect(notice(page)).toHaveCount(0);
  await expect(row(page, CUSTOMS).getByRole('img', { name: 'Şüpheli posta' })).toHaveCount(0);
  expect(await state(page, CUSTOMS)).toMatchObject({ role: 'archive', spam: false });
});

test("şüpheli posta: İstenmeyen'e taşı geri alınır; Göndereni engelle uyarıyı günceller ve geri alınır", async ({ page }) => {
  await openArchive(page);
  await row(page, CUSTOMS).click();
  await notice(page).getByRole('button', { name: "İstenmeyen'e taşı" }).click();
  await expect(toast(page)).toContainText('İstenmeyen klasörüne taşındı');
  await expect(row(page, CUSTOMS)).toHaveCount(0);
  expect(await state(page, CUSTOMS)).toMatchObject({ role: 'junk' });
  await toast(page).getByRole('button', { name: 'Geri al' }).click();
  await expect(row(page, CUSTOMS)).toBeVisible();
  expect(await state(page, CUSTOMS)).toMatchObject({ role: 'archive' });

  await row(page, CUSTOMS).click();
  await notice(page).getByRole('button', { name: 'Göndereni engelle' }).click();
  await expect(toast(page)).toContainText('bilgi@derinkargo.com.tr engellendi');
  // Posta arşivde kaldı: kutu artık engeli söyler, "Göndereni engelle" kalkar.
  await expect(notice(page).getByRole('listitem').first()).toHaveText('Bu göndericiyi sen engelledin.');
  await expect(notice(page).getByRole('button', { name: 'Göndereni engelle' })).toHaveCount(0);
  expect(await page.evaluate(() => window.__mh.state.settings.blocked)).toContain('bilgi@derinkargo.com.tr');
  await toast(page).getByRole('button', { name: 'Geri al' }).click();
  await expect.poll(() => page.evaluate(() => window.__mh.state.settings.blocked)).not.toContain('bilgi@derinkargo.com.tr');
  await expect(notice(page).getByRole('button', { name: 'Göndereni engelle' })).toBeVisible();
});

test('menüler: satır bağlam menüsünde Göndereni engelle; İstenmeyen kutusunda İstenmeyen değil, şüphelide Güvenli say', async ({ page }) => {
  await openArchive(page);
  await row(page, CUSTOMS).click({ button: 'right' });
  const menu = page.getByRole('menu', { name: CUSTOMS });
  await expect(menu.getByRole('menuitem', { name: /^Güvenli say/ })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: /^İstenmeyen değil/ })).toHaveCount(0);
  await menu.getByRole('menuitem', { name: /^Göndereni engelle/ }).click();
  await expect(toast(page)).toContainText('bilgi@derinkargo.com.tr engellendi');
  expect(await page.evaluate(() => window.__mh.state.settings.blocked)).toContain('bilgi@derinkargo.com.tr');

  // İstenmeyen kutusu: bağlam menüsü ve Diğer eylemler "İstenmeyen değil" der; "tümünü çöpe at" yoktur.
  await nav(page).getByRole('button', { name: 'Kişisel klasörlerini göster' }).click();
  await nav(page).getByRole('button', { name: /^İstenmeyen/ }).first().click();
  await row(page, SUSPECT).click();
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  const more = page.getByRole('menu');
  await expect(more.getByRole('menuitem', { name: 'İstenmeyen değil' })).toBeVisible();
  await expect(more.getByRole('menuitem', { name: /adresini engelle/ })).toBeVisible();
  await expect(more.getByRole('menuitem', { name: /tüm postalarını çöpe/ })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await row(page, SUSPECT).click({ button: 'right' });
  await page.getByRole('menu', { name: SUSPECT }).getByRole('menuitem', { name: /^İstenmeyen değil/ }).click();
  await expect(toast(page)).toContainText('posta gelen kutusuna taşındı');
  expect(await state(page, SUSPECT)).toMatchObject({ role: 'inbox' });
});

test('İstenmeyen kutusunda çubuk taşmaz; dar okuma alanında yanıt düğmeleri ikona iner', async ({ page }) => {
  await openJunk(page);
  await row(page, SUSPECT).click();
  const bar = page.locator('.reader .toolbar');
  const fits = () => bar.evaluate((el) => el.scrollWidth <= el.clientWidth);
  await expect(bar.getByRole('button', { name: 'İstenmeyen değil' })).toContainText('İstenmeyen değil');
  expect(await fits()).toBe(true);
  // 1440 px pencerede okuma alanı 860 px'ten dar: yanıt düğmeleri ikon, asıl eylem etiketli.
  await expect(bar.getByRole('button', { name: 'Yanıtla', exact: true }).locator('.lbl')).toBeHidden();
  for (const width of [1180, 1000, 800]) {
    // 800 px yığın kipidir: açık posta okuma alanında tam genişlikte kalır.
    await page.setViewportSize({ width, height: 800 });
    await expect(bar.getByRole('button', { name: 'Diğer eylemler' })).toBeVisible();
    expect(await fits(), `${width} px`).toBe(true);
  }
});

test('hızlı okuma penceresi: şüpheli postada nedenler yazar, eylem düğmesi yoktur', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  const id = await page.evaluate((s) => window.__mh.state.messages.find((m) => m.subject === s)!.id, CUSTOMS);
  await page.goto(`/#/quick/${id}`);
  await page.reload();
  await expect(page.getByRole('heading', { level: 1, name: CUSTOMS })).toBeVisible();
  await expect(notice(page).getByRole('listitem')).toHaveText(['Posta, yazdığı adresten geldiğini kanıtlayamadı.']);
  await expect(notice(page).getByRole('button')).toHaveCount(0);
  // 440 px pencerede kutu taşmaz.
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
});
