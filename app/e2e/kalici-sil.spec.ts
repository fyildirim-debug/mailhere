import { expect, test } from '@playwright/test';
import { list, toast, row, SUSPECT, token, openJunk, openTrash, state, trashBySubject } from './koruma';

// 0.11 (masaüstü): Çöp ve İstenmeyen'de onaylı "Kalıcı sil" (kısayol, çubuk,
// satır, bağlam menüsü, toplu panel, odak kipi) ve kutuyu boşaltma.

test('Çöp: Sil "Kalıcı sil" olur ve onay ister; Esc vazgeçer, Enter siler; geri alma yok', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await trashBySubject(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır']);
  await openTrash(page);
  await row(page, 'Cumartesi kahvaltı?').click();
  await expect(page.getByRole('heading', { level: 2, name: 'Cumartesi kahvaltı?' })).toBeVisible();
  const bar = page.locator('.reader .toolbar');
  await expect(bar.getByRole('button', { name: 'Kalıcı sil' })).toBeVisible();
  await expect(bar.getByRole('button', { name: 'Sil', exact: true })).toHaveCount(0);

  // Kısayol: onay penceresi; odak onay düğmesinde, Sekme iki düğme arasında döner.
  await page.keyboard.press('Delete');
  const dialog = page.getByRole('alertdialog', { name: 'Bu posta kalıcı silinsin mi?' });
  await expect(dialog).toContainText('Bu işlem geri alınamaz.');
  const go = dialog.getByRole('button', { name: 'Kalıcı sil' });
  const cancel = dialog.getByRole('button', { name: 'Vazgeç' });
  await expect(go).toBeFocused();
  expect(await go.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(await token(page, '--danger'));
  await page.keyboard.press('Tab');
  await expect(cancel).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(go).toBeFocused();
  // Pencere açıkken tek tuşlu kısayollar arkada çalışmaz.
  await page.keyboard.press('e');
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(row(page, 'Cumartesi kahvaltı?')).toBeVisible();
  expect(await state(page, 'Cumartesi kahvaltı?')).toMatchObject({ role: 'trash' });

  // Odak Vazgeç'teyken Enter vazgeçer (odaktaki düğme çalışır).
  await page.keyboard.press('Delete');
  await page.keyboard.press('Tab');
  await page.keyboard.press('Enter');
  await expect(dialog).toHaveCount(0);
  expect(await state(page, 'Cumartesi kahvaltı?')).toMatchObject({ role: 'trash' });

  // Enter onaylar: posta sunucudan da silinir, "Geri al" sunulmaz.
  await page.keyboard.press('Delete');
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(toast(page)).toContainText('Posta kalıcı silindi');
  await expect(toast(page).getByRole('button', { name: 'Geri al' })).toHaveCount(0);
  await expect(row(page, 'Cumartesi kahvaltı?')).toHaveCount(0);
  expect(await state(page, 'Cumartesi kahvaltı?')).toBeNull();
  // Sıradaki posta seçilir ve yerinde durur.
  expect(await state(page, 'Sözleşme imzaya hazır')).toMatchObject({ role: 'trash' });
});

test('Çöp: satır düğmesi, bağlam menüsü ve toplu panel kalıcı siler; gelen kutusunda Sil çöpe taşır', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  // Gelen kutusu: Sil eskisi gibi çöpe taşır ve geri alınır.
  await row(page, 'Cumartesi kahvaltı?').click();
  await page.keyboard.press('Delete');
  await expect(page.getByRole('alertdialog')).toHaveCount(0);
  await expect(toast(page)).toContainText('Çöp kutusuna taşındı');
  await expect(toast(page).getByRole('button', { name: 'Geri al' })).toBeVisible();

  await trashBySubject(page, ['Sözleşme imzaya hazır', 'Ekim faturanız hazır', 'Gönderiniz dağıtıma çıktı']);
  await openTrash(page);
  await expect(list(page).locator('.row')).toHaveCount(4);

  // Bağlam menüsü.
  await row(page, 'Cumartesi kahvaltı?').click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Cumartesi kahvaltı?' });
  await expect(menu.getByRole('menuitem', { name: /^Sil/ })).toHaveCount(0);
  await menu.getByRole('menuitem', { name: /^Kalıcı sil/ }).click();
  const one = page.getByRole('alertdialog', { name: 'Bu posta kalıcı silinsin mi?' });
  await one.getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(toast(page)).toContainText('Posta kalıcı silindi');
  await expect(list(page).locator('.row')).toHaveCount(3);

  // Satırın üzerine gelince çıkan düğme.
  await row(page, 'Sözleşme imzaya hazır').hover();
  await row(page, 'Sözleşme imzaya hazır').getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(one).toBeVisible();
  await one.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(list(page).locator('.row')).toHaveCount(3);

  // Toplu panel: sayı başlıkta yazar.
  await row(page, 'Sözleşme imzaya hazır').click();
  await page.keyboard.press('Control+a');
  const panel = page.getByRole('main', { name: '3 posta seçili' });
  await expect(panel.getByRole('button', { name: /^Sil/ })).toHaveCount(0);
  await panel.getByRole('button', { name: /^Kalıcı sil/ }).click();
  const many = page.getByRole('alertdialog', { name: '3 posta kalıcı silinsin mi?' });
  await expect(many).toContainText('Bu işlem geri alınamaz.');
  await many.getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(toast(page)).toContainText('3 posta kalıcı silindi');
  await expect(list(page).getByText('Çöp boş')).toBeVisible();
  expect(await page.evaluate(() => window.__mh.state.messages.filter((m) => m.mailboxId.toLowerCase().includes('trash') || m.mailboxId.includes('Çöp')).length)).toBe(0);
});

