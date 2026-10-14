import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Çoklu seçim ve toplu işlemler, göndericiye göre temizlik, sabitleme,
// etiketler, kayıtlı aramalar (sahte arka uçla).

const side = (page: Page) => page.getByRole('navigation', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });

async function openSettings(page: Page, section: string) {
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: section, exact: true }).click();
}

test('çoklu seçim: Ctrl+tık iki satır, toplu arşivle ve geri al; E tuşu seçiliye uygulanır', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Cumartesi kahvaltı?').click({ modifiers: ['Control'] });
  // Tek işaret okuma alanını değiştirmez.
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await list(page).getByText('Gönderiniz dağıtıma çıktı', { exact: true }).click({ modifiers: ['Control'] });
  const panel = page.getByRole('main', { name: '2 posta seçili' });
  await expect(panel).toBeVisible();
  await panel.getByRole('button', { name: /^Arşivle/ }).click();
  await expect(page.getByRole('status')).toContainText('2 posta arşivlendi');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toHaveCount(0);
  await expect(list(page).getByText('Gönderiniz dağıtıma çıktı', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await expect(list(page).getByText('Gönderiniz dağıtıma çıktı', { exact: true })).toBeVisible();

  // Klavye: Shift+J seçimi genişletir, E tümünü arşivler.
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await page.keyboard.press('Shift+j');
  await expect(page.getByRole('main', { name: '2 posta seçili' })).toBeVisible();
  await page.keyboard.press('e');
  await expect(page.getByRole('status')).toContainText('2 posta arşivlendi');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toHaveCount(0);
});

test('çoklu seçim: Shift+tık aralık, Ctrl A tümü, Esc bırakır; okunmadı say ve etiket toplu uygulanır', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await list(page).getByText('Gönderiniz dağıtıma çıktı', { exact: true }).click({ modifiers: ['Shift'] });
  await expect(page.getByRole('main', { name: '3 posta seçili' })).toBeVisible();
  await page.keyboard.press('Control+a');
  await expect(page.getByRole('main', { name: '10 posta seçili' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();

  // Satırdaki seçim düğmesiyle de işaretlenir.
  await list(page).getByRole('button', { name: 'Sözleşme imzaya hazır postasını seç' }).click();
  await list(page).getByRole('button', { name: 'Ekim faturanız hazır postasını seç' }).click();
  const panel = page.getByRole('main', { name: '2 posta seçili' });
  await panel.getByRole('button', { name: /^Okunmadı say/ }).click();
  await expect(page.getByRole('status')).toContainText('2 posta okunmadı sayıldı');
  // Başta 3 okunmamış vardı; açılan posta okundu sayıldı (2), iki posta okunmadı yapıldı (4).
  await expect(page.getByRole('navigation').getByRole('button', { name: /Gelen Kutusu/ })).toContainText('4');

  await panel.getByRole('button', { name: 'Etiketler' }).click();
  await page.getByRole('menuitemcheckbox', { name: 'Proje' }).click();
  await expect(page.getByRole('status')).toContainText('2 postaya Proje etiketi eklendi');
  const tagged = await page.evaluate(() => window.__mh.state.messages.filter((m) => m.tags.includes('etiket-proje')).length);
  expect(tagged).toBe(2);
});

test('göndericiye göre temizlik: en yenisi kalır, kalanı çöpe gider; her adım onay ister', async ({ page }) => {
  await page.goto('/#/main');
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'Yalnız en yenisini tut (eskilerini arşivle)' }).click();
  await expect(page.getByRole('menu', { name: '1 posta arşivlenecek' })).toBeVisible();
  await page.getByRole('menuitem', { name: 'Evet, uygula' }).click();
  await expect(page.getByRole('status')).toContainText('1 posta arşivlendi');
  const archived = await page.evaluate(
    () => window.__mh.state.messages.filter((m) => m.from.email === 'kerem.aydin@ornek.com.tr' && m.mailboxId === 'demo-is:Archive').length,
  );
  expect(archived).toBe(1);

  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'Bu göndericinin tüm postalarını çöpe at' }).click();
  await expect(page.getByRole('menu', { name: '1 posta çöpe atılacak' })).toBeVisible();
  await page.getByRole('menuitem', { name: 'Evet, uygula' }).click();
  await expect(page.getByRole('status')).toContainText('1 posta çöp kutusuna taşındı');
  await expect(list(page).getByText('Re: Q4 bütçe taslağı')).toHaveCount(0);
});

test('sabitleme: P ile sabitlenir, Sabitlenenler grubu en üstte, geri alınır', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Sözleşme imzaya hazır').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Sözleşme imzaya hazır' })).toBeVisible();
  await page.keyboard.press('p');
  await expect(page.getByRole('status')).toContainText('1 posta sabitlendi');
  await expect(list(page).getByText('Sabitlenenler')).toBeVisible();
  await expect(list(page).locator('[data-id]').first()).toContainText('Sözleşme imzaya hazır');
  await expect(list(page).getByRole('img', { name: 'Sabitli' })).toBeVisible();
  const unpin = page.getByRole('main').getByRole('button', { name: 'Sabitlemeyi kaldır' });
  await expect(unpin).toBeVisible();
  await unpin.click();
  await expect(page.getByRole('status')).toContainText('sabitlemesi kaldırıldı');
  await expect(list(page).getByText('Sabitlenenler')).toHaveCount(0);
  await expect(list(page).locator('[data-id]').first()).toContainText('Re: Q4 bütçe taslağı');
});

