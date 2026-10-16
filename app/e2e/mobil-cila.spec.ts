import { expect, test, type Locator, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
  }
}

// Mobil cila (Paket M): hesap rozeti harf kontrastı (ayarlar listesi, çekmece
// başlığı; iki tema), satırdaki hesap işareti köşeli kare (okunmamış noktası
// yuvarlak kalır), toast "Geri al" ve yazma "Gönder" 44 px hedef, yanıt
// alanındaki hazır yanıt çiplerinin görünmez kenarı, sürüklenen satırın nötr
// zemini ve yüzen düğme etiketinin belgelenmiş daralması.

const drawer = (page: Page) => page.getByRole('dialog', { name: 'Kutular ve hesaplar' });
const list = (page: Page) => page.getByRole('region', { name: 'Posta listesi' });
const row = (page: Page, text: string) => list(page).getByText(text, { exact: true });
const h1 = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true });

async function openDrawer(page: Page) {
  await page.getByRole('button', { name: 'Menü', exact: true }).click();
  await expect(drawer(page)).toBeVisible();
}

/** Temayı sahte arka uçtan yazar; arayüz ayar olayıyla uygular. */
async function setTheme(page: Page, theme: 'light' | 'dark') {
  await page.evaluate((th) => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, theme: th } }), theme);
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
}

/**
 * WCAG kontrastı: metin rengi ve atalardan harmanlanan zemin tuvalde sRGB'ye
 * çözülür (`color-mix`, `color(srgb …/α)` dahil her biçim), saydam katmanlar
 * alttan üste bindirilir.
 */
async function contrast(el: Locator): Promise<number> {
  return el.evaluate((node) => {
    const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true })!;
    const rgba = (css: string): [number, number, number, number] => {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = '#000';
      ctx.fillStyle = css;
      ctx.fillRect(0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      return [d[0], d[1], d[2], d[3] / 255];
    };
    const over = (top: number[], under: number[]) => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3]));
    const layers: [number, number, number, number][] = [];
    for (let n: Element | null = node; n; n = n.parentElement) {
      const c = rgba(getComputedStyle(n).backgroundColor);
      if (c[3] > 0) layers.push(c);
      if (c[3] >= 1) break;
    }
    let bg = [255, 255, 255];
    for (const l of layers.reverse()) bg = over(l, bg);
    const fg = over(rgba(getComputedStyle(node).color), bg);
    const lum = (c: number[]) => {
      const [r, g, b] = c.map((v) => {
        const s = v / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
    return (a + 0.05) / (b + 0.05);
  });
}

/** Yükseklik yüzde bire yuvarlanır (Pixel 7 piksel oranında 44 px 43.99998 ölçülebilir). */
async function height(el: Locator): Promise<number> {
  const box = await el.boundingBox();
  expect(box, 'öğe görünür olmalı').not.toBeNull();
  return Math.round(box!.height * 100) / 100;
}

/** Verilen noktadaki en üst öğe hedef öğenin kendisi ya da içindeyse doğru. */
async function hitsAt(el: Locator, x: number, y: number): Promise<boolean> {
  return el.evaluate((node, [px, py]) => {
    const hit = document.elementFromPoint(px, py);
    return !!hit && (hit === node || node.contains(hit));
  }, [x, y] as const);
}

/** Öğenin ve atalarının giriş canlandırmaları biter (kutular kayarken ölçülmez). */
async function settled(el: Locator) {
  await el.evaluate(async (node) => {
    const anims: Animation[] = [];
    for (let n: Element | null = node; n; n = n.parentElement) anims.push(...n.getAnimations());
    await Promise.all(anims.map((a) => a.finished.catch(() => undefined)));
  });
}

test.beforeEach(async ({ page }) => {
  await page.goto('#/main');
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
});

test('C-M1: ayarlar listesindeki hesap rozeti harfi iki temada en az 4.5:1', async ({ page }) => {
  await openDrawer(page);
  await drawer(page).getByRole('button', { name: 'Ayarlar', exact: true }).click();
  await expect(h1(page, 'Ayarlar')).toBeVisible();
  const badges = page.locator('.m-screen.pushed .badge');
  await expect(badges).toHaveCount(3);
  for (const theme of ['light', 'dark'] as const) {
    await setTheme(page, theme);
    for (let i = 0; i < 3; i++) {
      const b = badges.nth(i);
      expect(await contrast(b), `${theme} rozet ${await b.textContent()}`).toBeGreaterThanOrEqual(4.5);
    }
  }
});

