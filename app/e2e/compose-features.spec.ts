import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Yanıt gelmezse hatırlat, şablon ve hızlı yanıt değişkenleri, yeni gönderici eleği.

const side = (page: Page) => page.getByRole('navigation', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });

async function openSettings(page: Page, section: string) {
  await page.getByRole('button', { name: 'Ayarlar' }).click();
  await page.getByRole('navigation', { name: 'Ayarlar' }).getByRole('button', { name: section, exact: true }).click();
}

test('şablon değişkenleri yazma penceresinde alıcı ve gönderenle doldurulur', async ({ page }) => {
  await page.goto('/#/main');
  await page.evaluate(() =>
    window.__mh.call('settings_set', {
      settings: {
        ...window.__mh.state.settings,
        templates: [{ id: '', name: 'Selamlama', subject: 'Merhaba {ad}', body: 'Merhaba {ad},\n\n{tam_ad} için not. Sevgiler, {benim_adim} · {first_name}' }],
      },
    }),
  );
  // Yanıt penceresi: alıcı Kerem Aydın, gönderen İş hesabı (Furkan).
  await page.goto('/#/compose/reply/3');
  await expect(page.getByRole('button', { name: 'Kerem Aydın alıcısını kaldır' })).toBeVisible();
  await page.getByRole('button', { name: 'Şablon ekle' }).click();
  await page.getByRole('menuitem', { name: 'Selamlama' }).click();
  const body = page.getByRole('textbox', { name: 'Posta metni' });
  await expect(body).toHaveValue(/Merhaba Kerem,\n\nKerem Aydın için not\. Sevgiler, Furkan · Kerem/);
  // Yanıtta konu korunur.
  await expect(page.getByLabel('Konu')).toHaveValue('Re: Q4 bütçe taslağı');

  // Alıcısız yeni postada değişken boş kalır, süslü parantez sızmaz.
  await page.goto('/#/compose/new/0');
  await page.getByRole('button', { name: 'Şablon ekle' }).click();
  await page.getByRole('menuitem', { name: 'Selamlama' }).click();
  await expect(page.getByLabel('Konu')).toHaveValue('Merhaba');
  const text = await page.getByRole('textbox', { name: 'Posta metni' }).inputValue();
  expect(text.startsWith('Merhaba,\n\n')).toBe(true);
  expect(text).not.toContain('{');
  expect(text).toContain('Sevgiler, Furkan');
});

test('hızlı yanıt değişkenleri hızlı okumada ve okuma panelinde doldurulur', async ({ page }) => {
  await page.goto('/#/main');
  await page.evaluate(() =>
    window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, quickReplies: ['Teşekkürler {ad}!', 'Bakıp dönüyorum.'] } }),
  );
  await page.goto('/#/quick/4');
  await page.getByRole('button', { name: 'Teşekkürler {ad}!' }).click();
  await expect(page.getByRole('textbox', { name: 'Yanıtla' })).toHaveValue('Teşekkürler Ayşe!');

  await page.goto('/#/main');
  await list(page).getByText('Sözleşme imzaya hazır').click();
  // Hazır yanıtlar kutu odaklanınca açılır.
  await page.getByRole('textbox', { name: 'Hızlı yanıt yaz…' }).click();
  await page.getByRole('group', { name: 'Hızlı yanıtlar' }).getByRole('button', { name: 'Teşekkürler {ad}!' }).click();
  await expect(page.getByRole('textbox', { name: 'Hızlı yanıt yaz…' })).toHaveValue('Teşekkürler Elif!');
});

test('yazma: gönder menüsünden yanıt hatırlatması seçilir ve gönderimle kurulur', async ({ page }) => {
  await page.goto('/#/main');
  // Sahte arka uç ilk komutta kurulur; hazır olmadan çağrılmasın.
  await page.waitForFunction(() => !!window.__mh);
  await page.evaluate(() => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, undoSendSeconds: 0 } }));
  await page.goto('/#/compose/new/0');
  const to = page.locator('#compose-to');
  await to.fill('yeni.kisi@ornek.com');
  await to.press('Enter');
  await page.locator('#compose-subject').fill('Teklif');
  await page.getByRole('textbox', { name: 'Posta metni' }).fill('Ekte.');

  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  const menu = page.getByRole('group', { name: 'Yanıt gelmezse hatırlat' });
  await expect(menu.getByRole('menuitemcheckbox', { name: /Yarın/ })).toBeVisible();
  await menu.getByRole('menuitemcheckbox', { name: /3 gün sonra/ }).click();
  // Taslak durumu da `status` olduğundan hatırlatma satırı metniyle seçilir.
  const follow = page.getByRole('status').filter({ hasText: 'Yanıt gelmezse hatırlat ·' });
  await expect(follow).toBeVisible();

  // Özel tarih de seçilebilir.
  await page.getByRole('button', { name: 'Sonra gönder' }).click();
  await page.getByRole('menuitem', { name: 'Özel tarih…' }).click();
  await page.getByLabel('Hatırlatma tarihi ve saati').fill('2030-01-02T09:30');
  await page.getByRole('button', { name: 'Ayarla' }).click();
  await expect(follow).toContainText('2030');

  await page.getByRole('button', { name: /^Gönder/ }).click();
  await expect.poll(() => page.evaluate(() => window.__mh.call('followups_list').then((l) => l.length))).toBe(1);
  const item = await page.evaluate(() => window.__mh.call('followups_list').then((l) => l[0]));
  expect(item.subject).toBe('Teklif');
  expect(new Date(item.at).getFullYear()).toBe(2030);
});

