import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';
import { contrast } from './kontrast';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil 0.11 · AMOLED siyah: Ayarlar › Genel'de yalnız bu telefonda tutulan
// tercih; koyu temada zeminler tam siyah, metin / zemin çiftleri en az 4.5:1;
// açık temada etkisiz; yeniden açılışta korunur; sistem çubukları koyu temadaki
// gibi açık simgeli kalır (arka planları uygulamanın siyah çubuklarıdır).

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });
const amoledSwitch = (page: Page) => page.getByRole('switch', { name: 'AMOLED siyah' });
const BLACK = 'rgb(0, 0, 0)';

async function setTheme(page: Page, theme: 'light' | 'dark') {
  await page.evaluate((th) => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, theme: th } }), theme);
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
}

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

async function drawerGone(page: Page) {
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => page.locator('.scrim').evaluateAll((els) => els.every((el) => getComputedStyle(el).visibility === 'hidden'))).toBe(true);
}

async function openGeneral(page: Page) {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await page.getByRole('button', { name: 'Genel' }).click();
  await expect(h1(page, 'Genel')).toBeVisible();
}

const bg = (page: Page, selector: string) => page.locator(selector).first().evaluate((el) => getComputedStyle(el).backgroundColor);
/** Jetonun çözülmüş rengi (`rgb(…)`); derleme kısaltmalarından (`#fff`) bağımsız. */
const token = (page: Page, name: string) =>
  page.evaluate((n) => {
    const probe = document.createElement('span');
    probe.style.color = `var(${n})`;
    document.body.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  }, name);
/** Zemin siyaha oturana dek bekler (çubuk zeminleri 200 ms'de geçer). */
const black = (page: Page, selector: string) => expect.poll(() => bg(page, selector), selector).toBe(BLACK);

/** Görünen her eşleşmenin karşıtlığı en az 4.5:1. */
async function readable(page: Page, selectors: string[], where: string) {
  for (const sel of selectors) {
    const found = page.locator(sel).filter({ visible: true });
    const n = await found.count();
    expect(n, `${where}: ${sel} bulunmalı`).toBeGreaterThan(0);
    for (let i = 0; i < Math.min(n, 6); i++) {
      const el = found.nth(i);
      if (!(await el.textContent())?.trim()) continue;
      expect(await contrast(el), `${where}: ${sel} #${i}`).toBeGreaterThanOrEqual(4.5);
    }
  }
}

/** Giriş hareketi (itilen ekran, alt sayfa) bitene dek bekler. */
async function settled(page: Page) {
  await page.evaluate(async () => {
    await Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined)));
  });
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('ayar: varsayılan kapalı; açılınca kök öğeye yazılır, cihazda saklanır ve yeniden açılışta korunur', async ({ page }) => {
  await setTheme(page, 'dark');
  await expect(page.locator('html')).not.toHaveAttribute('data-amoled');
  await openGeneral(page);
  await expect(amoledSwitch(page)).not.toBeChecked();
  await expect(page.getByText('Koyu temada zeminler tam siyah olur')).toBeVisible();
  // Koyu temadayken "açık tema etkin" notu yok.
  await expect(page.getByText('Şu an açık tema etkin')).toHaveCount(0);
  await amoledSwitch(page).click();
  await expect(amoledSwitch(page)).toBeChecked();
  await expect(page.locator('html')).toHaveAttribute('data-amoled');
  expect(await page.evaluate(() => localStorage.getItem('mailhere.mob.amoled'))).toBe('1');
  // Arka uç ayarına girmez (cihazlar arası aktarımla taşınmaz).
  expect(await page.evaluate(() => JSON.stringify(window.__mh.state.settings).toLowerCase().includes('amoled'))).toBe(false);
  await black(page, '.m-screen.pushed .body');

  await page.reload();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-amoled');
  await black(page, '.m-shell');

  await openGeneral(page);
  await expect(amoledSwitch(page)).toBeChecked();
  await amoledSwitch(page).click();
  await expect(page.locator('html')).not.toHaveAttribute('data-amoled');
  expect(await page.evaluate(() => localStorage.getItem('mailhere.mob.amoled'))).toBeNull();
  expect(await bg(page, '.m-screen.pushed .body')).not.toBe(BLACK);
});

