import { existsSync } from 'node:fs';
import { defineConfig, devices } from '@playwright/test';

// Kurulu Chromium (bulut ortamı) varsa onu kullan; CI kendi tarayıcısını indirir.
const localChromium = '/opt/pw-browsers/chromium';
const executablePath = process.env.PW_CHROMIUM ?? (existsSync(localChromium) ? localChromium : undefined);
// Aynı makinede birden çok çalışma ağacı test ediliyorsa her biri kendi kapısını alır (PW_PORT).
const port = Number(process.env.PW_PORT ?? 4173);
// Mobil kabuk testleri `mobil-*.spec.ts` dosyalarındadır; masaüstü projesi onları atlar.
const mobileSpecs = /mobil-.*\.spec\.ts$/;

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : 'list',
  use: {
    baseURL: `http://localhost:${port}`,
    locale: 'tr-TR',
    timezoneId: 'Europe/Istanbul',
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      testIgnore: mobileSpecs,
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, launchOptions: { executablePath } },
    },
    {
      // Mobil kabuk: telefon ölçüleri ve dokunma; `?kabuk=mobil` kabuğu seçer
      // (göreli `page.goto('#/main')` sorguyu korur).
      name: 'mobil',
      testMatch: mobileSpecs,
      use: { ...devices['Pixel 7'], baseURL: `http://localhost:${port}/?kabuk=mobil`, launchOptions: { executablePath } },
    },
  ],
  webServer: {
    command: `pnpm build && pnpm exec vite preview --port ${port} --strictPort`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
