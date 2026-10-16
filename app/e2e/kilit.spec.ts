import { expect, test, type Page } from '@playwright/test';

// Parola kilidi (masaüstü): Ayarlar › Gizlilik ve güvenlik'ten kurma (iki kez
// parola, onay kutusu), yeniden yükleme = yeniden başlatma → kilit ekranı,
// yanlış parola, doğru parola → posta; yalnız arayüz kilidi; kapatma; unutma.

const PASS = 'çok gizli parola';
const lockScreen = (page: Page) => page.getByTestId('lock-screen');
const passField = (page: Page) => lockScreen(page).getByLabel('Parola', { exact: true });

async function openPrivacy(page: Page) {
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Gizlilik ve güvenlik', exact: true }).click();
  await expect(page.getByRole('heading', { level: 3, name: 'Parola kilidi' })).toBeVisible();
}

async function enableLock(page: Page) {
  await openPrivacy(page);
  await page.getByRole('button', { name: 'Kilidi kur' }).click();
  const dlg = page.getByRole('dialog', { name: 'Parola kilidini kur' });
  await expect(dlg).toBeVisible();
  const submit = dlg.getByRole('button', { name: 'Kilidi kur' });
  await dlg.getByLabel('Yeni parola', { exact: true }).fill('kisa');
  await expect(dlg.getByText('En az 8 karakter.')).toBeVisible();
  await dlg.getByLabel('Yeni parola', { exact: true }).fill(PASS);
  await expect(dlg.getByText(/Parola gücü:/)).toBeVisible();
  await dlg.getByLabel('Yeni parola (yeniden)').fill('başka bir şey');
  await expect(dlg.getByRole('alert')).toHaveText('İki parola aynı değil.');
  await expect(submit).toBeDisabled();
  await dlg.getByLabel('Yeni parola (yeniden)').fill(PASS);
  // Unutma uyarısı açık ve onay kutusu zorunlu.
  await expect(dlg.getByText(/Parolayı unutursan/)).toBeVisible();
  await expect(submit).toBeDisabled();
  await dlg.getByLabel(/Anladım/).check();
  await submit.click();
  await expect(dlg).toHaveCount(0);
  await expect(page.getByText('Parola kilidi kuruldu.')).toBeVisible();
  await expect(page.getByText('Açık', { exact: true })).toBeVisible();
  await expect(page.getByRole('switch', { name: 'Pencereyi her açışta parola sor' })).toBeChecked();
}

test.beforeEach(async ({ page }) => {
  await page.goto('/#/main');
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
});

