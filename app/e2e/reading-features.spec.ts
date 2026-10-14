import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
    /** Test: çerçevedeki `window.print` yerine basılan belgenin başlığını yazar. */
    __printed?: string;
  }
}

// Okuma özellikleri: konuşma görünümü, kişi kartı, yazdırma / .eml, takvim daveti.

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const reader = (page: Page) => page.getByRole('main');
const collapsed = (page: Page) => reader(page).getByRole('button', { name: /iletisini aç$/ });

test('konuşma görünümü: eskiler katlı, en yeni açık; tek tek ve toptan açılır', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Re: Yayın planı').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Re: Yayın planı' })).toBeVisible();
  await expect(reader(page).getByText('4 ileti')).toBeVisible();
  // En yeni ileti açık, üç eskisi katlı; kendi iletimiz "Sen" olarak görünür.
  await expect(reader(page).getByText('Duyuru metnini pazartesi gönderirim.', { exact: false })).toBeVisible();
  await expect(collapsed(page)).toHaveCount(3);
  await expect(reader(page).getByRole('button', { name: 'Sen iletisini aç' })).toBeVisible();

  await reader(page).getByRole('button', { name: 'Kerem Aydın iletisini aç' }).click();
  await expect(collapsed(page)).toHaveCount(2);
  await expect(reader(page).getByText('Sürüm notlarının taslağını ekledim', { exact: false })).toBeVisible();
  await expect(reader(page).getByRole('button', { name: 'Surum_notlari_0_2.pdf' })).toBeVisible();

  await reader(page).getByRole('button', { name: 'Tümünü aç' }).click();
  await expect(collapsed(page)).toHaveCount(0);
  await expect(reader(page).getByRole('button', { name: /iletisini kapat$/ })).toHaveCount(4);
  await reader(page).getByRole('button', { name: 'Tümünü kapat' }).click();
  await expect(collapsed(page)).toHaveCount(3, { timeout: 5_000 });
  await expect(reader(page).getByText('Duyuru metnini pazartesi gönderirim.', { exact: false })).toBeVisible();
});

test('ayar kapalıysa yalnız seçili ileti ve öncekilerin özeti görünür', async ({ page }) => {
  await page.goto('/#/main');
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  const sw = page.getByRole('switch', { name: 'Konuşmaları birlikte göster' });
  await expect(sw).toBeChecked();
  await sw.uncheck();
  await page.getByRole('button', { name: 'Ayarları kapat' }).click();
  await list(page).getByText('Re: Yayın planı').click();
  await expect(reader(page).getByText('4 ileti · murat.kaya@ornek.com.tr')).toBeVisible();
  await expect(collapsed(page)).toHaveCount(0);
  await expect(reader(page).getByRole('button', { name: 'Tümünü aç' })).toHaveCount(0);
  const saved = await page.evaluate(() => window.__mh.state.settings.conversationView);
  expect(saved).toBe(false);
});

