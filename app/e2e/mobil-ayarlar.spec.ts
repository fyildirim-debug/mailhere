import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil kabuk (Pixel 7, `?kabuk=mobil`): ayarlar listesi ve bölümleri, tema/dil,
// hesap sayfası, kural düzenleyici, masaüstüne özgü alanların gizliliği,
// hesap ekleme akışı ve planlananlar ekranı.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name });

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

/** Çekmeceden Ayarlar'ı açar; bölüm verilirse listeden ona dokunur. */
async function openSettings(page: Page, section?: string) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await expect(h1(page, 'Ayarlar')).toBeVisible();
  if (section) {
    await page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: section, exact: true }).click();
    await expect(h1(page, section)).toBeVisible();
  }
}

/**
 * Öğenin dokunma hedefi en az 44 px yüksek. Ölçü yüzde bire yuvarlanır:
 * yüksek piksel oranında (Pixel 7: 2.625) 44 px'lik kutu 43.99998 ölçülebilir.
 */
async function expectTouchHeight(locator: ReturnType<Page['getByRole']>) {
  const box = await locator.boundingBox();
  expect(box, 'öğe görünür olmalı').not.toBeNull();
  expect(Math.round(box!.height * 100) / 100).toBeGreaterThanOrEqual(44);
}

test('ayarlar: liste → bölüm → geri; kenar çubuğu yok, yatay taşma yok', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page);
  const nav = page.getByRole('navigation', { name: 'Ayar bölümleri' });
  for (const name of ['Genel', 'Bildirimler', 'Yazma ve gönderme', 'Kurallar', 'Etiketler', 'Gizlilik ve güvenlik', 'Hakkında', 'Hesap ekle']) {
    await expect(nav.getByRole('button', { name, exact: true })).toBeVisible();
  }
  // Masaüstü ayar gezinmesi (sol sütun) yoktur; satırlar dokunma hedefidir.
  await expect(page.getByRole('navigation', { name: 'Ayarlar' })).toHaveCount(0);
  await expectTouchHeight(nav.getByRole('button', { name: 'Genel', exact: true }));
  await expect(nav).toContainText(/Mailhere \d+\.\d+/);

  await nav.getByRole('button', { name: 'Gizlilik ve güvenlik', exact: true }).click();
  await expect(h1(page, 'Gizlilik ve güvenlik')).toBeVisible();
  await expect(page.getByText('Android anahtar deposunda (Keystore)')).toBeVisible();
  await expect(page.getByText('Güvenilen sertifikalar')).toBeVisible();
  // Bölüm başlığı yalnız uygulama çubuğunda; sayfa içi h2 yinelenmez.
  await expect(page.getByRole('heading', { level: 2, name: 'Gizlilik ve güvenlik' })).toBeHidden();

  await page.getByRole('button', { name: 'Geri', exact: true }).click();
  await expect(h1(page, 'Ayarlar')).toBeVisible();
  await page.goBack();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});

