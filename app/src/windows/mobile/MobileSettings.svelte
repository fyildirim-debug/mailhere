<script lang="ts">
  // Mobil ayarlar: `section` boşsa bölüm listesi, doluysa o bölüm tek sütunda.
  // Her bölüm gezinme yığınına ayrı ekran olarak itilir (nav.push); geri tuşu
  // listeye döner. Bölümler masaüstüyle aynı bileşenlerdir: masaüstüne özgü
  // alanlar bileşen içinde `shell` ile gizlenir, yerleşim `container: settings`
  // kapsayıcı sorgusuyla tek sütuna iner (global.css ve bölüm stilleri).
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import { accountHint } from '../../lib/mail/nav';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import { mail, type SettingsSection } from '../../lib/stores/mail.svelte';
  import AboutSection from '../main/settings/AboutSection.svelte';
  import AccountPage from '../main/settings/AccountPage.svelte';
  import AccountsSection from '../main/settings/AccountsSection.svelte';
  import ComposeSection from '../main/settings/ComposeSection.svelte';
  import DevicesSection from '../main/settings/DevicesSection.svelte';
  import GeneralSection from '../main/settings/GeneralSection.svelte';
  import NotifySection from '../main/settings/NotifySection.svelte';
  import PrivacySection from '../main/settings/PrivacySection.svelte';
  import RulesSection from '../main/settings/RulesSection.svelte';
  import TagsSection from '../main/settings/TagsSection.svelte';
  import AppBar from './AppBar.svelte';
  import MobileGestures from './MobileGestures.svelte';

  interface Props {
    section: SettingsSection | null;
    onback: () => void;
    /** Listeden bölüm seçilince (yığına itilir). */
    onopen: (section: SettingsSection) => void;
    onaddaccount: () => void;
  }

  let { section, onback, onopen, onaddaccount }: Props = $props();

  interface Item {
    key: SettingsSection;
    icon: IconName;
    label: MessageKey;
  }

  const head: Item[] = [
    { key: 'general', icon: 'palette', label: 'set.nav.general' },
    { key: 'notifications', icon: 'bell', label: 'set.nav.notifications' },
    { key: 'compose', icon: 'pen', label: 'set.nav.compose' },
    { key: 'rules', icon: 'rules', label: 'set.nav.rules' },
    { key: 'tags', icon: 'tag', label: 'tag.title' },
  ];
  // Hesaplar grubu aşağıda ayrı çizilir (hesap başına satır + "Hesap ekle");
  // "Cihazlar" (bilgisayardan QR ile aktarım) onun altında, Gizlilik'in üstünde.
  const tail: Item[] = [
    { key: 'devices', icon: 'devices', label: 'aktar.nav' },
    { key: 'privacy', icon: 'shield', label: 'set.nav.privacy' },
    { key: 'about', icon: 'info', label: 'mob.settings.about' },
  ];

  const version = __APP_VERSION__;
  const accountId = $derived(section?.startsWith('account:') ? section.slice(8) : null);
  const account = $derived(accountId ? mail.accounts.find((a) => a.id === accountId) : undefined);

  // Kaldırılan hesabın sayfası açık kalmasın: listeye dönülür.
  $effect(() => {
    if (accountId && mail.accounts.length && !mail.accounts.some((a) => a.id === accountId)) onback();
  });

  const title = $derived.by(() => {
    if (section == null) return t('set.title');
    if (accountId) return account?.name ?? t('set.nav.accounts');
    const item = [...head, ...tail].find((x) => x.key === section);
    return item ? t(item.label) : t('set.nav.accounts');
  });
</script>

<AppBar leading="back" {title} onleading={onback} />

{#if section == null}
  <nav class="list" aria-label={t('mob.settings.sections')}>
    <div class="group">
      {#each head as s (s.key)}
        <button class="row" onclick={() => onopen(s.key)}>
          <Icon name={s.icon} size={22} />
          <span class="trunc grow">{t(s.label)}</span>
          <Icon name="chevronRight" size={18} class="chev" />
        </button>
      {/each}
    </div>

    <div class="m-section group-title">{t('set.nav.accounts')}</div>
    <div class="group">
      {#each mail.accounts as a (a.id)}
        <button class="row acc" onclick={() => onopen(`account:${a.id}`)}>
          <span class="badge" style:--c={a.color} aria-hidden="true">{a.name.slice(0, 1).toLocaleUpperCase('tr')}</span>
          <span class="text">
            <span class="trunc name">{a.name}</span>
            <span class="trunc meta">{a.email} · {accountHint(a)}</span>
          </span>
          <Icon name="chevronRight" size={18} class="chev" />
        </button>
      {/each}
      <button class="row" onclick={onaddaccount}>
        <Icon name="plus" size={22} />
        <span class="trunc grow">{t('act.addAccount')}</span>
      </button>
    </div>

    <div class="group">
      {#each tail as s (s.key)}
        <button class="row" onclick={() => onopen(s.key)}>
          <Icon name={s.icon} size={22} />
          <span class="trunc grow">{t(s.label)}</span>
          <Icon name="chevronRight" size={18} class="chev" />
        </button>
      {/each}
    </div>

    <p class="version meta num">{t('set.version', { version })}</p>
  </nav>
{:else}
  <div class="body">
    {#key section}
      {#if accountId}
        {#if account}<AccountPage id={accountId} />{/if}
      {:else if section === 'general'}
        <GeneralSection />
        <MobileGestures />
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
{/if}

<style>
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 0 calc(16px + var(--sab));
    background: var(--paper);
    overscroll-behavior: contain;
  }

  /* Gruplar arası cömert boşluk; grup içinde satırlar etiket hizasından başlayan ince çizgiyle ayrılır. */
  .group {
    display: flex;
    flex-direction: column;
  }

  .group + .group {
    margin-top: 20px;
  }

  .group-title {
    padding-bottom: 2px;
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    min-height: 56px;
    padding: 0 12px 0 var(--m-gutter);
    border: 0;
    background: transparent;
    color: var(--ink);
    text-align: left;
    transition: background var(--t-fast);
  }

  .row:active {
    background: var(--hover);
  }

  .row + .row::before {
    content: '';
    position: absolute;
    top: 0;
    left: 52px;
    right: 0;
    height: 1px;
    background: var(--line);
  }

  .row > :global(.icon) {
    flex: none;
    color: var(--ink-2);
  }

  .row > :global(.chev) {
    color: var(--ink-3);
  }

  .grow {
    flex: 1;
  }

  .acc {
    min-height: 64px;
  }

  .acc + .row::before,
  .row + .acc::before {
    left: 66px;
  }

  /* Hesap rozeti: zemin hesap renginin %18'i; harf aynı ton mürekkeple
     koyulaşır (--ink temayla döner, iki temada ≥ 4.5:1). */
  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--c) 18%, transparent);
    color: color-mix(in srgb, var(--c) 60%, var(--ink));
    font-weight: 600;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.35;
  }

  .name {
    font-weight: 500;
  }

  .version {
    padding: 28px var(--m-gutter) 0;
    text-align: center;
  }

  /* Bölüm gövdesi: tek sütun, kapsayıcı sorgularının kökü. */
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px var(--m-gutter) calc(32px + var(--sab));
    background: var(--paper);
    container: settings / inline-size;
    overscroll-behavior: contain;
  }

  /* Bölüm başlığını uygulama çubuğu taşır; sayfa içi h2 yinelenmez. */
  .body :global(h2.page) {
    display: none;
  }
</style>
