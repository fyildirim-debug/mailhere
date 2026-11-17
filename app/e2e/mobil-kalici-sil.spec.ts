import { expect, test, type Locator, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil 0.11 · kalıcı silme ve kutuyu boşaltma: Çöp ve İstenmeyen
// görünümlerinde "Sil" yerine onaylı "Kalıcı sil" (kaydırma, seçim çubuğu,
// okuma şeridi; geri alma yok), uygulama çubuğu menüsünde "Çöpü boşalt" /
// "İstenmeyen'i boşalt", İstenmeyen kutusunda "İstenmeyen değil" kaydırması.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const strip = (page: Page) => page.getByRole('toolbar', { name: 'Posta eylemleri' });
const toast = (page: Page, text: string | RegExp) => page.getByRole('status').filter({ hasText: text });
const confirm = (page: Page, name: string) => page.getByRole('dialog', { name });
const undo = (page: Page) => page.getByRole('button', { name: 'Geri al' });

/** Çekmece kapandı ve örtüsü söndü (örtü 200 ms daha dokunuşları tutar). */
async function drawerGone(page: Page) {
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => page.locator('.scrim').evaluateAll((els) => els.every((el) => getComputedStyle(el).visibility === 'hidden'))).toBe(true);
}

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

async function openView(page: Page, name: RegExp, title: string) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name }).click();
  await expect(h1(page, title)).toBeVisible();
  await drawerGone(page);
}

async function openJunk(page: Page) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Kişisel klasörlerini göster' }).click();
  await drawer(page).getByRole('group', { name: 'Kişisel klasörleri' }).getByRole('button', { name: /İstenmeyen/ }).click();
  await expect(h1(page, 'İstenmeyen · Kişisel')).toBeVisible();
  await drawerGone(page);
}

/** Uygulama çubuğundaki "Diğer eylemler" menüsü (liste ekranı). */
async function openListMenu(page: Page) {
  await page.getByRole('banner').getByRole('button', { name: 'Diğer eylemler' }).click();
  await expect(page.getByRole('menu')).toBeVisible();
}

/** Verilen konulu postaları Çöp'e taşır (tanıtım verisinde Çöp boştur). */
async function fillTrash(page: Page, subjects: string[]) {
  await page.evaluate(async (list) => {
    const ids = window.__mh.state.messages.filter((m) => list.includes(m.subject)).map((m) => m.id);
    await window.__mh.call('messages_move', { ids, target: 'trash' });
  }, subjects);
}

async function setSwipe(page: Page, patch: { swipeLeft?: string; swipeRight?: string }) {
  await page.evaluate((p) => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, ...p } as never }), patch);
}

/** Satırı yatay sürükler; bırakmadan önce alttaki panelin adını döner. */
async function swipe(page: Page, text: string, dx: number): Promise<string> {
  const box = await row(page, text).boundingBox();
  if (!box) throw new Error(`satır bulunamadı: ${text}`);
  const y = box.y + box.height / 2;
  const x0 = dx < 0 ? box.x + 300 : box.x + 20;
  await page.mouse.move(x0, y);
  await page.mouse.down();
  await page.mouse.move(x0 + Math.sign(dx) * 20, y, { steps: 3 });
  await page.mouse.move(x0 + dx, y, { steps: 8 });
  const label = (await list(page).locator('.under').textContent()) ?? '';
  await page.mouse.up();
  return label.trim();
}

const subjects = (page: Page) => page.evaluate(() => window.__mh.state.messages.map((m) => m.subject));