test('masaüstüne özgü alanlar mobilde gizli: bildirim kartı, güncelleyici, kısayollar', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page, 'Bildirimler');
  await expect(page.getByRole('radio', { name: 'Kişilerden' })).toBeVisible();
  await expect(page.getByText('Bildirim göstermeyen postalar yalnız sayacı artırır.')).toBeVisible();
  await expect(page.getByRole('heading', { level: 3, name: 'Sessiz saatler' })).toBeVisible();
  await expect(page.getByText('Bildirim kartı')).toHaveCount(0);
  await expect(page.getByText('Kartın yeri')).toHaveCount(0);
  await expect(page.getByText('Ekranda kalma süresi')).toHaveCount(0);
  await expect(page.getByRole('switch', { name: 'Sistem bildirimini kullan' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Örnek kart göster' })).toHaveCount(0);
  // Anahtar adıyla aynı satırda, sağ kenarda durur.
  const sw = await page.getByRole('switch', { name: 'İlk kez yazanları elekte beklet' }).boundingBox();
  expect(sw!.x + sw!.width).toBeGreaterThan(page.viewportSize()!.width - 40);

  await page.goBack();
  await page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: 'Hakkında', exact: true }).click();
  await expect(h1(page, 'Hakkında')).toBeVisible();
  await expect(page.getByRole('heading', { level: 3, name: /Mailhere \d+\.\d+/ })).toBeVisible();
  await expect(page.getByText('MIT lisanslı')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Yeni sürüm GitHub’da' })).toBeVisible();
  await expect(page.getByRole('switch', { name: 'Güncellemeleri kendiliğinden denetle' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Şimdi denetle' })).toHaveCount(0);
  await expect(page.getByText('Klavye kısayolları')).toHaveCount(0);
});

test('bildirim düğmeleri: beş anahtar, en çok üç; satır dokunma hedefi, kart önizlemesi yok', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page, 'Bildirimler');
  await expect(page.getByRole('heading', { level: 3, name: 'Bildirim düğmeleri' })).toBeVisible();
  await expect(page.getByText('Telefon bildiriminin altında, bu sırayla durur. En çok üç düğme.')).toBeVisible();
  const sw = (name: string) => page.getByRole('switch', { name, exact: true });
  for (const name of ['Yanıtla', 'Sil', 'İstenmeyen']) await expect(sw(name)).toBeChecked();
  await expect(sw('Okundu')).toBeDisabled();
  await expect(sw('Arşivle')).toBeDisabled();
  await expect(page.getByText('Üç düğme seçili. Başka birini seçmek için önce birini kapat.')).toBeVisible();
  // Satırın tamamı anahtarın etiketi: satıra dokununca değişir.
  const row = page.locator('label').filter({ has: sw('İstenmeyen') });
  await expectTouchHeight(row);
  await row.getByText('Hesabın İstenmeyen klasörüne taşır.').click();
  await expect(sw('İstenmeyen')).not.toBeChecked();
  await expect(sw('Arşivle')).toBeEnabled();
  await sw('Arşivle').check();
  expect(await page.evaluate(() => window.__mh.state.settings.notifyActions)).toEqual(['reply', 'archive', 'trash']);
  // Kart önizlemesi masaüstüne özgü; ekran yatay taşmaz.
  await expect(page.getByRole('img', { name: /Kart önizlemesi/ })).toHaveCount(0);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});

test('bildirim izni: hesap varken ilk açılışta bir kez istenir', async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  // Sahte arka uç isteği onaylar; bölüm izni açık gösterir.
  await openSettings(page, 'Bildirimler');
  const row = page.locator('.set-row', { hasText: 'Telefon bildirimleri' });
  await expect(row).toContainText('Yeni posta telefon bildirimi olarak gelir.');
  await expect(row.getByText('Açık', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem('mailhere.notifyAsked'))).toBe('1');
});

test('bildirim izni: daha önce sorulduysa yeniden sorulmaz; ayarlardan verilir', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('mailhere.notifyAsked', '1'));
  await page.goto('#/main');
  await openSettings(page, 'Bildirimler');
  const row = page.locator('.set-row', { hasText: 'Telefon bildirimleri' });
  await expect(row).toContainText('bildirim izni gerekir');
  const allow = row.getByRole('button', { name: 'İzin ver', exact: true });
  await expectTouchHeight(allow);
  await allow.click();
  await expect(row.getByText('Açık', { exact: true })).toBeVisible();
  await expect(allow).toHaveCount(0);
});

test('bildirim sağlığı: anlık bildirim varsayılan açık, kapatılıp açılır; pil kısıtlaması kaldırılır', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page, 'Bildirimler');
  const system = page.locator('.set-row', { hasText: 'Telefon bildirimleri' });
  const instant = page.getByRole('switch', { name: 'Anlık bildirim' });
  const row = page.locator('.set-row', { has: instant });
  // Varsayılan açık: uygulama kapalıyken ve telefon yeniden başladıktan sonra da bağlı.
  await expect(instant).toBeChecked();
  await expect(row).toContainText('Arka planda da bağlı');
  await expect(row).toContainText('telefon yeniden başladıktan sonra');
  // Telefon bildirimleri satırının altında; anahtar adıyla aynı satırda, sağ kenarda.
  const above = await system.boundingBox();
  const own = await row.boundingBox();
  expect(own!.y).toBeGreaterThanOrEqual(above!.y + above!.height - 1);
  const sw = await instant.boundingBox();
  expect(sw!.x + sw!.width).toBeGreaterThan(page.viewportSize()!.width - 40);
  await instant.click();
  await expect(instant).not.toBeChecked();
  await expect(row).toContainText('15 dakikada bir');
  await instant.click();
  await expect(instant).toBeChecked();
  await expect(row).toContainText('Arka planda da bağlı');

  // Pil kısıtlaması: açıklama + düğme; kaldırılınca "Yok" ve düğme kalkar.
  const battery = page.locator('.set-row', { hasText: 'Pil kısıtlaması' });
  await expect(battery).toContainText('bildirim gecikir');
  const lift = battery.getByRole('button', { name: 'Kısıtlamayı kaldır', exact: true });
  await expectTouchHeight(lift);
  await lift.click();
  await expect(battery.getByText('Yok', { exact: true })).toBeVisible();
  await expect(battery).toContainText('uyutmaz');
  await expect(lift).toHaveCount(0);
  // Üretici notu bağlantı değil, kısa açıklama.
  await expect(page.getByText(/^Bazı üreticiler/)).toBeVisible();
  await expect(page.getByRole('link', { name: /dontkillmyapp/i })).toHaveCount(0);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});

