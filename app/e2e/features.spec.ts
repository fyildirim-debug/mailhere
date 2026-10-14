import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Klasör ağacı, ayrıntılı hesap ayarları, kurallar, planlananlar, imzalar,
// şablonlar, süzgeç, abonelik, otomatik yanıt, hızlı yanıtlar, koyu posta.

const side = (page: Page) => page.getByRole('navigation', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });

async function openSettings(page: Page, section: string) {
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: section, exact: true }).click();
}

test('kenar çubuğu: hesabın klasör ağacı açılır, alt klasör seçilir, klasör gizlenip geri getirilir', async ({ page }) => {
  await page.goto('/#/main');
  await side(page).getByRole('button', { name: 'İş klasörlerini göster' }).click();
  const tree = page.getByRole('group', { name: 'İş klasörleri' });
  await expect(tree.getByRole('button', { name: 'Projeler', exact: true })).toBeVisible();
  await tree.getByRole('button', { name: 'Mailhere 1' }).click();
  await expect(list(page).getByRole('heading', { name: 'Mailhere · İş' })).toBeVisible();
  await expect(list(page).getByText('Yol haritası notları')).toBeVisible();

  // Alt klasörler kapanır.
  await tree.getByRole('button', { name: 'Projeler klasörlerini kapat' }).click();
  await expect(tree.getByRole('button', { name: 'Mailhere 1' })).toHaveCount(0);

  // Sağ tık menüsünden gizle; Ayarlar › Klasörler'den geri getir.
  await tree.getByRole('button', { name: 'Hukuk' }).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Kenar çubuğunda gizle' }).click();
  await expect(tree.getByRole('button', { name: 'Hukuk' })).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('Hukuk gizlendi');

  await side(page).getByRole('button', { name: 'İş klasörlerini düzenle' }).click();
  await page.getByRole('tab', { name: 'Klasörler' }).click();
  const show = page.getByRole('switch', { name: 'Hukuk kenar çubuğunda görünsün' });
  await expect(show).not.toBeChecked();
  await show.check();
  await expect(side(page).getByRole('button', { name: 'Hukuk' })).toBeVisible();
  // Gelen kutusu gizlenemez.
  await expect(page.getByRole('switch', { name: 'Gelen Kutusu kenar çubuğunda görünsün' })).toBeDisabled();
});

test('hesap ayarları: özel klasör görevi değişir, sunucu değişince parola istenir', async ({ page }) => {
  await page.goto('/#/main');
  await openSettings(page, 'İş');
  await page.getByRole('tab', { name: 'Klasörler' }).click();
  await page.locator('#role-archive').selectOption({ label: 'Hukuk' });
  await expect(page.getByRole('status')).toContainText('Klasör görevi değişti');
  const role = await page.evaluate(() => window.__mh.state.mailboxes.find((b) => b.id === 'demo-is:Hukuk')?.role);
  expect(role).toBe('archive');
  await page.getByRole('button', { name: 'Sunucunun bildirdiğine dön' }).click();
  await expect.poll(() => page.evaluate(() => window.__mh.state.mailboxes.find((b) => b.id === 'demo-is:Hukuk')?.role)).toBe('other');

  await page.getByRole('tab', { name: 'Sunucu ve parola' }).click();
  await page.getByRole('textbox', { name: 'Sunucu', exact: true }).first().fill('imap.yeni-ornek.com.tr');
  const save = page.getByRole('button', { name: 'Dene ve kaydet' });
  await expect(save).toBeDisabled();
  await page.locator('#srv-pass').fill('dogru-parola');
  await save.click();
  await expect(page.getByRole('status')).toContainText('Sunucu ayarları kaydedildi');
  const host = await page.evaluate(() => window.__mh.state.accounts[0].imap?.host);
  expect(host).toBe('imap.yeni-ornek.com.tr');

  await page.getByLabel('Yeni parola').fill('yanlis');
  await page.getByRole('button', { name: 'Parolayı güncelle' }).click();
  await expect(page.getByRole('alert')).toContainText('parola');
});

test('otomatik yanıt: JMAP hesabında ayarlanır, IMAP hesabında desteklenmediği söylenir', async ({ page }) => {
  await page.goto('/#/main');
  await openSettings(page, 'İş');
  await page.getByRole('tab', { name: 'Otomatik yanıt' }).click();
  await expect(page.getByText('uzaktan otomatik yanıtı desteklemiyor')).toBeVisible();

  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Sunucu', exact: true }).click();
  await page.getByRole('tab', { name: 'Otomatik yanıt' }).click();
  await page.getByRole('switch', { name: 'Otomatik yanıt açık' }).check();
  await page.getByLabel('Konu').fill('İzindeyim');
  await page.getByLabel('Yanıt metni').fill('20 Ekim’e kadar izindeyim.');
  await page.getByRole('button', { name: 'Kaydet' }).click();
  await expect(page.getByRole('status')).toContainText('Otomatik yanıt açıldı');
});

