import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * Windows dosya sistemi büyük-küçük harf ayırmaz: `./triage.svelte` içe
 * aktarması orada `triage.svelte.ts` modülü yerine `Triage.svelte`
 * bileşenine çözülür ve derleme yalnız Windows'ta kırılır. Aynı klasörde
 * yalnız harf büyüklüğüyle ayrılan içe aktarma adları olmamalı.
 */
function importNames(file: string): string[] {
  // `x.svelte.ts` modülü `./x.svelte` diye içe aktarılır.
  const names = [file];
  if (file.endsWith('.svelte.ts')) names.push(file.slice(0, -'.ts'.length));
  return names;
}

function collisions(dir: string): string[] {
  const out: string[] = [];
  const seen = new Map<string, string>();
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      if (entry !== 'node_modules') out.push(...collisions(path));
      continue;
    }
    for (const name of importNames(entry)) {
      const key = name.toLowerCase();
      const prev = seen.get(key);
      if (prev !== undefined && prev !== entry) out.push(`${dir}: ${prev} ↔ ${entry}`);
      else seen.set(key, entry);
    }
  }
  return out;
}

describe('dosya adları', () => {
  it('aynı klasörde yalnız harf büyüklüğüyle ayrılan içe aktarma adı yok', () => {
    expect(collisions(join(__dirname, '..'))).toEqual([]);
  });
});
