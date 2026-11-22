// Microsoft Store (MSIX) paketinin logo varlıklarını üretir:
//   node scripts/msix-varliklar.mjs
// Kaynak tek dosyadır (src-tauri/icons/app-icon.svg); çıktı
// src-tauri/msix/Assets altına yazılır ve depoya girer. İkon değişince
// yeniden çalıştırılır. Çizim için Playwright'ın Chromium'u kullanılır
// (arayüz testleriyle aynı bağımlılık; ek paket yok).
//
// Adlardaki niteleyiciler (scale-200, targetsize-32_altform-unplated…)
// Windows'un kaynak dizini içindir: paketleme betiği `makepri` ile
// resources.pri üretir, kabuk ekran ölçeğine ve yüzeye uyan dosyayı seçer.

import { mkdirSync, readFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(root, 'app', 'package.json'));
const { chromium } = require('@playwright/test');

const svg = readFileSync(join(root, 'src-tauri', 'icons', 'app-icon.svg'), 'utf8');
const out = join(root, 'src-tauri', 'msix', 'Assets');
const SCALES = [100, 125, 150, 200, 400];

/** [dosya adı, genişlik, yükseklik, ikonun kısa kenara oranı] */
const files = [];
for (const s of SCALES) {
  const k = s / 100;
  // Görev çubuğu ve Başlat listesi: ikon neredeyse tam boy.
  files.push([`Square44x44Logo.scale-${s}.png`, 44 * k, 44 * k, 0.9]);
  // Kutucuklar: ikon ortada, çevresinde boşluk.
  files.push([`Square71x71Logo.scale-${s}.png`, 71 * k, 71 * k, 0.7]);
  files.push([`Square150x150Logo.scale-${s}.png`, 150 * k, 150 * k, 0.6]);
  files.push([`Wide310x150Logo.scale-${s}.png`, 310 * k, 150 * k, 0.6]);
  files.push([`StoreLogo.scale-${s}.png`, 50 * k, 50 * k, 0.9]);
}
// Zeminsiz küçük ikonlar (görev çubuğu, Alt Sekme, dosya gezgini): tam boy.
for (const n of [16, 24, 32, 48, 256]) {
  files.push([`Square44x44Logo.targetsize-${n}.png`, n, n, 1]);
  files.push([`Square44x44Logo.targetsize-${n}_altform-unplated.png`, n, n, 1]);
  files.push([`Square44x44Logo.targetsize-${n}_altform-lightunplated.png`, n, n, 1]);
}

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const executablePath = process.env.PW_CHROMIUM ?? '/opt/pw-browsers/chromium';
const browser = await chromium.launch({ executablePath }).catch(() => chromium.launch());
const page = await browser.newPage();
for (const [name, w, h, ratio] of files) {
  const width = Math.round(w);
  const height = Math.round(h);
  const side = Math.round(Math.min(width, height) * ratio);
  await page.setViewportSize({ width, height });
  await page.setContent(
    `<style>html,body{margin:0;height:100%;background:transparent}body{display:flex;align-items:center;justify-content:center}svg{width:${side}px;height:${side}px;display:block}</style>${svg}`,
  );
  await page.screenshot({ path: join(out, name), omitBackground: true, clip: { x: 0, y: 0, width, height } });
}
await browser.close();
console.log(`${files.length} dosya yazıldı: ${out}`);