test('kurallar: yeni kural yazılır ve gelen kutusuna uygulanır; göndericiyi engelleme kural oluşturur', async ({ page }) => {
  await page.goto('/#/main');
  await openSettings(page, 'Kurallar');
  await expect(page.getByText('Faturalar klasöre')).toBeVisible();
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  await page.getByLabel('Kural adı').fill('GitHub okundu');
  await page.getByLabel('Değer').fill('github.com');
  await page.getByLabel('Eylem 1').selectOption({ label: 'Okundu say' });
  await page.getByRole('button', { name: 'Kuralı kaydet' }).click();
  await expect(page.getByText('GitHub okundu')).toBeVisible();
  await expect(page.getByText('Kimden içerir “github.com” → Okundu say')).toBeVisible();
  const n = await page.evaluate(async () => {
    const r = window.__mh.state.rules.find((x) => x.name === 'GitHub okundu')!;
    return window.__mh.call('rule_run', { id: r.id });
  });
  expect(n).toBe(1);

  // Klasöre taşıma hesap ister.
  await page.getByRole('button', { name: 'Yeni kural' }).click();
  await page.getByRole('combobox', { name: /^Hesap/ }).selectOption({ label: 'Tüm hesaplar' });
  await page.getByLabel('Eylem 1').selectOption({ label: 'Klasöre taşı' });
  await expect(page.getByText('Klasöre taşımak için yukarıdan bir hesap seç.')).toBeVisible();
  await page.getByRole('button', { name: 'Vazgeç' }).click();

  // Okuma panelinden göndericiyi engelle.
  await page.getByRole('button', { name: 'Ayarları kapat' }).click();
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'ayse.demir@eposta.com adresini engelle' }).click();
  await expect(page.getByRole('status')).toContainText('engellendi');
  const blocked = await page.evaluate(() => window.__mh.state.rules.some((r) => r.name === 'Engellendi: ayse.demir@eposta.com'));
  expect(blocked).toBe(true);
});

test('planlananlar: sonra gönderilen posta listelenir, iptal edilir', async ({ page }) => {
  await page.goto('/#/main');
  await page.evaluate(() =>
    window.__mh.call('send', {
      msg: {
        accountId: 'demo-is', to: [{ name: 'Kerem', email: 'kerem.aydin@ornek.com.tr' }], cc: [], bcc: [],
        subject: 'Pazartesi toplantısı', bodyText: 'Gündem ekte.', inReplyTo: null, attachments: [], sendAt: Date.now() + 86_400_000,
      },
    }),
  );
  await side(page).getByRole('button', { name: 'Planlananlar 1' }).click();
  await expect(page.getByRole('heading', { name: 'Planlananlar' })).toBeVisible();
  await expect(page.getByText('Pazartesi toplantısı')).toBeVisible();
  await page.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(page.getByText('Planlanmış posta yok')).toBeVisible();
});

test('liste süzgeci ve klasöre taşıma', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByRole('button', { name: 'Süzgeç' }).click();
  await page.getByRole('menuitemcheckbox', { name: 'Yıldızlı' }).click();
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toBeVisible();
  await expect(list(page).getByText('Gönderiniz dağıtıma çıktı', { exact: true })).toHaveCount(0);
  await list(page).getByRole('button', { name: 'Süzgeci kaldır' }).click();
  await expect(list(page).getByText('Gönderiniz dağıtıma çıktı', { exact: true })).toBeVisible();

  await list(page).getByText('Sözleşme imzaya hazır').click();
  await page.getByRole('button', { name: 'Klasöre taşı' }).click();
  await page.getByRole('menuitem', { name: 'Hukuk' }).click();
  await expect(page.getByRole('status')).toContainText('Hukuk klasörüne taşındı');
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toHaveCount(0);
});

test('bülten: abonelikten tek tıkla çıkılır', async ({ page }) => {
  await page.goto('/#/main');
  await list(page).getByText('Sayı 142 · Sakin arayüzler üzerine').click();
  await expect(page.getByText('Bu bir toplu gönderim.')).toBeVisible();
  await page.getByRole('button', { name: 'Abonelikten çık' }).click();
  await expect(page.getByRole('status')).toContainText('aboneliğinden çıkıldı');
});

test('arama işleçleri: from: ve has:ek', async ({ page }) => {
  await page.goto('/#/main');
  await page.keyboard.press('Control+k');
  await expect(page.getByRole('option', { name: /has:ek/ })).toBeVisible();
  await page.getByRole('combobox').fill('from:elif has:ek');
  await page.keyboard.press('Enter');
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toBeVisible();
  await expect(list(page).getByText('Ekim faturanız hazır')).toHaveCount(0);
});

