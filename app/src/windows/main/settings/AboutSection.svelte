<script lang="ts">
  import { openExternal } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { isMobileShell } from '../../../lib/shell';
  import { settings, updateSettings } from '../../../lib/stores/settings.svelte';
  import { checkUpdate, installUpdate, update } from '../../../lib/stores/update.svelte';

  const s = $derived(settings.value);
  const version = __APP_VERSION__;
  // Mobilde imzalı güncelleyici ve klavye kısayolu yoktur: "Hakkında" sürümü ve
  // yeni sürümlerin indirildiği sayfayı gösterir.
  const mobile = isMobileShell;
  const RELEASES_URL = 'https://github.com/fyildirim-debug/mailheremobile/releases';

  const keys: [string, MessageKey][] = [
    ['C', 'keys.compose'],
    ['R', 'keys.reply'],
    ['A', 'keys.replyAll'],
    ['F', 'keys.forward'],
    ['E', 'keys.archive'],
    ['#', 'keys.delete'],
    ['H', 'keys.snooze'],
    ['S', 'keys.star'],
    ['U', 'keys.unread'],
    ['J / K', 'keys.move'],
    ['Enter', 'keys.open'],
    ['Ctrl K', 'keys.palette'],
    ['Ctrl Z', 'keys.undo'],
    ['Ctrl ,', 'keys.settings'],
  ];
</script>

{#if mobile}
  <h2 class="page">{t('mob.settings.about')}</h2>
  <h3 class="set-h">{t('set.version', { version })}</h3>
  <p class="set-lead">{t('set.license')}</p>
  <p class="set-lead">{t('mob.settings.aboutLead')}</p>
  <button class="btn-soft release" onclick={() => openExternal(RELEASES_URL)}><Icon name="external" size={16} />{t('mob.settings.release')}</button>
{:else}
<h2 class="page">{t('set.nav.about')}</h2>

<h3 class="set-h">{t('set.updates')}</h3>
<div class="set-row top">
  <span class="set-name">{t('set.autoUpdate')}<span class="set-hint">{t('set.autoUpdateHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('set.autoUpdate')}
    checked={s.autoUpdate}
    onchange={(e) => updateSettings({ autoUpdate: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>
<div class="set-row top">
  <span class="set-name">{t('set.version', { version })}<span class="set-hint">{t('set.license')}</span></span>
  <div class="upd">
    {#if update.info}
      <div class="upd-new">
        <strong>{t('update.available', { version: update.info.version })}</strong>
        {#if update.info.notes}<span class="meta">{update.info.notes}</span>{/if}
      </div>
      <button class="btn-primary small" disabled={update.installing} onclick={installUpdate}>
        {update.installing ? t('update.installing') : t('update.install')}
      </button>
    {:else}
      <button class="btn-soft" disabled={update.checking} onclick={() => checkUpdate()}>
        <Icon name="refresh" size={16} />{update.checking ? t('set.checking') : t('set.checkUpdate')}
      </button>
      {#if update.upToDate}<span class="meta ok"><Icon name="check" size={14} />{t('set.upToDate')}</span>{/if}
    {/if}
    {#if update.error}<span class="set-error" role="alert">{update.error}</span>{/if}
  </div>
</div>

<h3 class="set-h">{t('keys.title')}</h3>
<dl class="keys">
  {#each keys as [k, label] (k)}
    <dt><span class="kbd">{k}</span></dt>
    <dd>{t(label)}</dd>
  {/each}
</dl>
{/if}

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .upd {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
  }

  .upd-new {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex-basis: 100%;
  }

  .release {
    margin-top: 12px;
  }

  .ok {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ok);
  }

  .keys {
    display: grid;
    grid-template-columns: 96px minmax(0, 1fr) 96px minmax(0, 1fr);
    gap: 10px 16px;
    margin: 12px 0 0;
    align-items: center;
  }

  dd {
    margin: 0;
    color: var(--ink-2);
  }

  @media (max-width: 900px) {
    .keys {
      grid-template-columns: 96px minmax(0, 1fr);
    }
  }
</style>