test('açık temada etkisiz: jetonlar değişmez, ayar bunu söyler', async ({ page }) => {
  await setTheme(page, 'light');
  const before = await Promise.all(['--ground', '--surface', '--paper', '--raised', '--line', '--line-strong', '--hover'].map((n) => token(page, n)));
  await openGeneral(page);
  await amoledSwitch(page).click();
  await expect(page.locator('html')).toHaveAttribute('data-amoled');
  await expect(page.getByText('Şu an açık tema etkin; ayar koyu temaya geçince görünür.')).toBeVisible();
  const after = await Promise.all(['--ground', '--surface', '--paper', '--raised', '--line', '--line-strong', '--hover'].map((n) => token(page, n)));
  expect(after).toEqual(before);
  expect(await token(page, '--paper')).toBe('rgb(255, 255, 255)');
  // Koyu temaya geçince hemen etkili.
  await setTheme(page, 'dark');
  await expect(page.getByText('Şu an açık tema etkin')).toHaveCount(0);
  expect(await token(page, '--paper')).toBe(BLACK);
  expect(await token(page, '--ink')).toBe('rgb(230, 232, 237)');
});

test('AMOLED: zeminler tam siyah; liste, çekmece, okuma, ayarlar ve alt sayfada metin en az 4.5:1', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('mailhere.mob.amoled', '1'));
  await page.reload();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await setTheme(page, 'dark');
  await expect(page.locator('html')).toHaveAttribute('data-amoled');

  // Liste: uygulama çubuğu (durum çubuğunun arkası) ve liste zemini siyah.
  await black(page, '.appbar');
  await black(page, '.list');
  await black(page, '.m-shell');
  await readable(page, ['.list .big', '.list .sub', '.list .group', '.row .sender', '.row .subject', '.row .snippet', '.row .time', '.m-chip', '.m-fab-label'], 'liste');

  // Arşiv: kod çipi ve "Daha eski postalar".
  await openDrawer(page);
  await settled(page);
  await black(page, '.drawer');
  await readable(page, ['.drawer .item', '.drawer .name', '.drawer .group-title'], 'çekmece');
  await drawer(page).getByRole('button', { name: /^Arşiv/ }).click();
  await expect(h1(page, 'Arşiv')).toBeVisible();
  await drawerGone(page);
  await readable(page, ['.otp-chip', '.older .btn-soft'], 'arşiv');

  // Seçim: seçili satır ve alt çubuk (gezinme çubuğunun arkası yükseltilmiş katman).
  await page.getByRole('button', { name: 'Yeni cihazdan giriş kodun postasını seç' }).click();
  await expect(page.getByRole('heading', { level: 1, name: '1 seçili' })).toBeVisible();
  await settled(page);
  await readable(page, ['.row.checked .sender', '.row.checked .subject', '.row.checked .snippet', '.row.checked .time', '.bar .act', '.appbar.select h1'], 'seçim');
  await page.getByRole('button', { name: 'Seçimi bırak' }).click();

  // Okuma: kâğıt siyah, başlık ve üst bilgi okunur; şerit yükseltilmiş katmanda.
  await row(page, 'Gönderiniz gümrükte bekliyor').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Gönderiniz gümrükte bekliyor' })).toBeVisible();
  await settled(page);
  await black(page, '.m-screen.pushed .scroll');
  await black(page, '.m-screen.pushed .appbar');
  await readable(page, ['.article h2', '.article .who-btn', '.article .when-inline', '.strip .wide'], 'okuma');
  // Düz metin posta temayı izler: gövde siyah kâğıtta.
  await readable(page, ['.article .body'], 'okuma gövdesi');

  // Alt sayfa menüsü: yükseltilmiş katman siyahtan ayrışır.
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Diğer eylemler' }).click();
  await expect(page.getByRole('menu')).toBeVisible();
  await settled(page);
  expect(await bg(page, '.sheet')).not.toBe(BLACK);
  await readable(page, ['.sheet .menu-item'], 'alt sayfa');
  await page.keyboard.press('Escape');
  await page.goBack();
  await expect(h1(page, 'Arşiv')).toBeVisible();

  // Ayarlar: açıklamalar, segmentler (iz kenarla belirir) ve seçili değerler.
  await openGeneral(page);
  await settled(page);
  await black(page, '.m-screen.pushed .body');
  await readable(page, ['.body .set-h', '.body .set-name', '.body .set-hint', '.body .seg', '.body .pick .value'], 'ayarlar');
  const track = page.locator('.body .seg-track').first();
  expect(await track.evaluate((el) => getComputedStyle(el).borderTopWidth)).toBe('1px');
  expect(await track.evaluate((el) => getComputedStyle(el).borderTopColor)).not.toBe(BLACK);
  await page.goBack();
  await page.getByRole('button', { name: 'Gizlilik ve güvenlik' }).click();
  await expect(h1(page, 'Gizlilik ve güvenlik')).toBeVisible();
  await settled(page);
  await readable(page, ['.body .set-lead', '.body .none', '.body .label', '.body .facts li'], 'gizlilik');
});

