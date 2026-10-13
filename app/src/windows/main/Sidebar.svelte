<script lang="ts">
  import type { View } from '../../lib/api';
  import { call } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import CertDialog, { type CertTarget } from '../../lib/mail/CertDialog.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import { mail, setView, syncLabel } from '../../lib/stores/mail.svelte';
  import { installUpdate, update } from '../../lib/stores/update.svelte';

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
  let cert = $state<CertTarget[] | null>(null);

  /** Eşitlemenin takıldığı sunucunun sertifikasını gösterir. */
  function reviewCert() {
    const u = sync.untrusted;
    if (!u) return;
    const servers = mail.accounts.flatMap((a) => [a.imap, a.smtp]).filter((s) => s != null);
    const security = servers.find((s) => s.host === u.host && s.port === u.port)?.security ?? 'tls';
    cert = [{ host: u.host, port: u.port, security }];
  }

  function certClosed(trusted: boolean) {
    cert = null;
    if (trusted) void call('sync_now');
  }
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

  {#if update.info && !update.dismissed}
    <div class="update" role="status">
      <div class="u-head">
        <Icon name="arrowDown" size={16} />
        <strong class="trunc" title={t('update.available', { version: update.info.version })}>{t('update.short', { version: update.info.version })}</strong>
        <button class="btn-icon small" aria-label={t('update.later')} title={t('update.later')} onclick={() => (update.dismissed = true)}>
          <Icon name="x" size={14} />
        </button>
      </div>
      {#if update.info.notes}<p class="u-notes">{update.info.notes}</p>{/if}
      {#if update.error}<p class="u-notes err">{update.error}</p>{/if}
      <button class="btn-primary small u-install" disabled={update.installing} onclick={installUpdate}>
        {update.installing ? t('update.installing') : t('update.install')}
      </button>
    </div>
  {/if}

  <div class="foot">
    <span class="state {sync.tone}"></span>
    {#if sync.untrusted}
      <button class="link trunc foot-text warn" onclick={reviewCert}>{sync.text}</button>
    {:else}
      <span class="trunc foot-text">{sync.text}</span>
    {/if}
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

{#if cert}<CertDialog targets={cert} onclose={certClosed} />{/if}

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

  .update {
    margin-top: auto;
    margin-bottom: 4px;
    padding: 12px;
    border-radius: var(--r-card);
    background: var(--paper);
    border: 1px solid var(--line);
    animation: fade-up var(--t-panel) var(--ease);
  }

  .u-head {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent-text);
  }

  .u-head strong {
    flex: 1;
    font-weight: 600;
    color: var(--ink);
  }

  .u-head .btn-icon {
    margin: -6px -6px -6px 0;
  }

  .u-notes {
    margin-top: 4px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .u-notes.err {
    color: var(--danger);
  }

  .u-install {
    width: 100%;
    margin-top: 10px;
  }

  .update + .foot {
    margin-top: 0;
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

  .foot-text.warn {
    padding: 0;
    text-align: left;
    color: var(--amber);
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