test('C-M1: tek hesapta çekmece başlığındaki rozet harfi iki temada en az 4.5:1', async ({ page }) => {
  // Sahte arka uç kurulurken yalnız ilk hesap bırakılır (çekmece başlığı hesabın kimliği olur).
  await page.addInitScript(() => {
    Object.defineProperty(window, '__mh', {
      configurable: true,
      set(mh: MockBackend) {
        const keep = mh.state.accounts[0].id;
        mh.state.accounts = mh.state.accounts.filter((a) => a.id === keep);
        mh.state.mailboxes = mh.state.mailboxes.filter((b) => b.accountId === keep);
        mh.state.messages = mh.state.messages.filter((m) => m.accountId === keep);
        Object.defineProperty(window, '__mh', { value: mh, writable: true, configurable: true });
      },
    });
  });
  await page.reload();
  await expect(h1(page, 'Gelen Kutusu')).toBeVisible();
  await openDrawer(page);
  const badge = drawer(page).locator('.badge');
  await expect(badge).toHaveCount(1);
  for (const theme of ['light', 'dark'] as const) {
    await setTheme(page, theme);
    expect(await contrast(badge), `${theme} çekmece rozeti`).toBeGreaterThanOrEqual(4.5);
  }
});

test('C-M2: satırdaki hesap işareti 8×8 köşe 2 px kare; okunmamış noktası yuvarlak', async ({ page }) => {
  const rowEl = list(page).locator('.row', { hasText: 'Cumartesi kahvaltı?' });
  const acc = rowEl.locator('.acc');
  await expect(acc).toBeVisible();
  const box = (await acc.boundingBox())!;
  expect([Math.round(box.width), Math.round(box.height)]).toEqual([8, 8]);
  expect(await acc.evaluate((el) => getComputedStyle(el).borderRadius)).toBe('2px');
  expect(await rowEl.locator('.dot').evaluate((el) => getComputedStyle(el).borderRadius)).toBe('50%');
});

test('C-M3: toast "Geri al" en az 44 px; toast yüksekliği değişmez', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click();
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Arşivle' }).click();
  const toast = page.getByRole('status').filter({ hasText: 'Arşivlendi' });
  await expect(toast).toBeVisible();
  await settled(toast);
  const undo = toast.getByRole('button', { name: 'Geri al' });
  expect(await height(undo)).toBeGreaterThanOrEqual(44);
  // Düğme negatif dış boşlukla taşar: toast 10 + 30 + 10 = 50 px kalır.
  expect(await height(toast)).toBeLessThanOrEqual(50.5);
  // Hedef toast'ın iç boşluğuna taşar: kenarın 4 px içinden dokunma Geri al'ı bulur.
  const t = (await toast.boundingBox())!;
  const u = (await undo.boundingBox())!;
  expect(await hitsAt(undo, u.x + u.width / 2, t.y + 4)).toBe(true);
  expect(await hitsAt(undo, u.x + u.width / 2, t.y + t.height - 4)).toBe(true);
});

test('C-M4: yanıt alanındaki hazır yanıt çipleri görünmez kenarla 44 px hedef', async ({ page }) => {
  await row(page, 'Cumartesi kahvaltı?').click();
  await page.getByRole('toolbar', { name: 'Posta eylemleri' }).getByRole('button', { name: 'Yanıtla' }).click();
  const chips = page.getByRole('region', { name: 'Yanıt' }).getByRole('group', { name: 'Hızlı yanıtlar' }).getByRole('button');
  await expect(chips.first()).toBeVisible();
  await settled(chips.first());
  const n = await chips.count();
  expect(n).toBeGreaterThan(0);
  for (let i = 0; i < n; i++) {
    const chip = chips.nth(i);
    const b = (await chip.boundingBox())!;
    if (b.x + b.width > (await page.evaluate(() => innerWidth))) continue;
    const cx = b.x + b.width / 2;
    // Hedef 4 + 32 + 8 = 44 px: başlığa kadar üstte 4, kutuya kadar altta 8 px.
    expect(await hitsAt(chip, cx, b.y - 3.5), `çip ${i} üstü`).toBe(true);
    expect(await hitsAt(chip, cx, b.y + b.height + 7.5), `çip ${i} altı`).toBe(true);
  }
  // Görünür çip 32 px kalır; kenar düzeni (başlıktan 4, kutudan 8 px) değişmez.
  const first = (await chips.first().boundingBox())!;
  expect(Math.round(first.height)).toBe(32);
  const head = (await page.getByRole('button', { name: 'Tam ekran yaz' }).boundingBox())!;
  const box = (await page.locator('.replying .replybox').boundingBox())!;
  expect(Math.round(first.y - (head.y + head.height))).toBe(4);
  expect(Math.round(box.y - (first.y + first.height))).toBe(8);
  // Başlıktaki düğmeler çiplerin kenarıyla örtülmez.
  expect(await hitsAt(page.getByRole('button', { name: 'Tam ekran yaz' }), head.x + head.width / 2, head.y + head.height - 1)).toBe(true);
});