test('yanıt bekleyenler: süresi dolan hatırlatma gelen kutusunun üstünde, okuma panelinden kaldırılır', async ({ page }) => {
  await page.goto('/#/main');
  // Gönderilmiş "Q4 bütçe taslağı" (id 1): hatırlatma kur ve süresini geçmişe çek.
  await page.evaluate(async () => {
    await window.__mh.call('followup_set', { id: 1, at: Date.now() + 60_000 });
    window.__mh.state.followUps[1].at = Date.now() - 1000;
    window.__mh.emit('mh:changed', { mailboxIds: [] });
  });
  await expect(side(page).getByRole('button', { name: 'Yanıt bekleyenler 1' })).toBeVisible();
  const rows = list(page).locator('.row');
  await expect(rows.first()).toContainText('Yanıt bekleniyor');
  await expect(rows.first()).toContainText('Kerem Aydın');
  await expect(rows.first()).toContainText('Q4 bütçe taslağı');

  await side(page).getByRole('button', { name: 'Yanıt bekleyenler 1' }).click();
  await expect(list(page).getByRole('heading', { name: 'Yanıt bekleyenler' })).toBeVisible();
  await rows.first().getByRole('button').first().click();
  await expect(page.getByRole('note').filter({ hasText: 'Yanıt gelmedi' })).toBeVisible();

  // Diğer eylemler › Yanıt gelmezse hatırlat: yeniden kur (1 hafta).
  await page.getByRole('button', { name: 'Diğer eylemler' }).click();
  await page.getByRole('menuitem', { name: 'Yanıt gelmezse hatırlat' }).click();
  await page.getByRole('menu', { name: 'Yanıt gelmezse hatırlat' }).getByRole('menuitemcheckbox', { name: /1 hafta sonra/ }).click();
  await expect(page.getByRole('status')).toContainText('Yanıt gelmezse hatırlatılır');
  await expect(page.getByRole('note').filter({ hasText: 'Yanıt gelmezse hatırlatılır' })).toBeVisible();

  // Kaldır: görünüm boşalır ve kenar çubuğundan düşer.
  await page.getByRole('button', { name: 'Hatırlatmayı kaldır' }).click();
  await expect(page.getByText('Yanıt bekleyen posta yok')).toBeVisible();
  await side(page).getByRole('button', { name: 'Gelen Kutusu' }).click();
  await expect(side(page).getByRole('button', { name: /Yanıt bekleyenler/ })).toHaveCount(0);
});

test('elek: ayar açılır, ilk kez yazan bekler; kabul gelen kutusuna döndürür, engelleme çöpe atar ve kural kurar', async ({ page }) => {
  await page.goto('/#/main');
  await openSettings(page, 'Bildirimler');
  const sw = page.getByRole('switch', { name: 'İlk kez yazanları elekte beklet' });
  await expect(sw).not.toBeChecked();
  await sw.check();
  expect(await page.evaluate(() => window.__mh.state.settings.screener)).toBe(true);
  await page.getByRole('button', { name: 'Ayarları kapat' }).click();

  // Elif ve Derin Kargo ilk kez yazıyor.
  await page.evaluate(() => {
    const id = (subject: string) => window.__mh.state.messages.find((m) => m.subject === subject)!.id;
    window.__mh.state.screened.push(id('Sözleşme imzaya hazır'), id('Gönderiniz dağıtıma çıktı'));
    window.__mh.emit('mh:changed', { mailboxIds: [] });
  });
  await expect(side(page).getByRole('button', { name: 'Elek 2' })).toBeVisible();
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toHaveCount(0);

  await side(page).getByRole('button', { name: 'Elek 2' }).click();
  await expect(list(page).getByRole('heading', { name: 'Elek' })).toBeVisible();
  await list(page).getByText('Sözleşme imzaya hazır').click();
  await expect(page.getByRole('note')).toContainText('Elif Şahin sana ilk kez yazıyor');
  await page.getByRole('button', { name: 'Kabul et' }).click();
  await expect(page.getByRole('status')).toContainText('kabul edildi');
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toHaveCount(0);
  expect(await page.evaluate(() => window.__mh.state.known)).toContain('demo-is|elif.sahin@hukukbiz.com.tr');

  await list(page).getByText('Gönderiniz dağıtıma çıktı', { exact: true }).click();
  await page.getByRole('button', { name: 'Engelle' }).click();
  await expect(page.getByRole('status')).toContainText('engellendi');
  await expect(page.getByText('Elek boş')).toBeVisible();
  const state = await page.evaluate(() => ({
    rule: window.__mh.state.rules.some((r) => r.name === 'Engellendi: bilgi@derinkargo.com.tr'),
    box: window.__mh.state.messages.find((m) => m.subject === 'Gönderiniz dağıtıma çıktı')?.mailboxId,
  }));
  expect(state).toEqual({ rule: true, box: 'demo-kisisel:Trash' });

  // Kabul edilen gelen kutusuna döndü, kenar çubuğundan Elek düştü.
  await side(page).getByRole('button', { name: 'Gelen Kutusu' }).click();
  await expect(list(page).getByText('Sözleşme imzaya hazır')).toBeVisible();
  await expect(side(page).getByRole('button', { name: /^Elek/ })).toHaveCount(0);
});

test('ayarlar › yazma ve gönderme: değişken yardımı listelenir', async ({ page }) => {
  await page.goto('/#/main');
  await openSettings(page, 'Yazma ve gönderme');
  await expect(page.getByRole('heading', { name: 'Değişkenler' })).toBeVisible();
  await expect(page.getByText('{benim_adim}')).toBeVisible();
  await expect(page.getByText('Alıcının adı', { exact: true })).toBeVisible();
});