test('kilit kapalıyken hiçbir yerde parola sorulmaz; ayar bölümü kapalı der', async ({ page }) => {
  await expect(lockScreen(page)).toHaveCount(0);
  await openPrivacy(page);
  await expect(page.getByText(/^Kapalı\./)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Kilidi kur' })).toBeVisible();
  await expect(page.getByRole('switch', { name: 'Pencereyi her açışta parola sor' })).toHaveCount(0);
});

test('kur → yeniden başlat → kilit ekranı → yanlış → doğru → posta görünür; yalnız arayüz kilidi; kapat', async ({ page }) => {
  await enableLock(page);

  // Yeniden yükleme yeniden başlatmadır: çekirdek kapalı, yalnız kilit ekranı.
  await page.reload();
  await expect(lockScreen(page)).toBeVisible();
  await expect(page.getByRole('heading', { level: 1, name: 'Mailhere kilitli' })).toBeVisible();
  await expect(page.getByText(/Postaları almak ve okumak için parolanı gir/)).toBeVisible();
  // Posta verisi DOM'da yok: liste, kenar çubuğu, arama yok.
  await expect(page.getByRole('region', { name: 'Posta listesi' })).toHaveCount(0);
  await expect(page.getByRole('navigation', { name: 'Kutular ve hesaplar' })).toHaveCount(0);
  await expect(page.locator('.search')).toHaveCount(0);
  expect(await page.title()).toBe('Mailhere');
  await expect(passField(page)).toBeFocused();
  await expect(passField(page)).toHaveAttribute('autocomplete', 'current-password');

  // Yanlış parola: hata ne oldu + ne yapılabilir, alan temizlenir.
  await passField(page).fill('yanlış parola');
  await lockScreen(page).getByRole('button', { name: 'Kilidi aç' }).click();
  await expect(lockScreen(page).getByRole('alert')).toHaveText('Parola yanlış. Yeniden dene.');
  await expect(passField(page)).toHaveValue('');
  // Göster/gizle.
  await passField(page).fill(PASS);
  await lockScreen(page).getByRole('button', { name: 'Parolayı göster' }).click();
  await expect(passField(page)).toHaveAttribute('type', 'text');
  await lockScreen(page).getByRole('button', { name: 'Parolayı gizle' }).click();
  await expect(passField(page)).toHaveAttribute('type', 'password');
  await passField(page).press('Enter');
  await expect(lockScreen(page)).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Posta listesi' })).toBeVisible();

  // Yalnız arayüz kilidi (pencere gizlenip açıldı): çekirdek açık, kilit ekranı.
  await page.evaluate(() => (window as unknown as { __mh: { call: (n: string) => Promise<unknown> } }).__mh.call('lock_ui_lock'));
  await expect(lockScreen(page)).toBeVisible();
  await expect(page.getByText(/Pencere yeniden açıldı/)).toBeVisible();
  await expect(page.getByRole('region', { name: 'Posta listesi' })).toHaveCount(0);
  // Kısayollar kilitliyken çalışmaz (komut paleti açılmaz).
  await page.keyboard.press('Control+k');
  await expect(page.getByRole('dialog', { name: 'Komut paleti' })).toHaveCount(0);
  await passField(page).fill(PASS);
  await lockScreen(page).getByRole('button', { name: 'Kilidi aç' }).click();
  await expect(page.getByRole('region', { name: 'Posta listesi' })).toBeVisible();

  // Tercih kapatılır, parola değiştirilir, kilit kapatılır (mevcut parola ile).
  await openPrivacy(page);
  await page.getByRole('switch', { name: 'Pencereyi her açışta parola sor' }).uncheck();
  await expect(page.getByRole('switch', { name: 'Pencereyi her açışta parola sor' })).not.toBeChecked();
  await page.getByRole('button', { name: 'Parolayı değiştir' }).click();
  const change = page.getByRole('dialog', { name: 'Parolayı değiştir' });
  await change.getByLabel('Mevcut parola').fill('yanlış parola');
  await change.getByLabel('Yeni parola', { exact: true }).fill('yeni parola 12');
  await change.getByLabel('Yeni parola (yeniden)').fill('yeni parola 12');
  await change.getByRole('button', { name: 'Parolayı değiştir' }).click();
  await expect(change.getByRole('alert')).toHaveText('Parola yanlış. Yeniden dene.');
  await change.getByLabel('Mevcut parola').fill(PASS);
  await change.getByRole('button', { name: 'Parolayı değiştir' }).click();
  await expect(change).toHaveCount(0);
  await expect(page.getByText('Parola değiştirildi.')).toBeVisible();

  await page.getByRole('button', { name: 'Kilidi kapat' }).click();
  const off = page.getByRole('dialog', { name: 'Parola kilidini kapat' });
  await off.getByLabel('Mevcut parola').fill('yeni parola 12');
  await off.getByRole('button', { name: 'Kilidi kapat' }).click();
  await expect(off).toHaveCount(0);
  await expect(page.getByText('Parola kilidi kapatıldı.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Kilidi kur' })).toBeVisible();
  // Kapalı: yeniden yüklemede parola sorulmaz.
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
  await expect(lockScreen(page)).toHaveCount(0);
});

test('beş yanlıştan sonra artan bekleme; sayaç biter, doğru parola açar', async ({ page }) => {
  await enableLock(page);
  await page.reload();
  await expect(lockScreen(page)).toBeVisible();
  for (let i = 0; i < 6; i++) {
    await passField(page).fill(`yanlış ${i}`);
    await lockScreen(page).getByRole('button', { name: 'Kilidi aç' }).click();
    await expect(lockScreen(page).getByRole('alert')).toContainText('Parola yanlış');
  }
  // Altıncı hatadan sonra 2 sn bekleme: düğme kapalı, sayaç yazar.
  await expect(lockScreen(page).getByRole('status')).toContainText(/saniye sonra/);
  await passField(page).fill(PASS);
  await expect(lockScreen(page).getByRole('button', { name: 'Kilidi aç' })).toBeDisabled();
  await expect(lockScreen(page).getByRole('button', { name: 'Kilidi aç' })).toBeEnabled({ timeout: 5000 });
  await lockScreen(page).getByRole('button', { name: 'Kilidi aç' }).click();
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
});

test('parolamı unuttum: açık uyarı, onay, yerel veri silinir ve ilk açılış ekranı gelir', async ({ page }) => {
  await enableLock(page);
  await page.reload();
  await expect(lockScreen(page)).toBeVisible();
  await lockScreen(page).getByRole('button', { name: 'Parolamı unuttum' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Parolamı unuttum' })).toBeVisible();
  await expect(page.getByText(/yerel postalar, taslaklar, ayarlar ve hesap bilgileri silinir/)).toBeVisible();
  await expect(page.getByText(/Sunucudaki postalar etkilenmez/)).toBeVisible();
  await expect(page.getByText(/Hesapları yeniden eklemen/)).toBeVisible();
  // Vazgeç geri döner.
  await lockScreen(page).getByRole('button', { name: 'Vazgeç' }).click();
  await expect(passField(page)).toBeVisible();
  await lockScreen(page).getByRole('button', { name: 'Parolamı unuttum' }).click();
  await lockScreen(page).getByRole('button', { name: 'Yerel veriyi sil ve baştan başla' }).click();
  // Boş çekirdek: hesap ekleme ekranı, kilit yok; yeniden yüklemede de parola sorulmaz.
  await expect(page.getByRole('heading', { level: 1, name: 'Hesap ekle' })).toBeVisible();
  await expect(lockScreen(page)).toHaveCount(0);
  await page.reload();
  await expect(lockScreen(page)).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
});