test('C-M5: sürüklenen satır nötr zeminde kalır (basma tonu almaz)', async ({ page }) => {
  const rowEl = list(page).locator('.row', { hasText: 'Cumartesi kahvaltı?' });
  const box = (await rowEl.boundingBox())!;
  const y = box.y + box.height / 2;
  const x0 = box.x + 300;
  await page.mouse.move(x0, y);
  await page.mouse.down();
  // Basılıyken (sürükleme başlamadan) basma tonu var.
  expect(await rowEl.evaluate((el) => el.matches(':active'))).toBe(true);
  await page.mouse.move(x0 - 20, y, { steps: 3 });
  await page.mouse.move(x0 - 90, y, { steps: 4 });
  await expect(page.locator('.under[data-action="archive"]')).toBeVisible();
  // Eşik öncesi: satırın kalan kısmı --surface (açık #F6F7F9).
  await expect.poll(() => rowEl.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe('rgb(246, 247, 249)');
  expect(await rowEl.evaluate((el) => el.matches(':active'))).toBe(true);
  await page.mouse.move(x0, y, { steps: 4 });
  await page.mouse.up();
});

test('C-M6: yazma ekranında Gönder en az 44 px', async ({ page }) => {
  await page.getByRole('button', { name: 'Yeni posta' }).click();
  await expect(h1(page, 'Yeni posta')).toBeVisible();
  const send = page.getByRole('banner').last().getByRole('button', { name: 'Gönder', exact: true });
  expect(await height(send)).toBeGreaterThanOrEqual(44);
  // Uygulama çubuğu (56 px) içinde kalır.
  const bar = (await page.getByRole('banner').last().boundingBox())!;
  const s = (await send.boundingBox())!;
  expect(s.y).toBeGreaterThanOrEqual(bar.y);
  expect(s.y + s.height).toBeLessThanOrEqual(bar.y + bar.height + 0.5);
});

test('C-M7: yüzen düğme etiketi belgelenen tek istisna (200 ms max-width)', async ({ page }) => {
  const label = page.locator('.m-fab-label');
  await expect(label).toBeVisible();
  const tr = await label.evaluate((el) => {
    const s = getComputedStyle(el);
    return { prop: s.transitionProperty, dur: s.transitionDuration };
  });
  expect(tr.prop.split(', ')).toContain('max-width');
  expect(tr.dur.split(', ')[tr.prop.split(', ').indexOf('max-width')]).toBe('0.2s');
});

test('doğrulama turu: kategori çipi görünmez kenarla gerçekten 44 px (çip satırı kırpsa da)', async ({ page }) => {
  // Kenar önce -6 px idi; satır yatay kaydırıcı olduğu için üstte 2 px'e
  // kırpılıyor, 1 px çip kenarı da düşünce hedef 40 px kalıyordu.
  const chips = list(page).getByRole('group', { name: 'Akıllı süzgeç' }).getByRole('button');
  const n = await chips.count();
  expect(n).toBeGreaterThan(2);
  for (let i = 0; i < 3; i++) {
    const chip = chips.nth(i);
    const b = (await chip.boundingBox())!;
    expect(Math.round(b.height)).toBe(32);
    const cx = b.x + b.width / 2;
    expect(await hitsAt(chip, cx, b.y - 1.5), `çip ${i} üstü`).toBe(true);
    expect(await hitsAt(chip, cx, b.y + b.height + 9.5), `çip ${i} altı`).toBe(true);
  }
});

test('C-12 artığı: tek alıcı çipi (yalnız adres) Kime satırını ikiye bölmez; uzun ad üç noktayla kısalır', async ({ page }) => {
  for (const width of [393, 360]) {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('#/main');
    await page.getByRole('button', { name: 'Yeni posta' }).click();
    await expect(h1(page, 'Yeni posta')).toBeVisible();
    const to = page.locator('#compose-to');
    await to.fill('kerem.aydin.uzun.adres@ornek-sirket.com.tr');
    await to.press('Enter');
    await expect(page.getByRole('button', { name: 'kerem.aydin.uzun.adres@ornek-sirket.com.tr alıcısını kaldır' })).toBeVisible();
    const field = page.locator('.field.wrap').first();
    // Tek satır: 56 px satır + 1 px alt çizgi.
    expect(await height(field)).toBeLessThanOrEqual(57);
    // Yazma alanı satırda kalır ve ikinci alıcı için yer bırakır.
    const chip = (await field.locator('.chip').boundingBox())!;
    const input = (await to.boundingBox())!;
    expect(Math.abs(input.y + input.height / 2 - (chip.y + chip.height / 2))).toBeLessThan(3);
    expect(input.width).toBeGreaterThanOrEqual(72);
    // Ad kırpılır, sağ kenara taşma yok.
    expect(chip.x + chip.width).toBeLessThanOrEqual(width);
    await page.keyboard.press('Escape');
  }
});
