import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { readFileSync } from 'node:fs';

// Uygulama sürümü kök paketten (Tauri paketi ve sürüm etiketiyle aynı).
const version = (JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as { version: string }).version;

// Tauri geliştirme sunucusu sabit portta çalışmalı.
const host = process.env.TAURI_DEV_HOST;
const platform = process.env.TAURI_ENV_PLATFORM;
// Kabuk: Android derlemesi mobil, diğerleri masaüstü; MH_SHELL ile elle seçilir.
const shell = process.env.MH_SHELL ?? (platform === 'android' ? 'mobile' : 'desktop');

export default defineConfig({
  plugins: [svelte()],
  define: { __APP_VERSION__: JSON.stringify(version), __MH_SHELL__: JSON.stringify(shell) },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    // Windows'ta WebView2 ve Android'de WebView (Chromium), Linux'ta WebKitGTK.
    target: platform === 'windows' || platform === 'android' ? 'chrome110' : 'safari15',
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    chunkSizeWarningLimit: 600,
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