test('etiketler: ayarlardan eklenir, postaya takılır, kenar çubuğu ve tag: işleci süzer', async ({ page }) => {
  await page.goto('/#/main');
  await openSettings(page, 'Etiketler');
  const settings = page.getByRole('region', { name: 'Ayarlar' });
  await expect(settings.getByText('Fatura', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Yeni etiket' }).click();
  await page.getByLabel('Etiket adı').fill('Acil');
  await page.getByRole('radio', { name: 'Renk 5' }).click();
  await page.getByRole('button', { name: 'Kaydet' }).click();
  await expect(page.getByRole('status')).toContainText('Kaydedildi');
  await expect(settings.getByText('Acil', { exact: true })).toBeVisible();
  // Aynı ad kabul edilmez.
  await page.getByRole('button', { name: 'Yeni etiket' }).click();
  await page.getByLabel('Etiket adı').fill('acil');
  await page.getByRole('button', { name: 'Kaydet' }).click();
  await expect(page.getByRole('alert')).toContainText('Bu adda bir etiket zaten var.');
  await page.getByRole('button', { name: 'Vazgeç' }).click();
  await page.getByRole('button', { name: 'Ayarları kapat' }).click();

  await list(page).getByText('Cumartesi kahvaltı?').click();
  await page.getByRole('main').getByRole('button', { name: 'Etiketler', exact: true }).click();
  await page.getByRole('menuitemcheckbox', { name: 'Acil' }).click();
  await expect(page.getByRole('status')).toContainText('Acil etiketi eklendi');
  await expect(page.getByRole('main').getByRole('button', { name: 'Acil' })).toBeVisible();
  await expect(list(page).getByRole('img', { name: 'Etiketler: Acil' })).toBeVisible();

  await side(page).getByRole('button', { name: 'Acil' }).click();
  await expect(list(page).getByRole('heading', { name: 'Acil' })).toBeVisible();
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toHaveCount(0);

  await page.keyboard.press('Control+k');
  await page.getByRole('combobox').fill('tag:fatura');
  await page.keyboard.press('Enter');
  await expect(list(page).getByText('Ekim faturanız hazır')).toBeVisible();
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toHaveCount(0);

  // Kural eylemi olarak etiket.
  await openSettings(page, 'Kurallar');
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  await page.getByLabel('Kural adı').fill('Kerem acil');
  await page.getByLabel('Değer').fill('kerem');
  await page.getByLabel('Eylem 1').selectOption({ label: 'Etiket ekle' });
  await page.getByLabel('Etiket adı').selectOption({ label: 'Acil' });
  await page.getByRole('button', { name: 'Kuralı kaydet' }).click();
  await expect(page.getByText('Kimden içerir “kerem” → Acil etiketi ekle')).toBeVisible();
});

test('kayıtlı arama: sonuç başlığından kaydedilir, kenar çubuğundan çalışır, yeniden adlandırılır ve silinir', async ({ page }) => {
  await page.goto('/#/main');
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox').fill('from:elif');
  await page.keyboard.press('Enter');
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toBeVisible();
  await list(page).getByRole('button', { name: 'Aramayı kaydet' }).click();
  const dlg = page.getByRole('dialog', { name: 'Aramayı kaydet' });
  await expect(dlg.getByLabel('Arama adı')).toHaveValue('from:elif');
  await dlg.getByLabel('Arama adı').fill('Elif');
  await dlg.getByRole('button', { name: 'Kaydet' }).click();
  await expect(page.getByRole('status')).toContainText('“Elif” kaydedildi');
  const saved = side(page).getByRole('button', { name: 'Elif', exact: true });
  await expect(saved).toBeVisible();

  await side(page).getByRole('button', { name: /Gelen Kutusu/ }).click();
  await expect(list(page).getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
  await saved.click();
  await expect(list(page).getByRole('heading', { name: '“from:elif” için sonuçlar' })).toBeVisible();

  await saved.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Yeniden adlandır' }).click();
  const ren = page.getByRole('dialog', { name: 'Yeniden adlandır' });
  await ren.getByLabel('Arama adı').fill('Elif ekli');
  await ren.getByLabel('Arama adı').press('Enter');
  const renamed = side(page).getByRole('button', { name: 'Elif ekli' });
  await expect(renamed).toBeVisible();

  await renamed.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Sil' }).click();
  await expect(side(page).getByRole('button', { name: 'Elif ekli' })).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('“Elif ekli” silindi');
  expect(await page.evaluate(() => window.__mh.state.settings.savedSearches.length)).toBe(0);
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(side(page).getByRole('button', { name: 'Elif ekli' })).toBeVisible();
});
