<script lang="ts">
  import type { View } from '../../lib/api';
  import { call } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import { mail, setView, syncLabel } from '../../lib/stores/mail.svelte';

  interface NavItem {
    kind: Exclude<View['kind'], 'mailbox'>;
    icon: IconName;
    label: MessageKey;
    count?: () => number;
  }

  const items: NavItem[] = [
    { kind: 'inbox', icon: 'inbox', label: 'nav.inbox', count: () => mail.counts.inboxUnread },
    { kind: 'snoozed', icon: 'clock', label: 'nav.snoozed', count: () => mail.counts.snoozed },
    { kind: 'starred', icon: 'star', label: 'nav.starred' },
    { kind: 'drafts', icon: 'draft', label: 'nav.drafts', count: () => mail.counts.drafts },
    { kind: 'sent', icon: 'send', label: 'nav.sent' },
    { kind: 'archive', icon: 'archive', label: 'nav.archive' },
    { kind: 'trash', icon: 'trash', label: 'nav.trash' },
  ];

  function domainOf(email: string) {
    return email.split('@')[1] ?? email;
  }

  const sync = $derived(syncLabel());
</script>

<nav class="side" aria-label={t('nav.label')}>
  <button class="btn-primary compose" onclick={() => call('open_compose', { prefill: { mode: 'new', messageId: null } })}>
    <Icon name="pen" />
    <span>{t('act.compose')}</span>
    <span class="kbd on-accent">C</span>
  </button>

  {#each items as item (item.kind)}
    {@const active = mail.screen === 'mail' && mail.view.kind === item.kind && mail.accountId == null}
    {@const n = item.count?.() ?? 0}
    <button class="nav" class:active aria-current={active ? 'page' : undefined} onclick={() => setView({ kind: item.kind })}>
      <Icon name={item.icon} />
      <span class="trunc">{t(item.label)}</span>
      {#if n > 0}<span class="count num">{n}</span>{/if}
    </button>
  {/each}

  <div class="group">{t('nav.accounts')}</div>
  {#each mail.accounts as account (account.id)}
    {@const active = mail.screen === 'mail' && mail.view.kind === 'inbox' && mail.accountId === account.id}
    {@const n = mail.counts.perAccountUnread[account.id] ?? 0}
    <button
      class="nav"
      class:active
      title={account.email}
      aria-current={active ? 'page' : undefined}
      onclick={() => setView({ kind: 'inbox' }, account.id)}
    >
      <span class="dot" style:background={account.color}></span>
      <span>{account.name}</span>
      <span class="trunc domain">{account.kind === 'jmap' ? 'JMAP' : domainOf(account.email)}</span>
      {#if n > 0}<span class="count num">{n}</span>{/if}
    </button>
  {/each}

  <div class="foot">
    <span class="state {sync.tone}"></span>
    <span class="trunc foot-text">{sync.text}</span>
    <button
      class="btn-icon"
      class:on={mail.screen === 'settings'}
      aria-label={t('act.settings')}
      title={t('act.settings')}
      onclick={() => (mail.screen = mail.screen === 'settings' ? 'mail' : 'settings')}
    >
      <Icon name="settings" />
    </button>
  </div>
</nav>

<style>
  .side {
    flex: none;
    width: 232px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 10px 10px;
    background: var(--ground);
    overflow-y: auto;
  }

  .compose {
    justify-content: flex-start;
    height: 40px;
    margin-bottom: 14px;
  }

  .on-accent {
    margin-left: auto;
    background: transparent;
    border-color: color-mix(in srgb, var(--accent-ink) 40%, transparent);
    color: inherit;
  }

  .nav {
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

  .nav:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .nav.active {
    background: var(--paper);
    color: var(--ink);
    font-weight: 500;
    box-shadow: 0 1px 2px rgba(20, 24, 40, 0.08);
  }

  :global(:root[data-theme='dark']) .nav.active {
    background: var(--raised);
    box-shadow: none;
  }

  .count {
    margin-left: auto;
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-2);
  }

  .group {
    padding: 20px 10px 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    margin: 0 5px;
    border-radius: 50%;
  }

  .domain {
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .foot {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 0 0 10px;
  }

  .foot-text {
    flex: 1;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .state {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--ok);
  }

  .state.busy {
    background: var(--accent);
    animation: pulse 1.4s var(--ease) infinite;
  }

  .state.warn {
    background: var(--amber);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
