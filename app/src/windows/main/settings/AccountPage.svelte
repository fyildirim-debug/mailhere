<script lang="ts">
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { mail, openSettings } from '../../../lib/stores/mail.svelte';
  import AccountFolders from './AccountFolders.svelte';
  import AccountGeneral from './AccountGeneral.svelte';
  import AccountServer from './AccountServer.svelte';
  import AccountVacation from './AccountVacation.svelte';
  import SignaturesEditor from './SignaturesEditor.svelte';

  interface Props {
    id: string;
  }

  let { id }: Props = $props();

  type Tab = 'general' | 'signatures' | 'server' | 'folders' | 'vacation';
  const tabs: { key: Tab; label: MessageKey }[] = [
    { key: 'general', label: 'acc.tab.general' },
    { key: 'signatures', label: 'acc.tab.signatures' },
    { key: 'server', label: 'acc.tab.server' },
    { key: 'folders', label: 'acc.tab.folders' },
    { key: 'vacation', label: 'acc.tab.vacation' },
  ];

  let tab = $state<Tab>('general');
  const account = $derived(mail.accounts.find((a) => a.id === id));

  function onTabKey(e: KeyboardEvent) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    e.preventDefault();
    const i = tabs.findIndex((x) => x.key === tab);
    const next = tabs[(i + (e.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length];
    tab = next.key;
    document.getElementById(`acc-tab-${next.key}`)?.focus();
  }
</script>

{#if account}
  <button class="link back" onclick={() => openSettings('accounts')}><Icon name="chevronLeft" size={14} />{t('set.nav.accounts')}</button>
  <header class="head">
    <span class="badge" style:--c={account.color}>{account.name.slice(0, 1).toLocaleUpperCase('tr')}</span>
    <div class="who">
      <h2 class="trunc">{account.name}</h2>
      <span class="trunc meta">{account.email}</span>
    </div>
  </header>

  <div class="tabs" role="tablist" aria-label={t('acc.tabs')} tabindex="-1" onkeydown={onTabKey}>
    {#each tabs as x (x.key)}
      <button
        id="acc-tab-{x.key}"
        class="tab"
        role="tab"
        aria-selected={tab === x.key}
        aria-controls="acc-panel"
        tabindex={tab === x.key ? 0 : -1}
        onclick={() => (tab = x.key)}
      >
        {t(x.label)}
      </button>
    {/each}
  </div>

  <div id="acc-panel" class="panel" role="tabpanel" aria-labelledby="acc-tab-{tab}">
    {#key `${tab}-${account.id}`}
      {#if tab === 'general'}
        <AccountGeneral {account} />
      {:else if tab === 'signatures'}
        <SignaturesEditor {account} />
      {:else if tab === 'server'}
        <AccountServer {account} />
      {:else if tab === 'folders'}
        <AccountFolders {account} />
      {:else}
        <AccountVacation {account} />
      {/if}
    {/key}
  </div>
{/if}

<style>
  .back {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-meta);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    margin: 14px 0 22px;
  }

  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--c) 18%, transparent);
    color: var(--c);
    font-size: var(--fs-section);
    font-weight: 600;
  }

  .who {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  h2 {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--line);
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }

  .tabs::-webkit-scrollbar {
    display: none;
  }

  .tab {
    position: relative;
    flex: none;
    height: 40px;
    padding: 0 12px;
    border: 0;
    background: transparent;
    color: var(--ink-2);
    font-weight: 500;
    transition: color var(--t-fast);
  }

  .tab:hover {
    color: var(--ink);
  }

  .tab[aria-selected='true'] {
    color: var(--ink);
  }

  /* Seçili sekmenin altında vurgu çizgisi (kenar değil, alt çizgi). */
  .tab[aria-selected='true']::after {
    content: '';
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: 0;
    height: 2px;
    border-radius: 2px;
    background: var(--accent);
  }

  .panel {
    padding-top: 8px;
    animation: fade-up var(--t-state) ease-out;
  }

  /* Mobil: uygulama çubuğu geri düğmesini ve hesap adını taşır; sekme şeridi
     kenara kadar yatay kaydırılır, sekmeler 44 px. */
  @container settings (max-width: 600px) {
    .back,
    .head {
      display: none;
    }

    .tabs {
      margin: 0 -16px;
      padding: 0 8px;
    }

    .tab {
      height: var(--m-touch, 44px);
    }
  }
</style>