test('AMOLED: toast ve onay alt sayfası okunur; yatay taşma yok', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('mailhere.mob.amoled', '1'));
  await page.reload();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await setTheme(page, 'dark');
  await row(page, 'Cumartesi kahvaltı?').click();
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Arşivle' }).click();
  const toast = page.locator('.toast[role="status"]');
  await expect(toast).toBeVisible();
  await settled(page);
  await readable(page, ['.toast[role="status"]', '.toast button'], 'toast');

  // Çöp: kalıcı silme onayı.
  await page.evaluate(async () => {
    const ids = window.__mh.state.messages.filter((m) => m.subject === 'Sözleşme imzaya hazır').map((m) => m.id);
    await window.__mh.call('messages_move', { ids, target: 'trash' });
  });
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: /^Çöp/ }).click();
  await expect(h1(page, 'Çöp')).toBeVisible();
  await drawerGone(page);
  await row(page, 'Sözleşme imzaya hazır').click();
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(page.getByRole('dialog', { name: 'Bu posta kalıcı silinsin mi?' })).toBeVisible();
  await settled(page);
  await readable(page, ['.sheet .title', '.sheet .body', '.sheet .btn-soft'], 'onay');
  expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)).toBe(false);
});

test('sistem çubukları: AMOLED’de de koyu tema simgeleri istenir', async ({ page }) => {
  await page.evaluate(() => {
    const w = window as unknown as { __bars: boolean[] };
    w.__bars = [];
    const mh = window.__mh;
    const real = mh.call.bind(mh);
    mh.call = ((name: string, args: { dark: boolean }) => {
      if (name === 'system_bars') w.__bars.push(args.dark);
      return real(name as never, args as never);
    }) as typeof mh.call;
  });
  const bars = () => page.evaluate(() => (window as unknown as { __bars: boolean[] }).__bars);
  await setTheme(page, 'dark');
  await expect.poll(async () => (await bars()).at(-1)).toBe(true);
  await openGeneral(page);
  await amoledSwitch(page).click();
  await expect(page.locator('html')).toHaveAttribute('data-amoled');
  // Simge rengi değişmez (koyu zeminde açık simge); çubukların arkası uygulamanın siyah çubuğudur.
  expect((await bars()).at(-1)).toBe(true);
  await black(page, '.m-screen.pushed .appbar');
  await setTheme(page, 'light');
  await expect.poll(async () => (await bars()).at(-1)).toBe(false);
});
