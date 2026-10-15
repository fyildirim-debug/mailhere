import { expect, test, type Page } from '@playwright/test';

// Güven ve gizlilik (masaüstü): izleyici şeridi ve satırı, gönderen uyarısı,
// aldatıcı bağlantı onayı, doğrulama kodu ve bildirim kartında "Kodu kopyala".
// Örnek postalar arşivdedir (sahte arka uç, demo-data.ts).

const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const reader = (page: Page) => page.getByRole('main');

async function toInbox(page: Page) {
  await page.getByRole('navigation', { name: 'Kutular ve hesaplar' }).getByRole('button', { name: /^Gelen Kutusu/ }).first().click();
}

async function openArchived(page: Page, subject: string) {
  await page.goto('/#/main');
  await page.getByRole('navigation', { name: 'Kutular ve hesaplar' }).getByRole('button', { name: /^Arşiv/ }).first().click();
  await list(page).getByText(subject, { exact: true }).click();
  await expect(page.getByRole('heading', { level: 2, name: subject })).toBeVisible();
}

test('uzak içerik şeridi izleyici sayısını gösterir; metne tıklayınca açıklama açılır', async ({ page }) => {
  await openArchived(page, 'Hafta sonu için beş rota');
  const strip = page.locator('.remote');
  await expect(strip).toContainText('3 izleyici engellendi. Uzak görseller de yüklenmedi.');
  const toggle = strip.getByRole('button', { name: /3 izleyici engellendi/ });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  // Şerit tek satır.
  expect((await strip.boundingBox())!.height).toBeLessThanOrEqual(44);
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(reader(page).getByText(/görünmez bir görseldir.*4 bağlantıdan izleme parametresi silindi\./)).toBeVisible();
  await toggle.click();
  await expect(reader(page).getByText(/görünmez bir görseldir/)).toHaveCount(0);
  // İzleyicisiz bültende eski metin değişmez.
  await toInbox(page);
  await list(page).getByText('Mimar Haftalık').click();
  await expect(page.getByText('Uzak görseller ve izleme pikselleri engellendi.', { exact: false })).toBeVisible();
  await expect(page.locator('.remote')).not.toContainText('izleyici engellendi');
});

test('uzak görseli olmayan postada izleyici satırı: 12 px, ikincil renk, açıklama açılır', async ({ page }) => {
  await openArchived(page, 'Haftalık özet: üç görev tamamlandı');
  await expect(page.locator('.remote')).toHaveCount(0);
  const line = reader(page).getByRole('button', { name: '2 izleyici engellendi' });
  await expect(line).toBeVisible();
  expect(await line.evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px');
  const ink3 = await page.evaluate(() => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--ink-3)';
    document.body.appendChild(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  });
  expect(await line.evaluate((el) => getComputedStyle(el).color)).toBe(ink3);
  await line.click();
  await expect(reader(page).getByText(/1 bağlantıdan izleme parametresi silindi\./)).toBeVisible();
});

test('sahte gönderici: amber uyarı tek satır; dar okuma alanında kısa metin', async ({ page }) => {
  await openArchived(page, 'Hesabınız 24 saat içinde askıya alınacak');
  const warn = reader(page).getByText('Görünen addaki adres gerçek gönderen değil. Bağlantılara tıklamadan önce adresi denetle.');
  await expect(warn).toBeVisible();
  const row = page.locator('.trust-row');
  expect((await row.boundingBox())!.height).toBeLessThanOrEqual(22);
  // Simge amber; renkli sol kenar çizgisi yok.
  const amber = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--amber').trim());
  const iconColor = await row.locator('svg').evaluate((el) => getComputedStyle(el).color);
  const hex = (rgb: string) => '#' + rgb.match(/\d+/g)!.slice(0, 3).map((n) => Number(n).toString(16).padStart(2, '0')).join('');
  expect(hex(iconColor)).toBe(amber.toLowerCase());
  expect(await row.evaluate((el) => getComputedStyle(el).borderLeftWidth)).toBe('0px');

  await page.setViewportSize({ width: 1000, height: 800 });
  await expect(reader(page).getByText('Gönderen şüpheli; tıklamadan önce adresi denetle.')).toBeVisible();
  await expect(warn).toBeHidden();

  // İzleyicisi, kodu ve uyarısı olmayan postada bu satırlar yok.
  await page.setViewportSize({ width: 1440, height: 900 });
  await toInbox(page);
  await list(page).getByText('Cumartesi kahvaltı?').click();
  await expect(page.locator('.trust, .otp, .trackers')).toHaveCount(0);
});

