<script lang="ts">
  // Ayarlar: solda bölümler ve hesaplar, sağda seçili bölüm.
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import type { IconName } from '../../../lib/ui/icons';
  import { mail, openSettings, type SettingsSection } from '../../../lib/stores/mail.svelte';
  import AboutSection from './AboutSection.svelte';
  import AccountPage from './AccountPage.svelte';
  import AccountsSection from './AccountsSection.svelte';
  import ComposeSection from './ComposeSection.svelte';
  import DevicesSection from './DevicesSection.svelte';
  import GeneralSection from './GeneralSection.svelte';
  import NotifySection from './NotifySection.svelte';
  import PrivacySection from './PrivacySection.svelte';
  import RulesSection from './RulesSection.svelte';
  import TagsSection from './TagsSection.svelte';

  const sections: { key: SettingsSection; icon: IconName; label: MessageKey }[] = [
    { key: 'general', icon: 'palette', label: 'set.nav.general' },
    { key: 'notifications', icon: 'bell', label: 'set.nav.notifications' },
    { key: 'compose', icon: 'pen', label: 'set.nav.compose' },
    { key: 'rules', icon: 'rules', label: 'set.nav.rules' },
    { key: 'tags', icon: 'tag', label: 'tag.title' },
    { key: 'accounts', icon: 'user', label: 'set.nav.accounts' },
  ];
  const tail: { key: SettingsSection; icon: IconName; label: MessageKey }[] = [
    { key: 'devices', icon: 'devices', label: 'aktar.nav' },
    { key: 'privacy', icon: 'shield', label: 'set.nav.privacy' },
    { key: 'about', icon: 'info', label: 'set.nav.about' },
  ];

  const section = $derived(mail.settingsSection);
  const accountId = $derived(section.startsWith('account:') ? section.slice(8) : null);
  // Kaldırılan hesabın sayfası açık kalmasın.
  $effect(() => {
    if (accountId && !mail.accounts.some((a) => a.id === accountId)) openSettings('accounts');
  });

  let content = $state<HTMLDivElement>();
  $effect(() => {
    void section;
    content?.scrollTo({ top: 0 });
  });
</script>

<section class="settings" aria-labelledby="set-title">
  <nav class="snav" aria-label={t('set.title')}>
    <h1 id="set-title">{t('set.title')}</h1>
    {#each sections as s (s.key)}
      {@const on = section === s.key || (s.key === 'accounts' && accountId != null)}
      <button class="sitem" class:on aria-current={section === s.key ? 'page' : undefined} onclick={() => openSettings(s.key)}>
        <Icon name={s.icon} />
        <span class="trunc">{t(s.label)}</span>
      </button>
      {#if s.key === 'accounts'}
        {#each mail.accounts as a (a.id)}
          {@const here = accountId === a.id}
          <button class="sitem sub" class:on={here} aria-current={here ? 'page' : undefined} onclick={() => openSettings(`account:${a.id}`)}>
            <span class="dot" style:background={a.color}></span>
            <span class="trunc">{a.name}</span>
          </button>
        {/each}
      {/if}
    {/each}
    <div class="sep"></div>
    {#each tail as s (s.key)}
      <button class="sitem" class:on={section === s.key} aria-current={section === s.key ? 'page' : undefined} onclick={() => openSettings(s.key)}>
        <Icon name={s.icon} />
        <span class="trunc">{t(s.label)}</span>
      </button>
    {/each}
  </nav>

  <div class="body" bind:this={content}>
    <button class="btn-icon close" aria-label={t('set.close')} title={t('set.close')} onclick={() => (mail.screen = 'mail')}>
      <Icon name="x" />
    </button>
    <div class="inner">
      {#key section}
        {#if accountId}
          <AccountPage id={accountId} />
        {:else if section === 'general'}
          <GeneralSection />
        {:else if section === 'notifications'}
          <NotifySection />
        {:else if section === 'compose'}
          <ComposeSection />
        {:else if section === 'rules'}
          <RulesSection />
        {:else if section === 'tags'}
          <TagsSection />
        {:else if section === 'accounts'}
          <AccountsSection />
        {:else if section === 'devices'}
          <DevicesSection />
        {:else if section === 'privacy'}
          <PrivacySection />
        {:else}
          <AboutSection />
        {/if}
      {/key}
    </div>
  </div>
</section>

<style>
  .settings {
    flex: 1;
    min-width: 0;
    display: flex;
    background: var(--paper);
    border-left: 1px solid var(--line);
  }

  .snav {
    flex: none;
    width: 232px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 22px 10px 16px;
    overflow-y: auto;
    background: var(--surface);
    border-right: 1px solid var(--line);
  }

  h1 {
    padding: 0 10px 14px;
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
  }

  .sitem {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink-2);
    text-align: left;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .sitem:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .sitem.on {
    background: var(--paper);
    color: var(--ink);
    font-weight: 500;
    box-shadow: 0 1px 2px rgba(20, 24, 40, 0.08);
  }

  :global(:root[data-theme='dark']) .sitem.on {
    background: var(--raised);
    box-shadow: none;
  }

  .sitem.sub {
    height: 30px;
    padding-left: 38px;
    font-size: var(--fs-body);
  }

  .sitem.sub.on {
    box-shadow: none;
    background: var(--hover);
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
  }

  .sep {
    height: 1px;
    margin: 10px 10px;
    background: var(--line);
  }

  .body {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow-y: auto;
  }

  .close {
    position: sticky;
    top: 14px;
    float: right;
    margin: 14px 14px 0 0;
    z-index: 2;
  }

  .inner {
    max-width: 780px;
    margin: 0 auto;
    padding: 32px 40px 64px;
    animation: fade-up var(--t-state) ease-out;
  }

  @media (max-width: 900px) {
    .snav {
      width: 196px;
    }

    .inner {
      padding: 28px 24px 56px;
    }
  }
</style>
