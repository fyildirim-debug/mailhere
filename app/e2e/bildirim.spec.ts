import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Masaüstü bildirim kartının düğmeleri: varsayılan sıra, yerinde yanıt
// (gönder, geri al), onay satırı ve geri alma, kartta hata; Ayarlar ›
// Bildirimler'deki "Bildirim düğmeleri" seçimi kartı değiştirir.

const card = (page: Page) => page.getByRole('alertdialog', { name: 'Yeni posta: Ayşe Demir' });
const acts = (page: Page) => card(page).getByRole('group', { name: 'Posta eylemleri' });
const SUBJECT = 'Cumartesi kahvaltı?';

/** Örnek kartın postasının bulunduğu kutunun rolü ve okunmamışlığı. */
function where(page: Page) {
  return page.evaluate((subject) => {
    const mh = window.__mh;
    const m = mh.state.messages.find((x) => x.subject === subject)!;
    return { role: mh.state.mailboxes.find((b) => b.id === m.mailboxId)?.role, unread: m.unread };
  }, SUBJECT);
}

async function openNotifySettings(page: Page) {
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Bildirimler', exact: true }).click();
  await expect(page.getByRole('heading', { level: 3, name: 'Bildirim düğmeleri' })).toBeVisible();
}

test('kart: varsayılan düğmeler Aç, Yanıtla, Sil, İstenmeyen; tek sırada, kartın içinde', async ({ page }) => {
  await page.goto('/#/notify');
  await expect(card(page)).toBeVisible();
  const buttons = acts(page).getByRole('button');
  await expect(buttons).toHaveText(['Aç', 'Yanıtla', 'Sil', 'İstenmeyen']);
  await expect(acts(page).getByRole('button', { name: 'Aç' })).toHaveClass(/btn-primary/);
  // Erişilebilir ad görünen etiketi içerir, düğme ne yaptığını söyler.
  await expect(acts(page).getByRole('button', { name: 'İstenmeyen olarak işaretle' })).toBeVisible();
  const boxes = await buttons.evaluateAll((els) => els.map((e) => e.getBoundingClientRect()).map((r) => ({ top: r.top, right: r.right })));
  const cardBox = (await card(page).boundingBox())!;
  expect(new Set(boxes.map((b) => Math.round(b.top))).size).toBe(1);
  for (const b of boxes) expect(b.right).toBeLessThanOrEqual(cardBox.x + cardBox.width);
  expect(await acts(page).evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
});

test('yanıt: kartın içinde açılır, yazarken kart kapanmaz; Ctrl Enter giden kutusuna koyar, Geri al metni geri getirir', async ({ page }) => {
  await page.goto('/#/notify');
  await acts(page).getByRole('button', { name: 'Yanıtla' }).click();
  const box = card(page).getByRole('textbox', { name: 'Yanıt metni' });
  await expect(box).toBeFocused();
  await expect(card(page).getByText('Kime: Ayşe Demir')).toBeVisible();
  // Geri sayım yok: yazarken kart kendiliğinden kapanmaz.
  await expect(card(page).locator('.bar')).toHaveCount(0);
  await expect(card(page).getByRole('button', { name: 'Gönder' })).toBeDisabled();

  // Kutu 1–4 satır büyür.
  const one = (await box.boundingBox())!.height;
  await box.fill('bir\niki\nüç');
  const three = (await box.boundingBox())!.height;
  await box.fill('1\n2\n3\n4\n5\n6\n7');
  const many = (await box.boundingBox())!.height;
  expect(three).toBeGreaterThan(one);
  expect(many).toBeLessThanOrEqual(one + 3 * 20 + 1);

  // Esc alanı kapatır; yazılan korunur.
  await box.press('Escape');
  await expect(box).toHaveCount(0);
  await expect(acts(page).getByRole('button', { name: 'Yanıtla' })).toBeFocused();
  await acts(page).getByRole('button', { name: 'Yanıtla' }).click();
  await expect(box).toHaveValue('1\n2\n3\n4\n5\n6\n7');

  await box.fill('Olur, 10 gibi oradayım.');
  await box.press('Control+Enter');
  await expect(card(page).getByRole('status')).toContainText('Yanıt giden kutusunda');
  await expect(card(page).getByRole('status')).toContainText('sn içinde gider · Ayşe Demir');
  const outbox = () => page.evaluate(() => window.__mh.call('outbox_list').then((l) => l.map((x) => x.subject)));
  expect(await outbox()).toEqual([`Re: ${SUBJECT}`]);
  expect((await where(page)).unread).toBe(false);

  await card(page).getByRole('button', { name: 'Geri al' }).click();
  await expect(box).toHaveValue('Olur, 10 gibi oradayım.');
  await expect(box).toBeFocused();
  expect(await outbox()).toEqual([]);

  // Vazgeç kartı posta haline döndürür, geri sayım yeniden başlar.
  await card(page).getByRole('button', { name: 'Vazgeç' }).click();
  await expect(acts(page)).toBeVisible();
  await expect(card(page).locator('.bar')).toHaveCount(1);
});

test('yanıt: hata kartın içinde yazar, metin kaybolmaz', async ({ page }) => {
  await page.goto('/#/notify');
  await expect(card(page)).toBeVisible();
  await page.evaluate(() => {
    window.__mh.state.messages = window.__mh.state.messages.filter((m) => m.subject !== 'Cumartesi kahvaltı?');
  });
  await acts(page).getByRole('button', { name: 'Yanıtla' }).click();
  const box = card(page).getByRole('textbox', { name: 'Yanıt metni' });
  await box.fill('Geliyorum.');
  await card(page).getByRole('button', { name: 'Gönder' }).click();
  await expect(card(page).getByRole('alert')).toContainText('Olmadı:');
  await expect(box).toHaveValue('Geliyorum.');
  await expect(box).toHaveAttribute('aria-invalid', 'true');
});

test('Sil: onay satırı ve Geri al; Okundu: süre bitince kart kapanır', async ({ page }) => {
  await page.goto('/#/notify');
  await acts(page).getByRole('button', { name: 'Sil' }).click();
  const status = card(page).getByRole('status');
  await expect(status).toContainText('Silindi');
  await expect(status).toContainText(`Ayşe Demir · ${SUBJECT}`);
  expect((await where(page)).role).toBe('trash');

  await card(page).getByRole('button', { name: 'Geri al' }).click();
  await expect(acts(page).getByRole('button', { name: 'Aç' })).toBeVisible();
  expect((await where(page)).role).toBe('inbox');

  // Okundu düğmesini seç; onay satırı birkaç saniye sonra kartla birlikte kapanır.
  await page.evaluate(async () => {
    const s = await window.__mh.call('settings_get');
    await window.__mh.call('settings_set', { settings: { ...s, notifyActions: ['markRead', 'archive'] } });
  });
  await expect(acts(page).getByRole('button')).toHaveText(['Aç', 'Okundu', 'Arşivle']);
  await acts(page).getByRole('button', { name: 'Okundu say' }).click();
  await expect(card(page).getByRole('status')).toContainText('Okundu sayıldı');
  expect((await where(page)).unread).toBe(false);
  await page.mouse.move(700, 700);
  await expect(card(page)).toHaveCount(0, { timeout: 8000 });
  await expect(page.getByRole('button', { name: 'Bildirimi yeniden göster' })).toBeVisible();
});

test('İstenmeyen: hesapta klasör yoksa ne olduğu ve ne yapılabileceği kartta yazar, kart kalır', async ({ page }) => {
  await page.goto('/#/notify');
  await expect(card(page)).toBeVisible();
  await page.evaluate((subject) => {
    const mh = window.__mh;
    const m = mh.state.messages.find((x) => x.subject === subject)!;
    mh.state.mailboxes = mh.state.mailboxes.filter((b) => !(b.accountId === m.accountId && b.role === 'junk'));
  }, SUBJECT);
  await acts(page).getByRole('button', { name: 'İstenmeyen olarak işaretle' }).click();
  await expect(card(page).getByRole('alert')).toHaveText('Olmadı: Bu hesapta İstenmeyen klasörü yok. Postayı uygulamada aç ve bir klasöre taşı.');
  await expect(card(page).locator('.bar')).toHaveCount(0);
  expect((await where(page)).role).toBe('inbox');
  // Başka bir düğme çalışınca hata kalkar.
  await acts(page).getByRole('button', { name: 'Sil' }).click();
  await expect(card(page).getByRole('status')).toContainText('Silindi');
  await expect(card(page).getByRole('alert')).toHaveCount(0);
});

test('ayar: en çok üç düğme, sıra sabit; kart seçilen düğmeleri gösterir', async ({ page }) => {
  await page.goto('/#/main');
  await openNotifySettings(page);
  const sw = (name: string) => page.getByRole('switch', { name, exact: true });
  for (const name of ['Yanıtla', 'Sil', 'İstenmeyen']) await expect(sw(name)).toBeChecked();
  // Üçü seçili: diğerleri seçilemez, neden yazar.
  for (const name of ['Okundu', 'Arşivle']) await expect(sw(name)).toBeDisabled();
  await expect(page.getByText('Üç düğme seçili. Başka birini seçmek için önce birini kapat.')).toBeVisible();
  await expect(page.getByText('En çok üç düğme; Android de üç gösterir.')).toBeVisible();
  const preview = page.getByRole('img', { name: /^Kart önizlemesi/ });
  await expect(preview).toHaveAccessibleName('Kart önizlemesi: Aç, Yanıtla, Sil, İstenmeyen');

  await sw('Sil').uncheck();
  await expect(sw('Okundu')).toBeEnabled();
  await expect(page.getByText('Üç düğme seçili.')).toHaveCount(0);
  await sw('Okundu').check();
  await expect(sw('Arşivle')).toBeDisabled();
  await expect(preview).toHaveAccessibleName('Kart önizlemesi: Aç, Yanıtla, Okundu, İstenmeyen');
  expect(await page.evaluate(() => window.__mh.state.settings.notifyActions)).toEqual(['reply', 'markRead', 'spam']);

  // Kart penceresi ayarı okur.
  await page.goto('/#/notify');
  await expect(acts(page).getByRole('button')).toHaveText(['Aç', 'Yanıtla', 'Okundu', 'İstenmeyen']);
  // Ayar değişince açık kart da güncellenir; hiç düğme seçilmezse yalnız "Aç".
  await page.evaluate(async () => {
    const s = await window.__mh.call('settings_get');
    await window.__mh.call('settings_set', { settings: { ...s, notifyActions: [] } });
  });
  await expect(acts(page).getByRole('button')).toHaveText(['Aç']);
});
