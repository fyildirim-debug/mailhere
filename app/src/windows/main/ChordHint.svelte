<script lang="ts">
  // G'ye basılınca sağ altta 12 px ipucu: ikinci tuşlar ve gittikleri yer.
  import { t } from '../../lib/i18n/index.svelte';
  import { GO_KEYS } from '../../lib/util/keys';
</script>

<div class="chord" role="status" aria-label={t('keys.goThen')}>
  <span class="lead"><span class="kbd">G</span>{t('keys.goThen')}</span>
  {#each GO_KEYS as g (g.key)}
    <span class="pair"><span class="kbd">{g.key}</span>{t(g.label)}</span>
  {/each}
</div>

<style>
  .chord {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 45;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 180px;
    padding: 10px 12px;
    border-radius: var(--r-card);
    background: var(--raised);
    box-shadow: var(--shadow);
    font-size: var(--fs-meta);
    color: var(--ink-2);
    animation: chord-in var(--t-state) var(--ease);
  }

  .lead {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
    font-weight: 600;
    color: var(--ink);
  }

  .pair {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  @keyframes chord-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
</style>