test('yazma: imza seçilir, şablon eklenir', async ({ page }) => {
  await page.goto('/#/main');
  await page.evaluate(() =>
    window.__mh.call('settings_set', {
      settings: { ...window.__mh.state.settings, templates: [{ id: '', name: 'Teşekkür', subject: 'Teşekkürler', body: 'Desteğin için çok teşekkürler.' }] },
    }),
  );
  await page.goto('/#/compose/new/0');
  await expect(page.getByRole('button', { name: 'İmza: Tam' })).toBeVisible();
  await expect(page.getByText('Ürün Geliştirme · Örnek A.Ş.')).toBeVisible();
  await page.getByRole('button', { name: 'İmza: Tam' }).click();
  await page.getByRole('menuitemcheckbox', { name: 'Kısa' }).click();
  await expect(page.getByText('Ürün Geliştirme · Örnek A.Ş.')).toHaveCount(0);

  await page.getByRole('button', { name: 'Şablon ekle' }).click();
  await page.getByRole('menuitem', { name: 'Teşekkür' }).click();
  await expect(page.getByLabel('Konu')).toHaveValue('Teşekkürler');
  await expect(page.getByRole('textbox', { name: 'Posta metni' })).toHaveValue(/Desteğin için çok teşekkürler/);
});

test('hızlı okuma: hızlı yanıtlar düzenlenir', async ({ page }) => {
  await page.goto('/#/quick/4');
  await expect(page.getByRole('button', { name: 'Teşekkürler!' })).toBeVisible();
  await page.getByRole('button', { name: 'Hızlı yanıtları düzenle' }).click();
  await page.getByRole('textbox', { name: 'Yeni hızlı yanıt' }).fill('Yarın ararım.');
  await page.keyboard.press('Enter');
  await page.getByRole('button', { name: '"Bakıp dönüyorum." yanıtını sil' }).click();
  await page.getByRole('button', { name: 'Bitti' }).click();
  await expect(page.getByRole('button', { name: 'Yarın ararım.' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Bakıp dönüyorum.' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Yarın ararım.' }).click();
  await expect(page.getByRole('textbox', { name: 'Yanıtla' })).toHaveValue('Yarın ararım.');
  const saved = await page.evaluate(() => window.__mh.state.settings.quickReplies);
  expect(saved).toEqual(['Teşekkürler!', 'Tamam, uygun.', 'Yarın ararım.']);
});

test('koyu tema (sistem): zeminsiz posta da bülten de koyuya eşlenir; ayar kapalıysa posta kendi renklerinde', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/#/main');
  await list(page).getByText('Re: Kitap kulübü').click();
  const frame = page.frameLocator('iframe[title="posta"]');
  await expect(frame.getByText('Pazar akşamına kadar listeyi gönderirim.')).toBeVisible();
  const bg = await frame.locator('body').evaluate((b) => getComputedStyle(b).backgroundColor);
  expect(bg).not.toBe('rgb(255, 255, 255)');
  // Siyah yazılmış satır okunur hale gelir.
  const color = await frame.getByText('Pazar akşamına kadar listeyi gönderirim.').evaluate((el) => getComputedStyle(el).color);
  expect(color).not.toBe('rgb(0, 0, 0)');

  // Kendi zemini (kapak yer tutucusu) olan bülten de koyu kağıtta; "Özgün renkler" beyaz kağıda döndürür.
  await list(page).getByText('Sayı 142 · Sakin arayüzler üzerine').click();
  const news = page.frameLocator('iframe[title="posta"]');
  await expect(news.getByText('Bu hafta üç yazı seçtik.')).toBeVisible();
  await expect(news.locator('html')).toHaveClass(/mh-dark/);
  const paper = () => news.locator('body').evaluate((b) => getComputedStyle(b).backgroundColor);
  expect(await paper()).toBe('rgb(25, 28, 34)');
  await page.getByRole('button', { name: 'Özgün renkler' }).click();
  await expect(news.locator('html')).not.toHaveClass(/mh-dark/);
  expect(await paper()).toBe('rgb(255, 255, 255)');

  // Ayar kapalı: koyu temada da posta kendi renklerinde, geçiş düğmesi yok.
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('switch', { name: 'Postaları koyu temaya uyarla' }).uncheck();
  await page.getByRole('button', { name: 'Ayarları kapat' }).click();
  await list(page).getByText('Re: Kitap kulübü').click();
  await expect(frame.getByText('Pazar akşamına kadar listeyi gönderirim.')).toBeVisible();
  await expect(frame.locator('html')).not.toHaveClass(/mh-dark/);
  expect(await frame.locator('body').evaluate((b) => getComputedStyle(b).backgroundColor)).toBe('rgb(255, 255, 255)');
  await expect(page.getByRole('button', { name: 'Özgün renkler' })).toHaveCount(0);
});
