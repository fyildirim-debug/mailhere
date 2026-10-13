// `node --test "scripts/*.test.mjs"` ile çalışır.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { assetUrl, buildManifest, rfc3339, versionFromTag } from './latest-json.mjs';

const REPO = 'fyildirim-debug/mailhere';
const BASE = `https://github.com/${REPO}/releases/download/v0.2.0`;

test('etiketten sürüm', () => {
  assert.equal(versionFromTag('v0.2.0'), '0.2.0');
  assert.equal(versionFromTag('refs/tags/v1.10.3-beta.1'), '1.10.3-beta.1');
  assert.equal(versionFromTag('0.2.0'), '0.2.0');
  assert.throws(() => versionFromTag('surum-iki'), /sürüm numarası değil/);
  assert.throws(() => versionFromTag('v0.2'), /sürüm numarası değil/);
});

test('adres ve zaman biçimi', () => {
  assert.equal(assetUrl(REPO, 'v0.2.0', 'Mailhere_0.2.0_amd64.AppImage'), `${BASE}/Mailhere_0.2.0_amd64.AppImage`);
  assert.equal(rfc3339(new Date(Date.UTC(2026, 9, 20, 12, 0, 0, 345))), '2026-10-20T12:00:00Z');
});

test('bildirim biçimi', () => {
  const m = buildManifest({
    version: '0.2.0',
    tag: 'v0.2.0',
    repo: REPO,
    notes: '  JMAP ve Google girişi.\n',
    pubDate: '2026-10-20T12:00:00Z',
    files: [
      { name: 'Mailhere_0.2.0_amd64.AppImage', signature: 'c2lnLWxpbnV4\n' },
      { name: 'Mailhere_0.2.0_x64-setup.exe', signature: 'c2lnLW5zaXM=' },
      { name: 'Mailhere_0.2.0_x64_en-US.msi', signature: 'c2lnLW1zaQ==' },
      { name: 'Mailhere_0.2.0_amd64.deb', signature: 'yok-sayilir' },
    ],
  });
  assert.deepEqual(m, {
    version: '0.2.0',
    pub_date: '2026-10-20T12:00:00Z',
    notes: 'JMAP ve Google girişi.',
    platforms: {
      'linux-x86_64': { signature: 'c2lnLWxpbnV4', url: `${BASE}/Mailhere_0.2.0_amd64.AppImage` },
      'windows-x86_64': { signature: 'c2lnLW5zaXM=', url: `${BASE}/Mailhere_0.2.0_x64-setup.exe` },
      'windows-x86_64-nsis': { signature: 'c2lnLW5zaXM=', url: `${BASE}/Mailhere_0.2.0_x64-setup.exe` },
      'windows-x86_64-msi': { signature: 'c2lnLW1zaQ==', url: `${BASE}/Mailhere_0.2.0_x64_en-US.msi` },
    },
  });
});

test('hatalı girdiler', () => {
  const base = { version: '0.2.0', tag: 'v0.2.0', repo: REPO, pubDate: '2026-10-20T12:00:00Z' };
  assert.throws(
    () => buildManifest({ ...base, files: [{ name: 'Mailhere_0.1.0_amd64.AppImage', signature: 'x' }] }),
    /etiketle uyuşmuyor/,
  );
  assert.throws(() => buildManifest({ ...base, files: [{ name: 'Mailhere_0.2.0_amd64.AppImage', signature: ' ' }] }), /İmza boş/);
  assert.throws(() => buildManifest({ ...base, files: [] }), /İmzalı paket bulunamadı/);
  assert.throws(
    () =>
      buildManifest({
        ...base,
        files: [
          { name: 'Mailhere_0.2.0_amd64.AppImage', signature: 'a' },
          { name: 'Baska_0.2.0_amd64.AppImage', signature: 'b' },
        ],
      }),
    /birden çok paket/,
  );
  // Notsuz sürümde alan yazılmaz.
  const m = buildManifest({ ...base, notes: '  ', files: [{ name: 'Mailhere_0.2.0_amd64.AppImage', signature: 'a' }] });
  assert.equal('notes' in m, false);
  assert.deepEqual(Object.keys(m.platforms), ['linux-x86_64']);
});

test('komut satırı: klasörleri tarar, imzasız paketi atlar', () => {
  const dir = mkdtempSync(join(tmpdir(), 'mh-latest-'));
  try {
    mkdirSync(join(dir, 'appimage'), { recursive: true });
    mkdirSync(join(dir, 'nsis'), { recursive: true });
    mkdirSync(join(dir, 'deb'), { recursive: true });
    writeFileSync(join(dir, 'appimage', 'Mailhere_0.2.0_amd64.AppImage'), 'ikili');
    writeFileSync(join(dir, 'appimage', 'Mailhere_0.2.0_amd64.AppImage.sig'), 'c2lnLWxpbnV4');
    writeFileSync(join(dir, 'nsis', 'Mailhere_0.2.0_x64-setup.exe'), 'ikili');
    writeFileSync(join(dir, 'nsis', 'Mailhere_0.2.0_x64-setup.exe.sig'), 'c2lnLW5zaXM=');
    writeFileSync(join(dir, 'deb', 'Mailhere_0.2.0_amd64.deb'), 'ikili');
    writeFileSync(join(dir, 'notlar.md'), 'Yenilikler\n\n- Taslaklar\n');
    const out = join(dir, 'latest.json');
    const script = fileURLToPath(new URL('./latest-json.mjs', import.meta.url));
    const r = spawnSync(
      process.execPath,
      [script, '--tag', 'v0.2.0', '--repo', REPO, '--notes-file', join(dir, 'notlar.md'), '--out', out, dir],
      { encoding: 'utf8' },
    );
    assert.equal(r.status, 0, r.stderr);
    const m = JSON.parse(readFileSync(out, 'utf8'));
    assert.equal(m.version, '0.2.0');
    assert.equal(m.notes, 'Yenilikler\n\n- Taslaklar');
    assert.match(m.pub_date, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
    assert.deepEqual(Object.keys(m.platforms).sort(), ['linux-x86_64', 'windows-x86_64', 'windows-x86_64-nsis']);
    assert.equal(m.platforms['linux-x86_64'].url, `${BASE}/Mailhere_0.2.0_amd64.AppImage`);

    const bad = spawnSync(process.execPath, [script, '--tag', 'v0.3.0', '--repo', REPO, dir], { encoding: 'utf8' });
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /etiketle uyuşmuyor/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