async function height(el: Locator): Promise<number> {
  return Math.round((await el.boundingBox())!.height * 100) / 100;
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('Çöp: "Sil" kaydırması "Kalıcı sil" olur ve onay ister; geri tuşu vazgeçer; silinen geri alınamaz', async ({ page }) => {
  await fillTrash(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır']);
  await setSwipe(page, { swipeLeft: 'trash' });
  await openView(page, /^Çöp/, 'Çöp');

  expect(await swipe(page, 'Cumartesi kahvaltı?', -260)).toBe('Kalıcı sil');
  // Satır yerine döner, onay alt sayfası açılır: ne olacağı ve geri alınamayacağı yazar.
  const sheet = confirm(page, 'Bu posta kalıcı silinsin mi?');
  await expect(sheet).toBeVisible();
  await expect(sheet).toContainText('Bu işlem geri alınamaz');
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  for (const b of [sheet.getByRole('button', { name: 'Vazgeç' }), sheet.getByRole('button', { name: 'Kalıcı sil' })]) {
    expect(await height(b)).toBeGreaterThanOrEqual(48);
  }

  // Geri tuşu yalnız onayı kapatır.
  await page.goBack();
  await expect(sheet).toBeHidden();
  await expect(h1(page, 'Çöp')).toBeVisible();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  expect(await subjects(page)).toContain('Cumartesi kahvaltı?');

  await swipe(page, 'Cumartesi kahvaltı?', -260);
  await confirm(page, 'Bu posta kalıcı silinsin mi?').getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(toast(page, 'Posta kalıcı silindi')).toBeVisible();
  await expect(undo(page)).toHaveCount(0);
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await expect(row(page, 'Sözleşme imzaya hazır')).toBeVisible();
  expect(await subjects(page)).not.toContain('Cumartesi kahvaltı?');
});

test('Çöp: seçim çubuğunda "Kalıcı sil"; Vazgeç seçimi korur, onay seçilenleri siler', async ({ page }) => {
  await fillTrash(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır', 'Ekim faturanız hazır']);
  await openView(page, /^Çöp/, 'Çöp');
  await page.getByRole('button', { name: 'Cumartesi kahvaltı? postasını seç' }).click();
  await page.getByRole('button', { name: 'Sözleşme imzaya hazır postasını seç' }).click();
  const bar = page.getByRole('toolbar', { name: '2 posta seçili' });
  await expect(bar.getByRole('button')).toHaveText(['Arşivle', 'Kalıcı sil', 'İstenmeyen', 'Diğer']);

  await bar.getByRole('button', { name: 'Kalıcı sil' }).click();
  const sheet = confirm(page, '2 posta kalıcı silinsin mi?');
  await expect(sheet).toContainText('Bu işlem geri alınamaz');
  await sheet.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(sheet).toBeHidden();
  await expect(page.getByRole('heading', { level: 1, name: '2 seçili' })).toBeVisible();

  await bar.getByRole('button', { name: 'Kalıcı sil' }).click();
  await confirm(page, '2 posta kalıcı silinsin mi?').getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(toast(page, '2 posta kalıcı silindi')).toBeVisible();
  await expect(undo(page)).toHaveCount(0);
  await expect(h1(page, 'Çöp')).toBeVisible();
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await expect(row(page, 'Sözleşme imzaya hazır')).toHaveCount(0);
  await expect(row(page, 'Ekim faturanız hazır')).toBeVisible();
  const left = await subjects(page);
  expect(left).not.toContain('Cumartesi kahvaltı?');
  expect(left).not.toContain('Sözleşme imzaya hazır');
});

test('Çöp: okuma şeridinde "Kalıcı sil"; geri tuşu onayı kapatır, onay listeye döner', async ({ page }) => {
  await fillTrash(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır']);
  await openView(page, /^Çöp/, 'Çöp');
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  await expect(strip(page).getByRole('button', { name: 'Sil', exact: true })).toHaveCount(0);
  const purge = strip(page).getByRole('button', { name: 'Kalıcı sil' });
  expect(await height(purge)).toBeGreaterThanOrEqual(44);

  await purge.click();
  const sheet = confirm(page, 'Bu posta kalıcı silinsin mi?');
  await expect(sheet).toBeVisible();
  await page.goBack();
  await expect(sheet).toBeHidden();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();

  await purge.click();
  await confirm(page, 'Bu posta kalıcı silinsin mi?').getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(toast(page, 'Posta kalıcı silindi')).toBeVisible();
  await expect(undo(page)).toHaveCount(0);
  await expect(h1(page, 'Çöp')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2 })).toHaveCount(0);
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  await expect(row(page, 'Sözleşme imzaya hazır')).toBeVisible();
  expect(await subjects(page)).not.toContain('Cumartesi kahvaltı?');
});

test('Çöp: kalıcı silmeden sonra ayar "sonraki posta" ise sıradaki posta açılır', async ({ page }) => {
  await fillTrash(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır']);
  await page.evaluate(() => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, afterRemove: 'next' } }));
  await openView(page, /^Çöp/, 'Çöp');
  const first = (await list(page).locator('.subject').first().textContent())!.trim();
  const second = first === 'Cumartesi kahvaltı?' ? 'Sözleşme imzaya hazır' : 'Cumartesi kahvaltı?';
  await row(page, first).click();
  await expect(page.getByRole('heading', { level: 2, name: first })).toBeVisible();
  await strip(page).getByRole('button', { name: 'Kalıcı sil' }).click();
  await confirm(page, 'Bu posta kalıcı silinsin mi?').getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(toast(page, 'Posta kalıcı silindi')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2, name: second })).toBeVisible();
});

test('gelen kutusunda silme eskisi gibi: "Sil", geri alınabilir; menüde boşaltma yok', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click();
  await strip(page).getByRole('button', { name: 'Sil', exact: true }).click();
  await expect(toast(page, 'Çöp kutusuna taşındı')).toBeVisible();
  await expect(undo(page)).toBeVisible();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await openListMenu(page);
  await expect(page.getByRole('menuitem', { name: /boşalt/ })).toHaveCount(0);
});

