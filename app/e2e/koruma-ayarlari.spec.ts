import { expect, test, type Page } from '@playwright/test';
import { list, toast } from './koruma';

// 0.11 (masaüstü): Ayarlar › Gizlilik ve güvenlik › İstenmeyen koruması,
// Engellenen göndericiler ve Doğrulama kodları.

async function openPrivacy(page: Page) {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox').fill('engellenen');
  await page.getByRole('option', { name: /Ayarlar: Gizlilik ve güvenlik/ }).click();
  await expect(page.getByRole('heading', { level: 3, name: 'İstenmeyen koruması' })).toBeVisible();
}

test('ayarlar: istenmeyen koruması ve doğrulama kodları ayarı yazar', async ({ page }) => {
  await openPrivacy(page);
  const s = () => page.evaluate(() => window.__mh.state.settings);
  const filter = page.getByRole('radiogroup', { name: 'Şüpheli posta gelince' });
  await expect(filter.getByRole('radio')).toHaveText(['Kapalı', 'Uyar', "İstenmeyen'e taşı"]);
  await expect(filter.getByRole('radio', { name: 'Uyar' })).toBeChecked();
  await filter.getByRole('radio', { name: "İstenmeyen'e taşı" }).click();
  await expect.poll(async () => (await s()).spamFilter).toBe('move');
  await expect(page.getByText('Posta İstenmeyen kutusuna taşınır, bildirimi çıkmaz.', { exact: false })).toBeVisible();
  await filter.getByRole('radio', { name: 'Kapalı' }).click();
  await expect.poll(async () => (await s()).spamFilter).toBe('off');

  const auto = page.getByRole('switch', { name: 'Kodu kendiliğinden kopyala' });
  const read = page.getByRole('switch', { name: 'Kopyalayınca okundu say' });
  await expect(auto).not.toBeChecked();
  await expect(read).toBeChecked();
  await auto.click();
  await read.click();
  await expect.poll(async () => [(await s()).otpAutoCopy, (await s()).otpMarkRead]).toEqual([true, false]);
  const expire = page.getByRole('radiogroup', { name: 'Kod postalarını sil' });
  await expect(expire.getByRole('radio')).toHaveText(['Kapalı', '1 saat sonra', '1 gün sonra']);
  await expect(expire.getByRole('radio', { name: 'Kapalı' })).toBeChecked();
  await expire.getByRole('radio', { name: '1 gün sonra' }).click();
  await expect.poll(async () => (await s()).otpExpireHours).toBe(24);
  await expire.getByRole('radio', { name: '1 saat sonra' }).click();
  await expect.poll(async () => (await s()).otpExpireHours).toBe(1);
});

test('ayarlar: engellenen göndericiler eklenir, doğrulanır, kaldırılır ve geri alınır', async ({ page }) => {
  await openPrivacy(page);
  const blocked = () => page.evaluate(() => window.__mh.state.settings.blocked);
  const box = page.getByRole('textbox', { name: 'Engellenecek adres ya da alan adı' });
  const add = page.getByRole('button', { name: 'Engelle', exact: true });
  const entries = page.getByRole('list', { name: 'Engellenen göndericiler' });
  // Boş durum: ne oldu + ne yapılabilir.
  await expect(page.getByText('Engellenen gönderici yok. Buraya adres yazabilir', { exact: false })).toBeVisible();

  // Geçersiz girdi: açık hata, girdi yerinde kalır.
  await box.fill('bu bir adres değil');
  await add.click();
  const error = page.getByRole('alert');
  await expect(error).toContainText('Bu bir adres ya da alan adı değil.');
  await expect(box).toHaveAttribute('aria-invalid', 'true');
  await expect(box).toBeFocused();
  expect(await blocked()).toEqual([]);
  // Yazmaya başlayınca hata kalkar.
  await box.fill('Reklam@Ornek-Kampanya.com ');
  await expect(error).toHaveCount(0);
  await box.press('Enter');
  await expect(entries.getByRole('listitem')).toHaveText([/reklam@ornek-kampanya\.com/]);
  await expect(box).toHaveValue('');
  // Alan adı girdisi.
  await box.fill('@spam-alan.example');
  await box.press('Enter');
  await expect(entries.getByRole('listitem').nth(1)).toContainText('@spam-alan.example');
  await expect(entries.getByRole('listitem').nth(1)).toContainText('alan adının tümü');
  expect(await blocked()).toEqual(['reklam@ornek-kampanya.com', '@spam-alan.example']);
  await expect(page.getByText('2 girdi · en çok 500')).toBeVisible();

  // Yinelenen, kendi adresimiz ve boş girdi.
  await box.fill('REKLAM@ornek-kampanya.com');
  await box.press('Enter');
  await expect(error).toContainText('reklam@ornek-kampanya.com zaten listede.');
  const mine = await page.evaluate(() => window.__mh.state.accounts[0].email);
  await box.fill(mine);
  await box.press('Enter');
  await expect(error).toContainText('kendi adresin engellenemez');
  await box.fill('');
  await add.click();
  await expect(error).toContainText('Önce bir adres ya da alan adı yaz');
  expect(await blocked()).toHaveLength(2);

  // Kaldır ve geri al.
  await page.getByRole('button', { name: 'reklam@ornek-kampanya.com engelini kaldır' }).click();
  await expect(toast(page)).toContainText('reklam@ornek-kampanya.com engeli kaldırıldı');
  await expect.poll(blocked).toEqual(['@spam-alan.example']);
  await toast(page).getByRole('button', { name: 'Geri al' }).click();
  await expect.poll(blocked).toEqual(['reklam@ornek-kampanya.com', '@spam-alan.example']);
});

test('ayarlar: engellenenler listesi doluyken ekleme nedeniyle reddedilir', async ({ page }) => {
  await openPrivacy(page);
  await page.evaluate(() => {
    const mh = window.__mh;
    const full = Array.from({ length: 500 }, (_, i) => `kisi${i}@dolu.example`);
    return mh.call('settings_set', { settings: { ...mh.state.settings, blocked: full } });
  });
  await expect(page.getByText('500 girdi · en çok 500')).toBeVisible();
  const box = page.getByRole('textbox', { name: 'Engellenecek adres ya da alan adı' });
  await box.fill('yeni@ornek.example');
  await box.press('Enter');
  await expect(page.getByRole('alert')).toContainText('Liste dolu (en çok 500 girdi).');
  // Uzun liste sayfayı uzatmaz: kendi içinde kayar.
  const entries = page.getByRole('list', { name: 'Engellenen göndericiler' });
  expect(await entries.evaluate((el) => el.clientHeight <= 320 && el.scrollHeight > el.clientHeight)).toBe(true);
});