test('tema ve dil Genel bölümünden değişir', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page, 'Genel');
  await page.getByRole('radio', { name: 'Koyu' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(bg).toBe('rgb(15, 17, 21)');
  await page.getByRole('radio', { name: 'Açık' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');

  await page.getByRole('radio', { name: 'English' }).click();
  await expect(h1(page, 'General')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await page.getByRole('radio', { name: 'Türkçe' }).click();
  await expect(h1(page, 'Genel')).toBeVisible();
  // Segmentler 44 px dokunma hedefi.
  await expectTouchHeight(page.getByRole('radio', { name: 'Türkçe' }));
});

test('hesap sayfası: ad ve renk kaydedilir, imza eklenir; sekme şeridi yatay kayar', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page);
  await page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: /^İş/ }).click();
  await expect(h1(page, 'İş')).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Genel' })).toHaveAttribute('aria-selected', 'true');
  await expectTouchHeight(page.getByRole('tab', { name: 'Genel' }));
  await expectTouchHeight(page.getByLabel('Hesap adı'));
  await page.getByLabel('Hesap adı').fill('Ofis');
  await page.getByRole('radio', { name: 'Renk 4' }).click();
  await page.getByRole('button', { name: 'Kaydet' }).click();
  await expect(page.getByRole('status')).toContainText('Kaydedildi');
  await expect(h1(page, 'Ofis')).toBeVisible();

  await page.getByRole('tab', { name: 'İmzalar' }).click();
  await page.getByRole('button', { name: 'İmza ekle' }).click();
  await page.getByLabel('İmza 3 imzasının metni').fill('Furkan YILDIRIM\nMailhere');
  await page.getByLabel('Yeni postalarda').selectOption({ label: 'İmza 3' });
  await page.getByRole('button', { name: 'Kaydet' }).click();
  await expect(page.getByRole('status')).toContainText('Kaydedildi');
  const acc = await page.evaluate(() => window.__mh.state.accounts[0]);
  expect(acc.name).toBe('Ofis');
  expect(acc.color).toBe('#9A5BD6');
  expect(acc.signatures).toHaveLength(3);

  // Uzak sekme kaydırılarak görünür; sunucu bilgisi tek sütunda.
  await page.getByRole('tab', { name: 'Sunucu ve parola' }).click();
  await expect(page.getByRole('tab', { name: 'Sunucu ve parola' })).toBeInViewport();
  await expect(page.getByLabel('Sunucu', { exact: true }).first()).toHaveValue('imap.ornek.com.tr');

  // Geri: hesap listesine, oradan gelen kutusuna.
  await page.goBack();
  await expect(h1(page, 'Ayarlar')).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: /^Ofis/ })).toBeVisible();
  await page.goBack();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('kurallar: yeni kural tek sütunda yazılır ve kaydedilir', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page, 'Kurallar');
  await expect(page.getByText('Faturalar klasöre')).toBeVisible();
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  await page.getByLabel('Kural adı').fill('GitHub okundu');
  await page.getByLabel('Değer').fill('github.com');
  await page.getByLabel('Eylem 1').selectOption({ label: 'Okundu say' });
  // Tür seçicisi ve değer alanı ayrı satırlarda (koşul iki satıra iner).
  const kind = await page.getByLabel('Koşul 1').boundingBox();
  const value = await page.getByLabel('Değer').boundingBox();
  expect(value!.y).toBeGreaterThan(kind!.y + kind!.height - 1);
  await page.getByRole('button', { name: 'Kuralı kaydet' }).click();
  await expect(page.getByRole('status')).toContainText('Kural oluşturuldu');
  await expect(page.getByText('GitHub okundu')).toBeVisible();
  await expect(page.getByText('Kimden içerir “github.com” → Okundu say')).toBeVisible();
  const saved = await page.evaluate(() => window.__mh.state.rules.some((r) => r.name === 'GitHub okundu'));
  expect(saved).toBe(true);
});