test('aldatıcı bağlantı tıklamada onay ister; vazgeç açmaz, "Yine de aç" açar', async ({ page }) => {
  await openArchived(page, 'Hesabınız 24 saat içinde askıya alınacak');
  await page.evaluate(() => {
    (window as unknown as { __opened: string[] }).__opened = [];
    window.open = ((url: string) => {
      (window as unknown as { __opened: string[] }).__opened.push(url);
      return null;
    }) as typeof window.open;
  });
  const opened = () => page.evaluate(() => (window as unknown as { __opened: string[] }).__opened);
  const link = page.frameLocator('iframe[title="Posta içeriği"]').getByRole('link', { name: 'www.odememerkezi.com.tr' });

  await link.click();
  const dialog = page.getByRole('alertdialog', { name: 'Bağlantı başka bir siteye gidiyor' });
  await expect(dialog).toContainText('gerçek hedef odememerkezi-hesap.example');
  await expect(dialog).toContainText('https://odememerkezi-hesap.example/giris');
  // Odak güvenli eylemde; Esc kapatır.
  await expect(dialog.getByRole('button', { name: 'Vazgeç' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);

  await link.click();
  await dialog.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(dialog).toHaveCount(0);
  expect(await opened()).toEqual([]);

  await link.click();
  await dialog.getByRole('button', { name: 'Yine de aç' }).click();
  await expect(dialog).toHaveCount(0);
  expect(await opened()).toEqual(['https://odememerkezi-hesap.example/giris']);

  // İşaretsiz bağlantı onaysız açılır.
  await list(page).getByText('Haftalık özet: üç görev tamamlandı', { exact: true }).click();
  await page.evaluate(() => {
    (window as unknown as { __opened: string[] }).__opened = [];
    window.open = ((url: string) => {
      (window as unknown as { __opened: string[] }).__opened.push(url);
      return null;
    }) as typeof window.open;
  });
  await page.frameLocator('iframe[title="Posta içeriği"]').getByRole('link', { name: 'Panoyu aç' }).click();
  await expect.poll(opened).toEqual(['https://projetakvimi.app/pano']);
  await expect(page.getByRole('alertdialog')).toHaveCount(0);
});

test('doğrulama kodu: 15 px kalın sabit genişlikli rakam; Kopyala panoya yazar', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await openArchived(page, 'Giriş doğrulama kodun');
  const code = page.locator('.otp-code');
  await expect(code).toHaveText('482 913');
  const style = await code.evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.fontSize, s.fontWeight, s.fontVariantNumeric];
  });
  expect(style).toEqual(['15px', '600', 'tabular-nums']);
  await page.getByRole('button', { name: 'Doğrulama kodunu kopyala' }).click();
  await expect(page.getByRole('status')).toContainText('Kod kopyalandı');
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('482913');
});

test('bildirim kartı: doğrulama kodunda "Kodu kopyala" birincil eylem', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/#/notify');
  // Örnek kartta kod yok: eylemler değişmez.
  const plain = page.getByRole('alertdialog', { name: 'Yeni posta: Ayşe Demir' });
  await expect(plain.getByRole('button', { name: 'Arşivle' })).toBeVisible();
  await expect(plain.getByRole('button', { name: 'Kodu kopyala' })).toHaveCount(0);

  await page.evaluate(async () => {
    const mh = (window as unknown as {
      __mh: { state: { messages: { id: number; subject: string }[] }; call: (n: string, a?: unknown) => Promise<unknown>; emit: (e: string, p: unknown) => void };
    }).__mh;
    const m = mh.state.messages.find((x) => x.subject === 'Giriş doğrulama kodun')!;
    const d = (await mh.call('message_get', { id: m.id })) as { otp: string };
    mh.emit('mh:notify', { message: d, accountName: 'Kişisel', accountColor: '#2E9A61', otp: d.otp });
  });
  const card = page.getByRole('alertdialog', { name: 'Yeni posta: Bulut Fatura' });
  const copy = card.getByRole('button', { name: 'Kodu kopyala' });
  await expect(copy).toHaveClass(/btn-primary/);
  await copy.click();
  await expect(card.getByRole('button', { name: 'Kod kopyalandı' })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('482913');
  // Kart sığar: eylemler kartın içinde kalır.
  const cardBox = (await card.boundingBox())!;
  const last = (await card.getByRole('button', { name: 'Okundu say' }).boundingBox())!;
  expect(last.x + last.width).toBeLessThanOrEqual(cardBox.x + cardBox.width);
});
