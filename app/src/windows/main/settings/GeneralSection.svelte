<script lang="ts">
  import type { Density, Language, Theme } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import { settings, updateSettings } from '../../../lib/stores/settings.svelte';

  const s = $derived(settings.value);

  const themes: [Theme, MessageKey][] = [
    ['system', 'set.theme.system'],
    ['light', 'set.theme.light'],
    ['dark', 'set.theme.dark'],
  ];
  const languages: [Language, string][] = [
    ['tr', 'Türkçe'],
    ['en', 'English'],
  ];
  const densities: [Density, MessageKey][] = [
    ['comfortable', 'set.density.comfortable'],
    ['compact', 'set.density.compact'],
  ];
</script>

<h2 class="page">{t('set.nav.general')}</h2>

<h3 class="set-h">{t('set.appearance')}</h3>
<div class="set-row">
  <span class="set-name">{t('set.theme')}</span>
  <div class="seg-track" role="radiogroup" aria-label={t('set.theme')}>
    {#each themes as [v, k] (v)}
      <button class="seg" role="radio" aria-checked={s.theme === v} onclick={() => updateSettings({ theme: v })}>{t(k)}</button>
    {/each}
  </div>
</div>
<div class="set-row top">
  <span class="set-name">{t('set.darkMail')}<span class="set-hint">{t('set.darkMailHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('set.darkMail')}
    checked={s.darkMail}
    onchange={(e) => updateSettings({ darkMail: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>
<div class="set-row">
  <span class="set-name">{t('set.density')}</span>
  <div class="seg-track" role="radiogroup" aria-label={t('set.density')}>
    {#each densities as [v, k] (v)}
      <button class="seg" role="radio" aria-checked={s.density === v} onclick={() => updateSettings({ density: v })}>{t(k)}</button>
    {/each}
  </div>
</div>

<h3 class="set-h">{t('set.languageTitle')}</h3>
<div class="set-row">
  <span class="set-name">{t('set.language')}</span>
  <div class="seg-track" role="radiogroup" aria-label={t('set.language')}>
    {#each languages as [v, label] (v)}
      <button class="seg" role="radio" aria-checked={s.language === v} onclick={() => updateSettings({ language: v })}>{label}</button>
    {/each}
  </div>
</div>

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .set-h:first-of-type {
    margin-top: 20px;
  }
</style>
