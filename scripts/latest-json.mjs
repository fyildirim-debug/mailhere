#!/usr/bin/env node
// Uygulama içi güncelleyici için `latest.json` üretir.
//
//   node scripts/latest-json.mjs --tag v0.2.0 --repo fyildirim-debug/mailhere \
//     [--notes-file notlar.md | --notes "metin"] [--pub-date 2026-10-20T12:00:00Z] \
//     [--out latest.json] paket-klasoru [paket-klasoru...]
//
// Klasörlerde (alt klasörler dahil) yanında `.sig` imzası olan paketler aranır:
//   linux-x86_64                          → *.AppImage
//   windows-x86_64, windows-x86_64-nsis   → NSIS *-setup.exe
//   windows-x86_64-msi                    → *.msi
// Adresler sürümün GitHub varlıklarını gösterir. Paket adındaki sürüm
// etiketle uyuşmazsa (ör. Cargo.toml sürümü yükseltilmemiş) hata verir.

import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Paket türleri ve güncelleyicinin aradığı platform anahtarları. */
const KINDS = [
  { re: /\.AppImage$/, platforms: ['linux-x86_64'] },
  { re: /-setup\.exe$/, platforms: ['windows-x86_64', 'windows-x86_64-nsis'] },
  { re: /\.msi$/, platforms: ['windows-x86_64-msi'] },
];

/** "v0.2.0" ya da "refs/tags/v0.2.0" → "0.2.0". */
export function versionFromTag(tag) {
  const v = String(tag ?? '')
    .trim()
    .replace(/^refs\/tags\//, '')
    .replace(/^v/, '');
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(v)) {
    throw new Error(`Etiket bir sürüm numarası değil: ${tag}`);
  }
  return v;
}

/** Sürüm varlığının indirme adresi. */
export function assetUrl(repo, tag, name) {
  return `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
}

/** Saniye duyarlı RFC 3339 zaman. */
export function rfc3339(date = new Date()) {
  return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/**
 * Bildirimi kurar.
 * @param {{version: string, tag: string, repo: string, notes?: string, pubDate: string,
 *          files: {name: string, signature: string}[]}} input
 */
export function buildManifest({ version, tag, repo, notes, pubDate, files }) {
  const platforms = {};
  const source = {};
  for (const f of files) {
    const kind = KINDS.find((k) => k.re.test(f.name));
    if (!kind) continue;
    if (!f.name.includes(`_${version}_`)) {
      throw new Error(`Paket sürümü etiketle uyuşmuyor: ${f.name} (beklenen ${version}).`);
    }
    const signature = String(f.signature ?? '').trim();
    if (!signature) throw new Error(`İmza boş: ${f.name}.sig`);
    for (const p of kind.platforms) {
      if (platforms[p]) {
        throw new Error(`Aynı platform için birden çok paket: ${p} (${source[p]}, ${f.name})`);
      }
      platforms[p] = { signature, url: assetUrl(repo, tag, f.name) };
      source[p] = f.name;
    }
  }
  if (!platforms['linux-x86_64'] && !platforms['windows-x86_64']) {
    throw new Error('İmzalı paket bulunamadı (.AppImage.sig ya da -setup.exe.sig yok).');
  }
  const manifest = { version, pub_date: pubDate, platforms };
  const text = String(notes ?? '').trim();
  if (text) manifest.notes = text;
  return manifest;
}

/** Klasörlerdeki imzalı paketler: [{name, signature}]. */
export function findSigned(dirs) {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir).sort()) {
      const path = join(dir, entry);
      if (statSync(path).isDirectory()) {
        walk(path);
      } else if (KINDS.some((k) => k.re.test(entry)) && existsSync(`${path}.sig`)) {
        out.push({ name: basename(path), signature: readFileSync(`${path}.sig`, 'utf8') });
      }
    }
  };
  for (const d of dirs) walk(d);
  return out;
}

function parseArgs(argv) {
  const opts = { dirs: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${a} bir değer bekliyor.`);
      return v;
    };
    if (a === '--tag') opts.tag = value();
    else if (a === '--repo') opts.repo = value();
    else if (a === '--notes') opts.notes = value();
    else if (a === '--notes-file') opts.notesFile = value();
    else if (a === '--pub-date') opts.pubDate = value();
    else if (a === '--out') opts.out = value();
    else if (a.startsWith('--')) throw new Error(`Bilinmeyen seçenek: ${a}`);
    else opts.dirs.push(a);
  }
  if (!opts.tag || !opts.repo || opts.dirs.length === 0) {
    throw new Error('Kullanım: latest-json.mjs --tag vX.Y.Z --repo sahip/depo [--notes-file dosya] [--out latest.json] klasör...');
  }
  return opts;
}

export function main(argv) {
  const opts = parseArgs(argv);
  const notes = opts.notesFile ? readFileSync(opts.notesFile, 'utf8') : opts.notes;
  const manifest = buildManifest({
    version: versionFromTag(opts.tag),
    tag: opts.tag.replace(/^refs\/tags\//, ''),
    repo: opts.repo,
    notes,
    pubDate: opts.pubDate ?? rfc3339(),
    files: findSigned(opts.dirs),
  });
  const json = `${JSON.stringify(manifest, null, 2)}\n`;
  if (opts.out) writeFileSync(opts.out, json);
  else process.stdout.write(json);
  return manifest;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const m = main(process.argv.slice(2));
    console.error(`latest.json: ${m.version} → ${Object.keys(m.platforms).join(', ')}`);
  } catch (e) {
    console.error(`latest.json üretilemedi: ${e.message}`);
    process.exit(1);
  }
}
