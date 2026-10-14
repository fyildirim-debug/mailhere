<script lang="ts">
  import { t } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { isMobileShell } from '../../../lib/shell';
  import { settings, updateSettings } from '../../../lib/stores/settings.svelte';

  const s = $derived(settings.value);
  // Android'de anahtar zinciri yerine Keystore vardır; açıklama ona göre.
  const mobile = isMobileShell;

  /** Parmak izinin ilk sekiz baytı; tamamı ipucunda. */
  function shortFp(sha: string): string {
    return sha.split(':').slice(0, 8).join(':') + '…';
  }
</script>

<h2 class="page">{t('set.nav.privacy')}</h2>

<ul class="facts">
  <li><Icon name="lock" size={18} /><span>{t(mobile ? 'mob.settings.db' : 'priv.db')}</span></li>
  <li><Icon name="key" size={18} /><span>{t(mobile ? 'mob.settings.keystore' : 'priv.keyring')}</span></li>
  <li><Icon name="eyeOff" size={18} /><span>{t('priv.remote')}</span></li>
</ul>

<h3 class="set-h">{t('set.remoteAllow')}</h3>
<p class="set-lead">{t('priv.remoteLead')}</p>
<div class="list">
  {#each s.remoteAllow as who (who)}
    <div class="list-row">
      <span class="trunc grow">{who}</span>
      <button class="btn-ghost" onclick={() => updateSettings({ remoteAllow: s.remoteAllow.filter((x) => x !== who) })}>{t('set.remove')}</button>
    </div>
  {:else}
    <span class="meta none">{t('set.remoteNone')}</span>
  {/each}
</div>

<h3 class="set-h">{t('set.trusted')}</h3>
<p class="set-lead">{t('set.trustedHint')}</p>
<div class="list">
  {#each s.trustedCerts as c (`${c.host}:${c.port}`)}
    <div class="list-row">
      <span class="trunc">{c.host}:{c.port}</span>
      <code class="fp" title={c.sha256}>{shortFp(c.sha256)}</code>
      <span class="grow"></span>
      <button class="btn-ghost" onclick={() => updateSettings({ trustedCerts: s.trustedCerts.filter((x) => x !== c) })}>{t('set.remove')}</button>
    </div>
  {:else}
    <span class="meta none">{t('set.trustedNone')}</span>
  {/each}
</div>

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin: 18px 0 8px;
    padding: 0;
    list-style: none;
    color: var(--ink-2);
  }

  .facts li {
    display: flex;
    gap: 12px;
    max-width: 64ch;
  }

  .facts :global(.icon) {
    margin-top: 2px;
    color: var(--ok);
  }

  .list {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--line);
  }

  .list-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 0 4px;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
  }

  .none {
    padding: 10px 4px;
  }

  .fp {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  @container settings (max-width: 600px) {
    .list-row {
      flex-wrap: wrap;
      padding: 6px 4px;
    }
  }
</style>