test('Çöpü boşalt: onayda sayı yazar; boşaltınca liste boş durumu gösterir, düğme kalkar', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  // Gelen kutusunda "boşalt" yoktur.
  await expect(list(page).getByRole('button', { name: /boşalt/ })).toHaveCount(0);
  await trashBySubject(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır', 'Ekim faturanız hazır']);
  await openTrash(page);
  const empty = list(page).getByRole('button', { name: 'Çöpü boşalt' });
  await empty.click();
  const dialog = page.getByRole('alertdialog', { name: 'Çöpteki 3 posta kalıcı silinsin mi?' });
  await expect(dialog).toContainText('Bu işlem geri alınamaz.');
  // Dışarı tıklamak vazgeçer; odak düğmeye döner.
  await page.mouse.click(10, 300);
  await expect(dialog).toHaveCount(0);
  await expect(empty).toBeFocused();
  await expect(list(page).locator('.row')).toHaveCount(3);

  await empty.click();
  await dialog.getByRole('button', { name: 'Çöpü boşalt' }).click();
  await expect(toast(page)).toContainText('Çöp boşaltıldı · 3 posta kalıcı silindi');
  await expect(list(page).getByText('Çöp boş')).toBeVisible();
  await expect(empty).toHaveCount(0);
});

test("İstenmeyen'i boşalt yalnız o hesabın kutusunu boşaltır", async ({ page }) => {
  await openJunk(page);
  await expect(row(page, SUSPECT)).toBeVisible();
  // Başka hesabın İstenmeyen kutusuna da posta koy: ona dokunulmamalı.
  const other = await page.evaluate(() => {
    const mh = window.__mh;
    const box = mh.state.mailboxes.find((b) => b.role === 'junk' && b.accountId !== 'demo-kisisel');
    const m = mh.state.messages.find((x) => box && x.accountId === box.accountId);
    if (!box || !m) return null;
    m.mailboxId = box.id;
    return m.subject;
  });
  await list(page).getByRole('button', { name: "İstenmeyen'i boşalt" }).click();
  const dialog = page.getByRole('alertdialog');
  await expect(dialog.getByRole('heading')).toHaveText('İstenmeyen kutusundaki 1 posta kalıcı silinsin mi?');
  await dialog.getByRole('button', { name: "İstenmeyen'i boşalt" }).click();
  await expect(toast(page)).toContainText('İstenmeyen boşaltıldı · 1 posta kalıcı silindi');
  expect(await state(page, SUSPECT)).toBeNull();
  if (other) expect(await state(page, other)).toMatchObject({ role: 'junk' });
});

test('odak kipi: Çöp’te Sil onay ister, onaylanınca sıradakine geçer', async ({ page }) => {
  await page.goto('/#/main');
  await expect(list(page).getByText('Cumartesi kahvaltı?')).toBeVisible();
  await trashBySubject(page, ['Cumartesi kahvaltı?', 'Sözleşme imzaya hazır']);
  await openTrash(page);
  await page.keyboard.press('Shift+O');
  const focus = page.getByRole('region', { name: 'Odak kipi' });
  await expect(focus.getByRole('status')).toHaveText('1/2');
  const first = await page.getByRole('heading', { level: 2 }).textContent();
  await focus.getByRole('button', { name: /^Kalıcı sil/ }).click();
  const dialog = page.getByRole('alertdialog', { name: 'Bu posta kalıcı silinsin mi?' });
  await dialog.getByRole('button', { name: 'Vazgeç' }).click();
  await expect(focus.getByRole('status')).toHaveText('1/2');
  await page.keyboard.press('Delete');
  await dialog.getByRole('button', { name: 'Kalıcı sil' }).click();
  await expect(toast(page)).toContainText('Posta kalıcı silindi');
  await expect(focus.getByRole('status')).toHaveText('2/2');
  await expect(page.getByRole('heading', { level: 2 })).not.toHaveText(first!);
  expect(await state(page, first!)).toBeNull();
});
