<script lang="ts">
  import type { Account } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { mail, openSettings } from '../../../lib/stores/mail.svelte';

  function authLabel(a: Account): string {
    return a.kind === 'jmap' ? 'JMAP' : t(`acc.via.${a.auth}` as MessageKey);
  }

  function state(a: Account): { text: string; tone: 'ok' | 'busy' | 'warn' } {
    const s = mail.sync[a.id];
    // Henüz durum gelmediyse kenar çubuğundaki genel durum gibi bağlı sayılır.
    if (!s) return { text: t('acc.state.online'), tone: 'ok' };
    switch (s.state) {
      case 'online':
        return { text: t('acc.state.online'), tone: 'ok' };
      case 'syncing':
        return { text: t('sync.syncing'), tone: 'busy' };
      case 'offline':
        return { text: t('sync.offline'), tone: 'warn' };
      case 'untrusted':
        return { text: t('sync.untrusted', { host: s.host }), tone: 'warn' };
      default:
        return { text: s.message, tone: 'warn' };
    }
  }
</script>

<h2 class="page">{t('set.nav.accounts')}</h2>
<p class="set-lead">{t('acc.lead')}</p>

<div class="list">
  {#each mail.accounts as a (a.id)}
    {@const st = state(a)}
    <button class="acc" onclick={() => openSettings(`account:${a.id}`)}>
      <span class="badge" style:--c={a.color}>{a.name.slice(0, 1).toLocaleUpperCase('tr')}</span>
      <span class="text">
        <span class="trunc name">{a.name}</span>
        <span class="trunc meta">{a.email} · {authLabel(a)}</span>
      </span>
      <span class="state {st.tone}"><span class="pip"></span><span class="trunc">{st.text}</span></span>
      <Icon name="chevronRight" size={16} />
    </button>
  {/each}
</div>
<button class="btn-soft add" onclick={() => (mail.screen = 'onboarding')}><Icon name="plus" size={16} />{t('act.addAccount')}</button>

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .list {
    display: flex;
    flex-direction: column;
    margin-top: 16px;
    border-top: 1px solid var(--line);
  }

  .acc {
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 68px;
    padding: 0 8px;
    border: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
    color: var(--ink-3);
    text-align: left;
    transition: background var(--t-fast);
  }

  .acc:hover {
    background: var(--hover);
  }

  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--c) 18%, transparent);
    /* Harf hesap renginin mürekkeple koyulaşmışı: iki temada ≥ 4.5:1. */
    color: color-mix(in srgb, var(--c) 60%, var(--ink));
    font-weight: 600;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .name {
    color: var(--ink);
    font-weight: 500;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 220px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .pip {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--ok);
  }

  .busy .pip {
    background: var(--accent);
  }

  .warn {
    color: var(--amber-text);
  }

  .warn .pip {
    background: var(--amber);
  }

  .add {
    margin-top: 16px;
  }
</style>