test('hesap ekleme: adres → ayarlar → parola → hazır (mobil, tam ekran)', async ({ page }) => {
  await page.goto('/?kabuk=mobil&bos#/main');
  await expect(page.getByRole('heading', { name: 'Hesap ekle' })).toBeVisible();
  // Marka paneli ve kenar çubuğu yok; alan ve düğmeler 44 px.
  await expect(page.getByText('Postaların tek yerde')).toBeHidden();
  await expectTouchHeight(page.getByLabel('E-posta adresi'));
  await expectTouchHeight(page.getByRole('button', { name: 'Google hesabı' }));
  await page.getByLabel('E-posta adresi').fill('furkan@ornek.com.tr');
  await page.getByRole('button', { name: 'Devam' }).click();
  await expect(page.getByRole('heading', { name: 'Ayarlar bulundu' })).toBeVisible();
  await expect(page.getByText('imap.ornek.com.tr · 993')).toBeVisible();
  await expect(page.getByText('Android anahtar deposunda şifreli saklanır')).toBeVisible();
  await page.getByLabel('Parola', { exact: true }).fill('yanlis');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(page.getByRole('alert')).toContainText('parola');
  await page.getByLabel('Parola', { exact: true }).fill('dogru-parola');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(page.getByRole('heading', { name: 'Hazırsın' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Menü', exact: true })).toHaveCount(0);
  await expect(page.getByText('Postalar indirildi')).toBeVisible();
  await page.getByRole('button', { name: 'Gelen kutusunu aç' }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Yeni posta' })).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});

test('ikinci hesap: çekmeceden ve ayarlardan açılır, kapatılınca listeye dönülür', async ({ page }) => {
  await page.goto('#/main');
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Hesap ekle' }).click();
  await expect(page.getByRole('heading', { name: 'Hesap ekle' })).toBeVisible();
  await page.getByRole('button', { name: 'Kapat', exact: true }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();

  await openSettings(page);
  await page.getByRole('navigation', { name: 'Ayar bölümleri' }).getByRole('button', { name: 'Hesap ekle', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Hesap ekle' })).toBeVisible();
  await page.getByRole('button', { name: 'Kapat', exact: true }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('planlananlar: çekmecede yalnız bekleyen varsa, ekranda iptal edilir', async ({ page }) => {
  await page.goto('#/main');
  await openDrawer(page);
  await expect(drawer(page).getByRole('button', { name: /Planlananlar/ })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(drawer(page)).toBeHidden();

  await page.evaluate(() =>
    window.__mh.call('send', {
      msg: {
        accountId: 'demo-is', to: [{ name: 'Kerem', email: 'kerem.aydin@ornek.com.tr' }], cc: [], bcc: [],
        subject: 'Pazartesi toplantısı', bodyText: 'Gündem ekte.', inReplyTo: null, attachments: [], sendAt: Date.now() + 86_400_000,
      },
    }),
  );
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Planlananlar 1' }).click();
  await expect(drawer(page)).toBeHidden();
  await expect(h1(page, 'Planlananlar')).toBeVisible();
  await expect(page.getByText('Pazartesi toplantısı')).toBeVisible();
  await expectTouchHeight(page.getByRole('button', { name: 'Şimdi gönder' }));
  await page.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(page.getByRole('status')).toContainText('iptal edildi');
  await expect(page.getByText('Planlanmış posta yok')).toBeVisible();
  await page.getByRole('button', { name: 'Geri', exact: true }).click();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('açılışta başlat: Genel’de varsayılan açık, telefon açıklaması; kapatılıp açılır, 44 px dokunma alanı', async ({ page }) => {
  await page.goto('#/main');
  await openSettings(page, 'Genel');
  const sw = page.getByRole('switch', { name: 'Açılışta başlat' });
  const row = page.locator('.set-row', { has: sw });
  await expect(page.getByRole('heading', { level: 3, name: 'Başlangıç' })).toBeVisible();
  await expect(sw).toBeChecked();
  await expect(row).toContainText('Telefon açılınca Mailhere arka planda başlar ve postaları denetler.');
  await expect(row).not.toContainText('tepside');
  // Anahtar 40×24; dokunma alanı görünmez kenarla 44 px'e çıkar.
  const hit = await sw.evaluate((el) => {
    const before = getComputedStyle(el, '::before');
    return el.getBoundingClientRect().height - parseFloat(before.top) - parseFloat(before.bottom);
  });
  expect(hit).toBeGreaterThanOrEqual(44);
  await sw.click();
  await expect(sw).not.toBeChecked();
  expect(await page.evaluate(() => window.__mh.call('autostart_get'))).toBe(false);
  await sw.click();
  await expect(sw).toBeChecked();
  // Anahtar sağ kenarda, sayfa yatay taşmaz.
  const box = await sw.boundingBox();
  expect(box!.x + box!.width).toBeGreaterThan(page.viewportSize()!.width - 40);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
});
