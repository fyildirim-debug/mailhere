import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { en } from './en';
import { tr } from './tr';

/**
 * Girdi yer tutucusu da arayüz metnidir: bileşende sabit yazılmaz, sözlükten
 * gelir. Yazma, hesap ekleme, kural düzenleyici ve zaman menüleri taranır.
 */
const FILES = [
  'windows/compose/ComposeWindow.svelte',
  'windows/compose/SendConfirm.svelte',
  'windows/main/Onboarding.svelte',
  'windows/main/settings/RuleEditor.svelte',
  'lib/mail/SnoozeMenu.svelte',
  'lib/mail/FollowUpMenu.svelte',
  'lib/mail/PickTime.svelte',
];

/** Dosyadaki sabit yazılmış yer tutucular: düz öznitelik ya da ifadenin içindeki dolu dizgi. */
function hardcoded(source: string): string[] {
  const out: string[] = [];
  for (const m of source.matchAll(/placeholder="([^"{][^"]*)"/g)) out.push(m[0]);
  for (const m of source.matchAll(/placeholder=\{([^}]*)\}/g)) {
    // Sözlük çağrıları ve karşılaştırılan değerler (`kind === 'from'`) metin değildir.
    const rest = m[1].replace(/\bt\(\s*'[^']*'[^)]*\)/g, '').replace(/[!=]==?\s*'[^']*'/g, '');
    if (/'[^']+'|"[^"]+"|`[^`]+`/.test(rest)) out.push(m[0]);
  }
  return out;
}

describe('yer tutucular sözlükten gelir', () => {
  it('taranan bileşenlerde sabit yazılmış yer tutucu yok', () => {
    for (const file of FILES) {
      const source = readFileSync(join(__dirname, '..', '..', file), 'utf8');
      expect(hardcoded(source), file).toEqual([]);
    }
  });

  it('tarayıcı sabit yazılanı yakalar, sözlükten geleni ve boş dizgiyi geçirir', () => {
    expect(hardcoded('<input placeholder="ad@alanadi.com" />')).toHaveLength(1);
    expect(hardcoded(`<input placeholder={kind === 'from' ? 'ad@alanadi.com' : ''} />`)).toHaveLength(1);
    expect(hardcoded(`<input placeholder={kind === 'from' ? t('placeholder.email') : ''} />`)).toEqual([]);
    expect(hardcoded(`<input placeholder={t('rule.namePlaceholder')} />`)).toEqual([]);
    expect(hardcoded(`<input placeholder={row.list.length ? '' : t('compose.addRecipient')} />`)).toEqual([]);
  });

  it('örnek adres ve sunucu iki dilde tanımlı', () => {
    expect(tr['placeholder.email']).toBe('ad@alanadi.com');
    expect(en['placeholder.email']).toBe('name@example.com');
    expect(tr['placeholder.jmapUrl']).toMatch(/^https:\/\//);
    expect(en['placeholder.jmapUrl']).toMatch(/^https:\/\//);
  });
});
