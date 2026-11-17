import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { en } from './en';
import { tr } from './tr';

/**
 * Sözlükte kullanılmayan anahtar birikmesin. Bir anahtar, kaynakta tırnak
 * içinde aynen geçiyorsa ya da şablon dizgisiyle kurulan bir önekin
 * (`snooze.${key}` gibi) altındaysa kullanılıyor sayılır.
 */
const SRC = join(__dirname, '..', '..');

/** Bilerek bekleyen anahtarlar: kullanımı yolda olanlar ve henüz karar verilmemiş eskiler. */
const WAITING = new Set([
  // Yolda: daha fazla yükle, tüm ekleri indir.
  'list.loadMore',
  'act.downloadAll',
  // Eski ayar başlıkları; silinip silinmeyeceğine karar verilmedi.
  'set.notifications',
  'set.privacy',
  'set.accounts',
  'set.auth.password',
  'set.auth.google',
  'set.auth.microsoft',
]);

function sources(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) out.push(...sources(path));
    else if (/\.(ts|svelte)$/.test(entry) && !/\.test\.ts$/.test(entry) && !/i18n[\\/](tr|en)\.ts$/.test(path)) out.push(readFileSync(path, 'utf8'));
  }
  return out;
}

function unusedKeys(): string[] {
  const text = sources(SRC).join('\n');
  const quoted = new Set([...text.matchAll(/['"`]([A-Za-z][\w.]*\.[\w.]+)['"`]/g)].map((m) => m[1]));
  const prefixes = [...new Set([...text.matchAll(/`([A-Za-z][\w.]*)\.\$\{/g)].map((m) => `${m[1]}.`))];
  return Object.keys(tr).filter((k) => !quoted.has(k) && !prefixes.some((p) => k.startsWith(p)));
}

describe('kullanılmayan çeviri anahtarları', () => {
  it('bekleyenler dışında kullanılmayan anahtar yok', () => {
    expect(unusedKeys().filter((k) => !WAITING.has(k))).toEqual([]);
  });

  it('silinen anahtarlar iki sözlükte de yok', () => {
    const removed = [
      'toast.sent',
      'compose.sent',
      'toast.draftDeleted',
      'tpl.manage',
      'set.signature',
      'set.signatureHint',
      'list.snoozedUntil',
      'bulk.selectAll',
      'mob.longPress',
      'mob.refresh',
    ];
    for (const k of removed) {
      expect(k in tr, k).toBe(false);
      expect(k in en, k).toBe(false);
    }
  });

  it('bekleyen anahtarlar sözlükte duruyor', () => {
    for (const k of WAITING) expect(k in tr, k).toBe(true);
  });
});