test('Çöpü boşalt: uygulama çubuğu menüsünden, onaylı; geri tuşu vazgeçer', async ({ page }) => {
  await fillTrash(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır', 'Ekim faturanız hazır']);
  await openView(page, /^Çöp/, 'Çöp');
  await openListMenu(page);
  await page.getByRole('menuitem', { name: 'Çöpü boşalt' }).click();
  const sheet = confirm(page, 'Çöpteki tüm postalar kalıcı silinsin mi?');
  await expect(sheet).toContainText('Bu işlem geri alınamaz');
  await page.goBack();
  await expect(sheet).toBeHidden();
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();

  await openListMenu(page);
  await page.getByRole('menuitem', { name: 'Çöpü boşalt' }).click();
  await confirm(page, 'Çöpteki tüm postalar kalıcı silinsin mi?').getByRole('button', { name: 'Çöpü boşalt' }).click();
  await expect(toast(page, 'Çöp boşaltıldı · 3 posta kalıcı silindi')).toBeVisible();
  await expect(undo(page)).toHaveCount(0);
  await expect(list(page).getByText('Çöp boş')).toBeVisible();
  expect(await page.evaluate(() => window.__mh.state.messages.filter((m) => window.__mh.state.mailboxes.find((b) => b.id === m.mailboxId)?.role === 'trash').length)).toBe(0);

  // Boş kutuda yeniden: silinecek bir şey olmadığı söylenir.
  await openListMenu(page);
  await page.getByRole('menuitem', { name: 'Çöpü boşalt' }).click();
  await confirm(page, 'Çöpteki tüm postalar kalıcı silinsin mi?').getByRole('button', { name: 'Çöpü boşalt' }).click();
  await expect(toast(page, 'Kutu zaten boş')).toBeVisible();
});

test('İstenmeyen: çubukta "İstenmeyen" yok, kaydırma "İstenmeyen değil"; boşaltma yalnız görünen hesabın kutusunu siler', async ({ page }) => {
  // İş hesabının İstenmeyen kutusuna da bir posta, Kişisel'e ikinci bir posta.
  await page.evaluate(() => {
    const s = window.__mh.state;
    const junk = (acc: string) => s.mailboxes.find((b) => b.accountId === acc && b.role === 'junk')!.id;
    s.messages.find((m) => m.subject === 'Sözleşme imzaya hazır')!.mailboxId = junk('demo-is');
    s.messages.find((m) => m.subject === 'Ekim faturanız hazır')!.mailboxId = junk('demo-kisisel');
  });
  await setSwipe(page, { swipeRight: 'spam' });
  await openJunk(page);
  await expect(row(page, 'Kartınız geçici olarak kapatıldı')).toBeVisible();
  await expect(row(page, 'Sözleşme imzaya hazır')).toHaveCount(0);

  // Seçim çubuğu: postalar zaten İstenmeyen'de; "İstenmeyen değil" tek postada "Diğer"de.
  await page.getByRole('button', { name: 'Kartınız geçici olarak kapatıldı postasını seç' }).click();
  const bar = page.getByRole('toolbar', { name: '1 posta seçili' });
  await expect(bar.getByRole('button')).toHaveText(['Arşivle', 'Kalıcı sil', 'Diğer']);
  await bar.getByRole('button', { name: 'Diğer' }).click();
  await expect(page.getByRole('menuitem', { name: 'İstenmeyen değil' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Güvenli say' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Seçimi bırak' }).click();

  // "İstenmeyen" kaydırması burada postayı geri çıkarır.
  expect(await swipe(page, 'Ekim faturanız hazır', 260)).toBe('İstenmeyen değil');
  await expect(toast(page, 'Gönderen güvenli sayıldı; posta gelen kutusuna taşındı')).toBeVisible();
  await expect(row(page, 'Ekim faturanız hazır')).toHaveCount(0);

  await openListMenu(page);
  await expect(page.getByRole('menuitem', { name: 'Çöpü boşalt' })).toHaveCount(0);
  await page.getByRole('menuitem', { name: "İstenmeyen'i boşalt" }).click();
  await confirm(page, "İstenmeyen'deki tüm postalar kalıcı silinsin mi?").getByRole('button', { name: "İstenmeyen'i boşalt" }).click();
  await expect(toast(page, 'İstenmeyen boşaltıldı · 1 posta kalıcı silindi')).toBeVisible();
  const left = await subjects(page);
  expect(left).not.toContain('Kartınız geçici olarak kapatıldı');
  // Öteki hesabın İstenmeyen kutusu ve gelen kutusuna dönen posta yerinde.
  expect(left).toContain('Sözleşme imzaya hazır');
  expect(left).toContain('Ekim faturanız hazır');
});

test('onay alt sayfası 360 px’te ve büyük yazıda taşmaz: uzun düğme adı kırpılmaz, alt alta iner', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  await openJunk(page);
  await openListMenu(page);
  await page.getByRole('menuitem', { name: "İstenmeyen'i boşalt" }).click();
  const sheet = confirm(page, "İstenmeyen'deki tüm postalar kalıcı silinsin mi?");
  const go = sheet.getByRole('button', { name: "İstenmeyen'i boşalt" });
  const cancel = sheet.getByRole('button', { name: 'Vazgeç' });
  const fits = async () => {
    for (const b of [go, cancel]) {
      expect(await b.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      const box = (await b.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(16);
      expect(box.x + box.width).toBeLessThanOrEqual(360 - 16);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)).toBe(false);
  };
  await fits();
  // Sistem yazı ölçeği 1.3×: iki düğme yan yana sığmazsa alt alta iner.
  await page.addStyleTag({ content: ':root { --fs-meta: 15.6px; --fs-body: 19.5px; --fs-section: 24.7px; --fs-title: 31.2px; }' });
  await fits();
});