test('kişi kartı: özet, son konuşmalar, önemli kişi, postalarını göster ve yeni posta', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await reader(page).getByRole('button', { name: 'Ayşe Demir kişi kartını aç' }).click();
  const card = page.getByRole('dialog', { name: 'Kişi kartı' });
  await expect(card).toBeVisible();
  await expect(card.getByText('ayse.demir@eposta.com')).toBeVisible();
  // Gönderdiği iki posta + alıcısı olduğu bir posta.
  await expect(card.getByText('3 posta', { exact: false })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Re: Q4 bütçe taslağı' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Re: Kitap kulübü' })).toBeVisible();

  await card.getByRole('button', { name: 'Önemli kişi yap' }).click();
  await expect(page.getByRole('status')).toContainText('önemli kişilere eklendi');
  await expect(card.getByText('Önemli kişi', { exact: true })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Önemli kişiden çıkar' })).toBeVisible();
  const vip = await page.evaluate(() => window.__mh.state.settings.vip);
  expect(vip).toContain('ayse.demir@eposta.com');

  // Yeni posta: alıcı dolu bir yazma penceresi açılır.
  const popup = page.waitForEvent('popup');
  await card.getByRole('button', { name: 'Yeni posta yaz' }).click();
  const compose = await popup;
  await expect(compose.getByRole('button', { name: 'Ayşe Demir alıcısını kaldır' })).toBeVisible();
  // Kartın açıldığı postanın hesabından (Kişisel) gönderilir.
  await expect(compose.locator('#compose-from')).toHaveValue('demo-kisisel');
  await compose.close();
  await expect(card).toHaveCount(0);

  // Postalarını göster: arama görünümü.
  await reader(page).getByRole('button', { name: 'Ayşe Demir kişi kartını aç' }).click();
  await card.getByRole('button', { name: 'Postalarını göster' }).click();
  await expect(list(page).getByRole('heading', { name: /from:ayse\.demir@eposta\.com/ })).toBeVisible();
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  // Esc kartı kapatır.
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await reader(page).getByRole('button', { name: 'Ayşe Demir kişi kartını aç' }).click();
  await expect(card).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(card).toHaveCount(0);
});

test('takvim daveti: kart, yanıt ve takvime ekleme', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Davet: Sprint planlama').click();
  const card = reader(page).getByRole('region', { name: 'Takvim daveti' });
  await expect(card).toBeVisible();
  await expect(card.getByRole('heading', { name: 'Sprint planlama' })).toBeVisible();
  await expect(card.getByText('Toplantı odası 2')).toBeVisible();
  await expect(card.getByText('Düzenleyen: Kerem Aydın')).toBeVisible();
  await expect(card.getByText('3 katılımcı')).toBeVisible();
  await expect(card.getByText('kerem.aydin@ornek.com.tr adresine gönderilir', { exact: false })).toBeVisible();

  await card.getByRole('button', { name: 'Katılıyorum' }).click();
  await expect(page.getByRole('status')).toContainText('“Katılıyorum” yanıtı gönderildi');
  await expect(card.getByRole('button', { name: 'Katılıyorum' })).toHaveAttribute('aria-pressed', 'true');
  const sent = await page.evaluate(() => window.__mh.state.messages.some((m) => m.subject === 'Kabul edildi: Sprint planlama' && m.to[0]?.email === 'kerem.aydin@ornek.com.tr'));
  expect(sent).toBe(true);

  await card.getByRole('button', { name: 'Takvime ekle' }).click();
  await expect(page.getByRole('status')).toContainText('takvim uygulamasıyla açıldı');
});

test('yazdırma yalıtılmış çerçevede yapılır; .eml kaydedilir', async ({ page }) => {
  // Çerçevedeki yazdırma çağrısı başlığı kaydeder (gerçek pencere açılmaz).
  await page.addInitScript(() => {
    window.print = () => {
      window.top!.__printed = document.title;
    };
  });
  await page.goto('/#/main');
  await list(page).getByText('Davet: Sprint planlama').click();
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'Yazdır' }).click();
  await expect.poll(() => page.evaluate(() => window.__printed)).toBe('Davet: Sprint planlama');
  const frame = page.frameLocator('iframe[title="yazdır"]');
  await expect(frame.getByRole('heading', { level: 1, name: 'Davet: Sprint planlama' })).toBeAttached();
  await expect(frame.getByText('Kerem Aydın <kerem.aydin@ornek.com.tr>')).toBeAttached();
  await expect(frame.getByText('davet.ics', { exact: false })).toBeAttached();
  // Uygulama arayüzü çerçevede yok.
  await expect(frame.getByRole('button')).toHaveCount(0);

  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'Farklı kaydet (.eml)' }).click();
  await expect(page.getByRole('status')).toContainText('Posta kaydedildi · /tmp/mailhere/Davet_ Sprint planlama.eml');
});
