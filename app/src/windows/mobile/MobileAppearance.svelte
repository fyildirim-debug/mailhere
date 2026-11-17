<script lang="ts">
  // Ayarlar › Genel'in mobil eki: AMOLED siyah. Koyu temada zeminleri tam
  // siyaha çeker (jetonlar `tokens.css`te); yalnız bu telefonda tutulan kişisel
  // tercihtir (amoled.svelte.ts). Açık temada anahtar çalışır ama etkisi koyu
  // temaya geçince görünür; bunu ipucu söyler.
  import { t } from '../../lib/i18n/index.svelte';
  import { settings } from '../../lib/stores/settings.svelte';
  import { amoled, setAmoled } from './amoled.svelte';

  const light = $derived(settings.resolved !== 'dark');
</script>

<h3 class="set-h">{t('mob.amoled.section')}</h3>
<div class="set-row top">
  <span class="set-name"
    >{t('mob.amoled.name')}<span class="set-hint">{t('mob.amoled.hint')}</span>{#if light}<span class="set-hint">{t('mob.amoled.lightHint')}</span>{/if}</span
  >
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('mob.amoled.name')}
    checked={amoled.on}
    onchange={(e) => setAmoled((e.currentTarget as HTMLInputElement).checked)}
  />
</div>
