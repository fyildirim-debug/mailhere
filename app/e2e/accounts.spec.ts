import { expect, test } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Hesap ekleme yolları, sertifika onayı, hesap düzenleme ve güncelleme.

test('JMAP: oturum adresi bulunur, hesap eklenir', async ({ page }) => {
  await page.goto('/?bos#/main');
  await page.getByRole('button', { name: 'JMAP' }).click();
  await expect(page.getByRole('heading', { name: 'JMAP hesabı' })).toBeVisible();
  await page.getByLabel('E-posta adresi').fill('furkan@ornek.dev');
  await page.getByLabel('E-posta adresi').blur();
  await expect(page.getByLabel('Oturum adresi')).toHaveValue('https://ornek.dev/.well-known/jmap');
  await expect(page.getByLabel('Kullanıcı adı')).toHaveValue('furkan@ornek.dev');
  await page.getByLabel('Parola').fill('parola');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(page.getByRole('heading', { name: 'Hazırsın' })).toBeVisible();
  await page.getByRole('button', { name: 'Gelen kutusunu aç' }).click();
  await expect(page.getByRole('navigation').getByText('JMAP')).toBeVisible();
});

test('Google: tarayıcı girişi izin verir, ad sorulur, hesap eklenir', async ({ page }) => {
  await page.goto('/?bos#/main');
  await page.getByRole('button', { name: 'Google hesabı' }).click();
  await expect(page.getByRole('heading', { name: 'Tarayıcıda giriş bekleniyor…' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Google girişi tamam' })).toBeVisible();
  await expect(page.getByLabel('Hesap adı')).toHaveValue('Gmail');
  await expect(page.getByLabel('Gönderen adın')).toHaveValue('Furkan Yıldırım');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(page.getByRole('heading', { name: 'Hazırsın' })).toBeVisible();
});

test('güvenilmeyen sertifika: parmak izi gösterilir, onayla hesap eklenir', async ({ page }) => {
  await page.goto('/?bos#/main');
  await page.getByLabel('E-posta adresi').fill('furkan@kendinden.dev');
  await page.getByRole('button', { name: 'Devam' }).click();
  await page.getByLabel('Parola').fill('parola');
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();

  const dialog = page.getByRole('alertdialog', { name: 'Bu sunucuya güvenilmiyor' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('kendinden imzalı');
  await expect(dialog).toContainText('A1:B2:C3');

  // Vazgeçince hesap eklenmez.
  await dialog.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Ayarlar bulundu' })).toBeVisible();

  // Gelen ve giden sunucunun sertifikası sırayla onaylanır.
  await page.getByRole('button', { name: 'Hesabı ekle' }).click();
  await expect(dialog).toContainText('1/2');
  await expect(dialog).toContainText('imap.kendinden.dev:993');
  await dialog.getByRole('button', { name: 'Bu sertifikaya güven' }).click();
  await expect(dialog).toContainText('2/2');
  await expect(dialog).toContainText('smtp.kendinden.dev:465');
  await dialog.getByRole('button', { name: 'Bu sertifikaya güven' }).click();
  await expect(page.getByRole('heading', { name: 'Hazırsın' })).toBeVisible();
});

test('ayarlar: hesap adı, rengi ve imzaları düzenlenir; güvenilen sertifika listelenir', async ({ page }) => {
  await page.goto('/#/main');
  await page.evaluate(() => window.__mh.call('cert_trust', { host: 'posta.ornek.dev', port: 993, sha256: 'AA:BB:CC:DD:EE:FF:00:11:22:33' }));
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  const nav = page.getByRole('navigation', { name: 'Ayarlar' });
  await nav.getByRole('button', { name: 'Gizlilik ve güvenlik' }).click();
  await expect(page.getByText('posta.ornek.dev:993')).toBeVisible();

  await nav.getByRole('button', { name: 'İş' }).click();
  await expect(page.getByRole('tab', { name: 'Genel' })).toHaveAttribute('aria-selected', 'true');
  await page.getByLabel('Hesap adı').fill('Ofis');
  await page.getByRole('radio', { name: 'Renk 4' }).click();
  await page.getByRole('button', { name: 'Kaydet' }).click();
  await expect(page.getByRole('status')).toContainText('Kaydedildi');
  await expect(page.getByRole('navigation', { name: 'Kutular ve hesaplar' }).getByRole('button', { name: 'Ofis ornek.com.tr' })).toBeVisible();

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
  const sig = acc.signatures.find((s) => s.id === acc.signatureNew);
  expect(sig?.body).toBe('Furkan YILDIRIM\nMailhere');
  expect(acc.signatureReply).toBe('is-kisa');
});

test('güncelleme: elle denetlenir, kenar çubuğunda kart çıkar', async ({ page }) => {
  await page.goto('/?guncelleme#/main');
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: 'Güncellemeler' }).click();
  await page.getByRole('button', { name: 'Şimdi denetle' }).click();
  await expect(page.getByText('Mailhere 0.2.0 hazır').first()).toBeVisible();
  await expect(page.getByRole('navigation').getByText('0.2.0 sürümü hazır')).toBeVisible();
  await page.getByRole('navigation').getByRole('button', { name: 'Sonra' }).click();
  await expect(page.getByRole('navigation').getByText('0.2.0 sürümü hazır')).toHaveCount(0);
});

test('taslak: okuma panelinde düzenle düğmesi çıkar, hızlı yanıt gizlenir', async ({ page }) => {
  await page.goto('/#/main');
  await expect(page.getByRole('heading', { name: 'Gelen Kutusu' })).toBeVisible();
  await page.evaluate(() =>
    window.__mh.call('draft_save', {
      msg: {
        accountId: window.__mh.state.accounts[0].id,
        to: [{ name: 'Derin', email: 'derin@ornek.com' }],
        cc: [],
        bcc: [],
        subject: 'Yarım kalan',
        bodyText: 'Merhaba',
        inReplyTo: null,
        attachments: [],
        sendAt: null,
      },
    }),
  );
  await page.getByRole('navigation').getByRole('button', { name: /Taslaklar/ }).click();
  await page.getByRole('region', { name: 'Posta listesi' }).getByText('Yarım kalan').click();
  await expect(page.getByRole('button', { name: 'Taslağı düzenle' })).toBeVisible();
  await expect(page.getByPlaceholder('Hızlı yanıt yaz…')).toHaveCount(0);
});

test('DNS başka alan adındaki sunucuyu gösterirse uyarı çıkar', async ({ page }) => {
  await page.goto('/?bos#/main');
  await page.getByLabel('E-posta adresi').fill('furkan@srv-ornek.dev');
  await page.getByRole('button', { name: 'Devam' }).click();
  await expect(page.getByRole('note')).toContainText('imap.barindirma.example');
  await expect(page.getByRole('note')).toContainText('parolanı yazma');
});
