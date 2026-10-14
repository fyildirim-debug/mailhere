import { expect, test, type Locator, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
    /** Test: çerçevedeki `window.print` yerine basılan belgenin başlığını yazar. */
    __printed?: string;
  }
}

// Okuma özellikleri: konuşma görünümü, kişi kartı, yazdırma / .eml, takvim daveti,
// koyu temada posta eşleme ve çerçevenin yükleyemediği uzak görseller.

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

/** Hesaplanmış renk → [r, g, b]. */
const rgb = (s: string) => (s.match(/\d+/g) ?? []).slice(0, 3).map(Number);
/** Göreli parlaklık (WCAG). */
const lum = ([r, g, b]: number[]) => {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
};
const bgOf = async (l: Locator) => rgb(await l.evaluate((el) => getComputedStyle(el).backgroundColor));
const colorOf = async (l: Locator) => rgb(await l.evaluate((el) => getComputedStyle(el).color));

test('koyu temada açık zeminli bülten koyuya eşlenir; “Özgün renkler” kendi renklerine döner', async ({ page }) => {
  await page.goto('/#/main');
  await page.getByRole('button', { name: 'Temayı değiştir' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await list(page).getByText('Renk, ışık ve odak').click();
  const frame = page.frameLocator('iframe[title="posta"]');
  const outer = frame.locator('table').first();
  const card = frame.locator('table').nth(1);
  const header = frame.locator('td[bgcolor="#1b1d24"]');
  const body = frame.locator('p').first();
  const button = frame.getByRole('link', { name: 'Sayıyı oku' });
  const toggle = reader(page).getByRole('button', { name: 'Özgün renkler' });
  await expect(frame.locator('html')).toHaveClass(/mh-dark/);
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');

  // Açık gri dış zemin ve beyaz kart koyulaştı; kağıt uygulamanın kağıdı.
  expect(lum(await bgOf(outer))).toBeLessThan(0.05);
  expect(lum(await bgOf(card))).toBeLessThan(0.05);
  expect(await bgOf(frame.locator('body'))).toEqual([25, 28, 34]);
  // Koyu başlık bandı, üstündeki beyaz yazı ve vurgu düğmesi olduğu gibi.
  expect(await bgOf(header)).toEqual([27, 29, 36]);
  expect(await colorOf(header)).toEqual([255, 255, 255]);
  expect(await bgOf(button)).toEqual([70, 85, 212]);
  expect(await colorOf(button)).toEqual([255, 255, 255]);
  // Koyu gövde yazısı uygulamanın mürekkebi; gri alt bilgi ikincil mürekkep.
  expect(await colorOf(body)).toEqual([230, 232, 237]);
  expect(await colorOf(frame.getByText('listeye üye olduğun için aldın', { exact: false }))).toEqual([174, 180, 191]);

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(frame.locator('html')).not.toHaveClass(/mh-dark/);
  expect(await bgOf(outer)).toEqual([244, 244, 246]);
  expect(await bgOf(card)).toEqual([255, 255, 255]);
  expect(await bgOf(frame.locator('body'))).toEqual([255, 255, 255]);
  expect(await colorOf(body)).toEqual([51, 51, 51]);
  expect(await bgOf(header)).toEqual([27, 29, 36]);

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expect(frame.locator('html')).toHaveClass(/mh-dark/);
  expect(lum(await bgOf(outer))).toBeLessThan(0.05);

  // Kendi zemini olmayan HTML yanıt da aynı yoldan: koyu kağıt, açık yazı, alıntı ikincil.
  await list(page).getByText('Re: Kitap kulübü').click();
  await expect(frame.locator('html')).toHaveClass(/mh-dark/);
  expect(await bgOf(frame.locator('body'))).toEqual([25, 28, 34]);
  expect(await colorOf(frame.getByText('Pazar akşamına kadar listeyi gönderirim.'))).toEqual([230, 232, 237]);
  expect(await colorOf(frame.getByText('Bu ayın kitabını kim seçiyor?'))).toEqual([174, 180, 191]);

  // Açık temada geçiş yok, posta kendi renklerinde.
  await page.getByRole('button', { name: 'Temayı değiştir' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect(toggle).toHaveCount(0);
  await expect(frame.locator('html')).not.toHaveClass(/mh-dark/);
});

test('izinli uzak görsel çerçevede yüklenemezse arka uçtan indirilip gösterilir', async ({ page }) => {
  // `Cross-Origin-Resource-Policy: same-origin` ile gelen görsel gibi: çerçeve yükleyemez.
  await page.route(/^https:\/\/(izleme\.)?tasarimnotlari\.co\//, (route) => route.abort());
  await page.goto('/#/main');
  await list(page).getByText('Renk, ışık ve odak').click();
  const frame = page.frameLocator('iframe[title="posta"]');
  const cover = frame.getByRole('img', { name: 'Sayı 18 kapak' });
  // İzin yokken görsel istenmez (kaynak yok).
  await expect(cover).not.toHaveAttribute('src', /./);
  await page.getByRole('button', { name: 'Bu göndericiye izin ver' }).click();
  await expect(cover).toHaveAttribute('src', /^data:image\/svg\+xml;base64,/);
  await expect.poll(() => cover.evaluate((el) => (el as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
  await expect(frame.locator('img[width="1"]')).toHaveAttribute('src', /^data:image\/png;base64,/);
  // Koyu temaya geçiş çerçeveyi yeniler; görseller yeniden istenmeden önbellekten gelir.
  await page.getByRole('button', { name: 'Temayı değiştir' }).click();
  await expect(frame.locator('html')).toHaveClass(/mh-dark/);
  await expect(cover).toHaveAttribute('src', /^data:image\/svg\+xml;base64,/);
  // Sahte arka uç: izinsiz gönderici ve postada geçmeyen adres reddedilir.
  const errors = await page.evaluate(async () => {
    const out: string[] = [];
    const idOf = (subject: string) => window.__mh.state.messages.find((m) => m.subject === subject)!.id;
    for (const [id, url] of [
      [idOf('Sayı 142 · Sakin arayüzler üzerine'), 'https://mimarhaftalik.com/kapak-142.jpg'],
      [idOf('Renk, ışık ve odak'), 'https://baska.example/x.png'],
    ] as const) {
      try {
        await window.__mh.call('remote_images', { id, urls: [url] });
        out.push('ok');
      } catch (e) {
        out.push((e as { code: string }).code);
      }
    }
    return out;
  });
  expect(errors).toEqual(['invalid', 'invalid']);
});
