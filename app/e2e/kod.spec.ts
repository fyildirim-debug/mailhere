import { expect, test } from '@playwright/test';
import { toast, row, CODE, openArchive, state } from './koruma';

// 0.11 (masaüstü): liste satırındaki doğrulama kodu çipi. Örnek posta arşivin
// ilk satırıdır (demo-data.ts).

test('kod çipi: taze kodda görünür, tıklayınca kopyalar ve postayı açmaz; okundu sayar', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.addInitScript(() => {
    (window as unknown as { __popups: number }).__popups = 0;
    const open = window.open.bind(window);
    window.open = ((...a: Parameters<typeof window.open>) => {
      (window as unknown as { __popups: number }).__popups++;
      void open;
      void a;
      return null;
    }) as typeof window.open;
  });
  await openArchive(page);
  await page.evaluate((s) => {
    window.__mh.state.messages.find((m) => m.subject === s)!.unread = true;
    window.__mh.emit('mh:changed', { mailboxIds: [] });
  }, CODE);
  await expect(row(page, CODE)).toHaveClass(/unread/);
  const chip = row(page, CODE).getByRole('button', { name: 'Doğrulama kodu 731 004; kopyala' });
  await expect(chip).toHaveText('731 004');
  // Bir haftalık kod postasında çip yok (kodun ömrü kısadır).
  await expect(row(page, 'Giriş doğrulama kodun').locator('.otp-chip')).toHaveCount(0);

  // Başka bir posta okunurken çipe basılır: okunan posta değişmemeli.
  await row(page, 'Hafta sonu için beş rota').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Hafta sonu için beş rota' })).toBeVisible();
  const opened = page.getByRole('heading', { level: 2, name: CODE });
  await chip.click();
  await expect(toast(page)).toContainText('Kod kopyalandı');
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('731004');
  await expect(opened).toHaveCount(0);
  // "Kopyalayınca okundu say" varsayılan açık.
  await expect(row(page, CODE)).not.toHaveClass(/unread/);
  expect(await state(page, CODE)).toMatchObject({ unread: false });

  // Çift tık hızlı okuma penceresi açmaz; klavyeyle (Enter, Boşluk) de yalnız kopyalar.
  await chip.dblclick();
  await page.evaluate(() => navigator.clipboard.writeText('x'));
  await chip.focus();
  await page.keyboard.press('Enter');
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('731004');
  await page.evaluate(() => navigator.clipboard.writeText('x'));
  await page.keyboard.press('Space');
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('731004');
  await expect(opened).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { __popups: number }).__popups)).toBe(0);
});

test('kod çipi: satır yüksekliği değişmez, dar listede taşmaz; sıkı görünümde satır eylemleri çipi örtmez', async ({ page }) => {
  await openArchive(page);
  const withChip = row(page, CODE);
  const plain = row(page, 'Hafta sonu için beş rota');
  const chip = withChip.locator('.otp-chip');
  const overlap = (a: { x: number; y: number; width: number; height: number }, b: { x: number; y: number; width: number; height: number }) =>
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

  for (const density of ['comfortable', 'compact'] as const) {
    await page.evaluate((d) => window.__mh.call('settings_set', { settings: { ...window.__mh.state.settings, density: d } }), density);
    await expect(withChip).toHaveClass(density === 'compact' ? /compact/ : /row/);
    // Varsayılan genişlik ve en dar liste (ayırıcıda Home: 320 px).
    for (const width of [404, 320]) {
      const sep = page.getByRole('separator', { name: 'Posta listesi genişliği' });
      await sep.focus();
      await page.keyboard.press(width === 320 ? 'Home' : 'Enter');
      await expect(sep).toHaveAttribute('aria-valuenow', String(width));
      const r = (await withChip.boundingBox())!;
      const p = (await plain.boundingBox())!;
      const c = (await chip.boundingBox())!;
      expect(r.height, `${density} ${width}`).toBe(p.height);
      expect(c.x + c.width, `${density} ${width}`).toBeLessThanOrEqual(r.x + r.width);
      expect(c.y + c.height, `${density} ${width}`).toBeLessThanOrEqual(r.y + r.height);
      expect(c.y, `${density} ${width}`).toBeGreaterThanOrEqual(r.y);
      // Çipin solundaki metin çipin altına girmez (görünmez eşi yer ayırır).
      const text = (await withChip.locator('.last .trunc').boundingBox())!;
      expect(text.x + text.width, `${density} ${width}`).toBeLessThanOrEqual(c.x);
      expect(await withChip.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      // Üzerine gelince çıkan satır eylemleri çipi örtmez; çip tıklanabilir kalır.
      await withChip.hover();
      const acts = withChip.locator('.acts');
      await expect(acts).toHaveCSS('opacity', '1');
      expect(overlap((await acts.boundingBox())!, (await chip.boundingBox())!), `${density} ${width}`).toBe(false);
      await page.mouse.move(0, 0);
    }
  }
});
