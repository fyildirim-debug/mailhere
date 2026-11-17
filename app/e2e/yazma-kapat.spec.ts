import { expect, test, type Page } from '@playwright/test';
import type { MockBackend } from '../src/lib/api/mock';

declare global {
  interface Window {
    __mh: MockBackend;
    __log: string[];
  }
}

// Yazma penceresi başlık çubuğundaki kapat (X): son değişiklik taslağa
// yazıldıktan SONRA kapanır; kayıt olmazsa pencere açık kalır ve nedenini
// söyler, ikinci kapatış kaydetmeden kapatır.

const drafts = (page: Page, subject: string) => page.evaluate((s) => window.__mh.state.messages.filter((m) => m.subject === s).length, subject);

/** Pencere kapanışını ve taslak kaydını sıraya yazar; kayıt `delay` ms sürer, `fail` ise hata verir. */
async function spy(page: Page, opts: { delay: number; fail?: boolean }) {
  await page.evaluate(({ delay, fail }) => {
    window.__log = [];
    window.close = () => void window.__log.push('close');
    const call = window.__mh.call.bind(window.__mh) as (name: string, args?: unknown) => Promise<unknown>;
    (window.__mh as unknown as { call: typeof call }).call = async (name, args) => {
      if (name !== 'draft_save') return call(name, args);
      await new Promise((r) => setTimeout(r, delay));
      if (fail) throw { code: 'network', message: 'Sunucuya ulaşılamadı.' };
      const id = await call(name, args);
      window.__log.push('saved');
      return id;
    };
  }, opts);
}

async function open(page: Page) {
  await page.goto('/#/compose/new/0');
  await expect(page.locator('#compose-to')).toBeFocused();
}

test('kapat: son değişiklik taslağa yazılır, pencere ondan sonra kapanır', async ({ page }) => {
  await open(page);
  await spy(page, { delay: 400 });
  await page.locator('#compose-subject').fill('Kapanış denemesi');
  // Kendiliğinden kayıt (1,5 sn) beklenmeden kapatılır.
  await page.getByRole('button', { name: 'Kapat', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__log)).toEqual(['saved', 'close']);
  expect(await drafts(page, 'Kapanış denemesi')).toBe(1);
});

test('kapat: değişiklik yoksa kayıt beklenmez, taslak oluşmaz', async ({ page }) => {
  await open(page);
  await spy(page, { delay: 400 });
  const before = await page.evaluate(() => window.__mh.state.messages.length);
  await page.getByRole('button', { name: 'Kapat', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__log)).toEqual(['close']);
  expect(await page.evaluate(() => window.__mh.state.messages.length)).toBe(before);
});

test('kapat: kayıt olmazsa pencere açık kalır ve nedenini söyler; ikinci kapatış kapatır', async ({ page }) => {
  await open(page);
  await spy(page, { delay: 50, fail: true });
  await page.locator('#compose-subject').fill('Kaydedilemeyen');
  const close = page.getByRole('button', { name: 'Kapat', exact: true });
  await close.click();
  const alert = page.getByRole('alert');
  await expect(alert).toContainText('Son değişiklikler taslağa yazılamadı; pencere açık bırakıldı.');
  await expect(alert).toContainText('Yeniden kapatırsan bu değişiklikler kaybolur.');
  await expect(alert).toContainText('Sunucuya ulaşılamadı.');
  expect(await page.evaluate(() => window.__log)).toEqual([]);
  // Yazılan yerinde duruyor.
  await expect(page.locator('#compose-subject')).toHaveValue('Kaydedilemeyen');

  await close.click();
  await expect.poll(() => page.evaluate(() => window.__log)).toEqual(['close']);
});
