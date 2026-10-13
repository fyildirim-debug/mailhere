import { expect, test } from '@playwright/test';

test('hesap ekleme: adres → ayarlar → parola → hazır', async ({ page }) => {
  await page.goto('/?bos#/main');
  await expect(page.getByRole('heading', { name: 'Hesap ekle' })).toBeVisible();
  await page.getByLabel('E-posta adresi').fill('furkan@ornek.com.tr');
  await page.getByRole('button', { name: 'Devam' }).click();
  await expect(page.getByRole('heading', { name: 'Ayarlar bulundu' })).toBeVisible();
  await expect(page.getByText('imap.ornek.com.tr · 993')).toBeVisible();
  await page.getByLabel('Parola', { exact: true }).fill('yanlis');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(page.getByRole('alert')).toContainText('parola');
  await page.getByLabel('Parola', { exact: true }).fill('dogru-parola');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(page.getByRole('heading', { name: 'Hazırsın' })).toBeVisible();
  await page.getByRole('button', { name: 'Gelen kutusunu aç' }).click();
  await expect(page.getByText('Burası sakin')).toBeVisible();
});

test('bildirim kartı görünür, Okundu say kartı kapatır', async ({ page }) => {
  await page.goto('/#/notify');
  const card = page.getByRole('alertdialog', { name: 'Yeni posta: Ayşe Demir' });
  await expect(card).toBeVisible();
  await expect(card).toContainText('Cumartesi kahvaltı?');
  await card.getByRole('button', { name: 'Okundu say' }).click();
  await expect(card).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Bildirimi yeniden göster' })).toBeVisible();
});

test('hızlı okuma: arşivle ve geri al', async ({ page }) => {
  await page.goto('/#/quick/4');
  await expect(page.getByRole('heading', { name: 'Cumartesi kahvaltı?' })).toBeVisible();
  await page.getByRole('button', { name: 'Teşekkürler!' }).click();
  await expect(page.getByRole('textbox', { name: 'Yanıtla' })).toHaveValue('Teşekkürler!');
  await page.getByRole('button', { name: 'Arşivle' }).click();
  await expect(page.getByText('Arşivlendi')).toBeVisible();
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(page.getByRole('heading', { name: 'Cumartesi kahvaltı?' })).toBeVisible();
});

test('yazma: alıcı çipi, alıcısız uyarı, gönder ve geri al', async ({ page }) => {
  await page.goto('/#/compose/new/0');
  await page.getByRole('button', { name: /^Gönder/ }).click();
  await expect(page.getByRole('alert')).toContainText('En az bir alıcı');
  const to = page.locator('#compose-to');
  await to.fill('kerem.aydin@ornek.com.tr');
  await to.press('Enter');
  await expect(page.getByRole('button', { name: 'kerem.aydin@ornek.com.tr alıcısını kaldır' })).toBeVisible();
  await page.locator('#compose-subject').fill('Plan');
  await page.getByRole('textbox', { name: 'Posta metni' }).fill('Merhaba');
  await page.keyboard.press('Control+Enter');
  await expect(page.getByText('Giden kutusunda', { exact: false })).toBeVisible();
  await page.getByRole('button', { name: 'Geri al' }).click();
  await expect(page.getByRole('textbox', { name: 'Posta metni' })).toBeEnabled();
});

test('yanıt penceresi alıcıyı ve alıntıyı doldurur', async ({ page }) => {
  await page.goto('/#/compose/replyAll/3');
  await expect(page.getByText('Yanıt · Q4 bütçe taslağı')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Kerem Aydın alıcısını kaldır' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Ayşe Demir alıcısını kaldır' })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Posta metni' })).toHaveValue(/> Merhaba,/);
});

test('yazma: kişi önerisi geçmiş postalardan gelir', async ({ page }) => {
  await page.goto('/#/compose/new/0');
  const to = page.locator('#compose-to');
  await to.fill('elif');
  await expect(page.getByRole('option', { name: /Elif Şahin/ })).toBeVisible();
  await to.press('Enter');
  await expect(page.getByRole('button', { name: 'Elif Şahin alıcısını kaldır' })).toBeVisible();
  await expect(to).toHaveValue('');
});

test('yazma: taslak kendiliğinden kaydedilir, silince kaldırılır', async ({ page }) => {
  await page.goto('/#/compose/new/0');
  await page.locator('#compose-subject').fill('Taslak deneme');
  await page.getByRole('textbox', { name: 'Posta metni' }).fill('Yarın devam ederim.');
  await expect(page.getByText('Taslak kaydedildi')).toBeVisible();
  const count = () => page.evaluate(() => (window as unknown as { __mh: { state: { messages: { subject: string }[] } } }).__mh.state.messages.filter((m) => m.subject === 'Taslak deneme').length);
  expect(await count()).toBe(1);
  // İkinci düzenleme aynı taslağın üzerine yazar.
  await page.getByRole('textbox', { name: 'Posta metni' }).fill('Yarın sabah devam ederim.');
  await expect(page.getByText('Taslak kaydedildi')).toBeVisible();
  await page.waitForTimeout(1800);
  expect(await count()).toBe(1);
  await page.getByRole('button', { name: 'Taslağı sil' }).click();
  await expect.poll(count).toBe(0);
});

test('iletme: özgün postanın ekleri gösterilir', async ({ page }) => {
  await page.goto('/#/compose/forward/3');
  await expect(page.getByText('İletme · Q4 bütçe taslağı')).toBeVisible();
  await expect(page.getByText('Butce_Q4_taslak.xlsx')).toBeVisible();
  await expect(page.getByText('Sunum_ozeti.pdf')).toBeVisible();
});
