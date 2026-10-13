<script lang="ts">
  import { call, EVENTS, on, toAppError, type DiscoveredConfig, type Security, type ServerConfig } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Logo from '../../lib/ui/Logo.svelte';
  import { mail, reloadAccounts } from '../../lib/stores/mail.svelte';

  type Step = 'email' | 'found' | 'ready';

  let step = $state<Step>('email');
  let email = $state('');
  let password = $state('');
  let showPass = $state(false);
  let name = $state('');
  let displayName = $state('');
  let finding = $state(false);
  let adding = $state(false);
  let error = $state<string | null>(null);
  let editing = $state(false);
  let notFound = $state(false);
  let config = $state<DiscoveredConfig | null>(null);
  let imap = $state<ServerConfig>({ host: '', port: 993, security: 'tls', username: '' });
  let smtp = $state<ServerConfig>({ host: '', port: 465, security: 'tls', username: '' });
  let newAccountId = $state<string | null>(null);
  let progress = $state<{ done: number; total: number } | null>(null);
  let online = $state(false);

  const valid = $derived(/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim()));
  const domain = $derived(email.trim().split('@')[1]?.toLowerCase() ?? '');
  const needsAppPassword = $derived(
    /^(gmail\.com|googlemail\.com|outlook\.com|hotmail\.com|live\.com|msn\.com|yahoo\.com|icloud\.com|me\.com|mac\.com)$/.test(domain),
  );
  const hasAccounts = $derived(mail.accounts.length > 0);

  function defaults() {
    const local = email.trim().split('@')[0] ?? '';
    const known: Record<string, string> = { 'gmail.com': 'Gmail', 'googlemail.com': 'Gmail', 'outlook.com': 'Outlook', 'hotmail.com': 'Outlook', 'icloud.com': 'iCloud', 'yahoo.com': 'Yahoo' };
    const label = domain.split('.')[0] ?? '';
    name ||= known[domain] ?? label.charAt(0).toLocaleUpperCase('tr') + label.slice(1);
    displayName ||= local.split(/[._-]/)[0].replace(/^./, (c) => c.toLocaleUpperCase('tr'));
  }

  async function find() {
    if (!valid || finding) return;
    error = null;
    finding = true;
    notFound = false;
    try {
      config = await call('account_discover', { email: email.trim() });
      imap = { ...config.imap };
      smtp = { ...config.smtp };
      editing = false;
    } catch {
      config = null;
      notFound = true;
      editing = true;
      imap = { host: `imap.${domain}`, port: 993, security: 'tls', username: email.trim() };
      smtp = { host: `smtp.${domain}`, port: 465, security: 'tls', username: email.trim() };
    } finally {
      finding = false;
    }
    defaults();
    step = 'found';
  }

  function manual() {
    if (!valid) {
      error = t('onb.invalidEmail');
      return;
    }
    config = null;
    notFound = false;
    editing = true;
    imap = { host: `imap.${domain}`, port: 993, security: 'tls', username: email.trim() };
    smtp = { host: `smtp.${domain}`, port: 465, security: 'tls', username: email.trim() };
    defaults();
    step = 'found';
  }

  async function add() {
    if (!password || adding) return;
    adding = true;
    error = null;
    try {
      const account = await call('account_add', {
        input: { name: name.trim() || domain, email: email.trim(), displayName: displayName.trim() || email.trim(), color: null, imap, smtp, password },
      });
      password = '';
      newAccountId = account.id;
      step = 'ready';
      await reloadAccounts();
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      adding = false;
    }
  }

  $effect(() => {
    const id = newAccountId;
    if (!id) return;
    return on(EVENTS.sync, (s) => {
      if (s.accountId !== id) return;
      if (s.state === 'syncing') progress = { done: s.done, total: s.total };
      if (s.state === 'online') {
        online = true;
        progress = null;
      }
    });
  });

  function again() {
    step = 'email';
    email = '';
    password = '';
    name = '';
    displayName = '';
    newAccountId = null;
    progress = null;
    online = false;
  }

  function openInbox() {
    mail.screen = 'mail';
  }

  const securities: Security[] = ['tls', 'starttls'];
  const sourceKey = $derived(config ? (`onb.foundFrom.${config.source}` as MessageKey) : null);
</script>

<div class="onb" class:full={!hasAccounts}>
  <aside class="brand">
    <Logo size={40} inverse />
    <h2>{t('onb.headline')}</h2>
    <ul>
      <li><Icon name="shield" size={20} /><span>{t('onb.p1')}</span></li>
      <li><Icon name="eyeOff" size={20} /><span>{t('onb.p2')}</span></li>
      <li><Icon name="plug" size={20} /><span>{t('onb.p3')}</span></li>
    </ul>
  </aside>

  <section class="form">
    {#if hasAccounts && step !== 'ready'}
      <button class="btn-icon close" aria-label={t('act.close')} onclick={openInbox}><Icon name="x" /></button>
    {/if}

    {#if step === 'email'}
      <div class="step">
        <h1>{t('onb.title')}</h1>
        <p class="lead">{t('onb.lead')}</p>
        <label class="label" for="onb-email">{t('onb.email')}</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          id="onb-email"
          class="input"
          type="email"
          autocomplete="email"
          placeholder="ad@alanadi.com"
          bind:value={email}
          onkeydown={(e) => e.key === 'Enter' && find()}
          autofocus
        />
        {#if error}<p class="error">{error}</p>{/if}
        <button class="btn-primary wide" disabled={!valid || finding} onclick={find}>{finding ? t('onb.finding') : t('onb.continue')}</button>
        <div class="or"><span></span>{t('onb.or')}<span></span></div>
        <div class="alts">
          <button class="alt" onclick={() => { if (!email.includes('@')) email = '@gmail.com'; document.getElementById('onb-email')?.focus(); }}>
            <Icon name="globe" />{t('onb.google')}
          </button>
          <button class="alt" onclick={() => { if (!email.includes('@')) email = '@outlook.com'; document.getElementById('onb-email')?.focus(); }}>
            <Icon name="grid" />{t('onb.microsoft')}
          </button>
          <button class="alt" onclick={manual}><Icon name="settings" />{t('onb.manual')}</button>
          <button class="alt" disabled><Icon name="server" />{t('onb.jmap')}<span class="soon">{t('onb.soon')}</span></button>
        </div>
      </div>
    {:else if step === 'found'}
      <div class="step">
        <button class="link back" onclick={() => (step = 'email')}><Icon name="chevronLeft" size={14} />{email}</button>
        <h1>{notFound ? t('onb.edit') : t('onb.found')}</h1>
        <p class="lead">{notFound ? t('onb.notFound') : sourceKey ? t(sourceKey, { domain }) : ''}</p>

        {#if !editing}
          <div class="servers">
            <span class="srv-label">{t('onb.incoming')}</span><span class="trunc">{imap.host} · {imap.port}</span><span class="sec">{imap.security.toUpperCase()}</span>
            <span class="srv-label">{t('onb.outgoing')}</span><span class="trunc">{smtp.host} · {smtp.port}</span><span class="sec">{smtp.security.toUpperCase()}</span>
          </div>
          <button class="link small" onclick={() => (editing = true)}>{t('onb.edit')}</button>
        {:else}
          {#each [{ key: 'onb.incoming', cfg: imap }, { key: 'onb.outgoing', cfg: smtp }] as row (row.key)}
            <fieldset class="manual">
              <legend>{t(row.key as MessageKey)}</legend>
              <label><span class="label">{t('onb.host')}</span><input class="input small" bind:value={row.cfg.host} /></label>
              <label class="port"><span class="label">{t('onb.port')}</span><input class="input small num" type="number" bind:value={row.cfg.port} /></label>
              <label><span class="label">{t('onb.security')}</span>
                <select class="input small" bind:value={row.cfg.security}>
                  {#each securities as s (s)}<option value={s}>{s === 'tls' ? 'SSL/TLS' : 'STARTTLS'}</option>{/each}
                </select>
              </label>
              <label class="span"><span class="label">{t('onb.username')}</span><input class="input small" bind:value={row.cfg.username} /></label>
            </fieldset>
          {/each}
        {/if}

        <div class="names">
          <label><span class="label">{t('onb.name')}</span><input class="input small" bind:value={name} /></label>
          <label><span class="label">{t('onb.displayName')}</span><input class="input small" bind:value={displayName} /></label>
        </div>

        {#if needsAppPassword}<p class="note">{t('onb.appPassword')}</p>{/if}
        <label class="label" for="onb-pass">{t('onb.password')}</label>
        <div class="pass">
          <input
            id="onb-pass"
            class="input"
            type={showPass ? 'text' : 'password'}
            autocomplete="current-password"
            bind:value={password}
            onkeydown={(e) => e.key === 'Enter' && add()}
          />
          <button class="link small show" onclick={() => (showPass = !showPass)}>{showPass ? t('onb.hide') : t('onb.show')}</button>
        </div>
        <span class="meta keyring">{t('onb.keyring')}</span>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <button class="btn-primary wide" disabled={!password || adding} onclick={add}>{adding ? t('onb.adding') : t('onb.add')}</button>
      </div>
    {:else}
      <div class="step">
        <span class="ok-icon"><Icon name="check" size={32} /></span>
        <h1>{t('onb.readyTitle')}</h1>
        <p class="lead">{t('onb.readyLead')}</p>
        <div class="checks">
          <div><Icon name="check" size={16} class="ok" />{t('onb.stepFolders')}</div>
          <div><Icon name={online ? 'check' : 'sync'} size={16} class={online ? 'ok' : 'busy'} />{t('onb.stepPush')}</div>
          <div><Icon name="sync" size={16} class="busy" />{t('onb.stepMail')}{#if progress?.total}<span class="meta num">  {progress.done}/{progress.total}</span>{/if}</div>
          <div class="bar"><div class="fill" style:width={progress?.total ? `${Math.max(6, (progress.done / progress.total) * 100)}%` : undefined} class:indeterminate={!progress?.total}></div></div>
        </div>
        <button class="btn-primary wide" onclick={openInbox}>{t('onb.openInbox')}</button>
        <button class="link small again" onclick={again}>{t('onb.another')}</button>
      </div>
    {/if}
  </section>
</div>

<style>
  .onb {
    flex: 1;
    min-width: 0;
    display: flex;
    background: var(--paper);
    overflow: hidden;
  }

  .brand {
    flex: none;
    width: 380px;
    display: flex;
    flex-direction: column;
    padding: 40px 40px 36px;
    background: var(--brand);
    color: var(--brand-ink);
  }

  .brand h2 {
    margin-top: auto;
    font-size: var(--fs-display);
    line-height: 1.15;
    font-weight: 600;
    letter-spacing: -0.025em;
    text-wrap: balance;
  }

  ul {
    margin: 28px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 16px;
    color: var(--brand-ink-2);
  }

  li {
    display: flex;
    gap: 12px;
  }

  li :global(.icon) {
    margin-top: 1px;
  }

  .form {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    justify-content: center;
    overflow-y: auto;
    padding: 48px 64px;
  }

  .close {
    position: absolute;
    top: 14px;
    right: 14px;
  }

  .step {
    width: 100%;
    max-width: 420px;
    margin: auto 0;
    display: flex;
    flex-direction: column;
    animation: step-in var(--t-panel) var(--ease);
  }

  h1 {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .lead {
    margin: 6px 0 24px;
    color: var(--ink-2);
  }

  .wide {
    width: 100%;
    height: 44px;
    margin-top: 16px;
  }

  .or {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 26px 0 14px;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .or span {
    flex: 1;
    height: 1px;
    background: var(--line);
  }

  .alts {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }

  .alt {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 44px;
    padding: 0 14px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-row);
    background: var(--paper);
    font-weight: 500;
    text-align: left;
    transition: background var(--t-fast);
  }

  .alt:hover:not(:disabled) {
    background: var(--surface);
  }

  .alt:disabled {
    color: var(--ink-3);
  }

  .soon {
    margin-left: auto;
    font-size: var(--fs-meta);
    font-weight: 400;
  }

  .back {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 12px;
    font-size: var(--fs-meta);
  }

  .servers {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    column-gap: 14px;
    row-gap: 10px;
    align-items: center;
    padding: 14px 16px;
    border-radius: var(--r-card);
    background: var(--surface);
    border: 1px solid var(--line);
  }

  .srv-label {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .sec {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ok);
  }

  .link.small {
    align-self: flex-start;
    margin-top: 8px;
    font-size: var(--fs-meta);
  }

  .manual {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 92px;
    gap: 10px;
    margin: 0 0 14px;
    padding: 12px 14px 14px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
  }

  legend {
    padding: 0 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .manual .span {
    grid-column: 1 / -1;
  }

  .names {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    margin: 18px 0;
  }

  .note {
    margin-bottom: 12px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .pass {
    position: relative;
  }

  .pass .input {
    padding-right: 76px;
  }

  .show {
    position: absolute;
    right: 14px;
    top: 4px;
  }

  .keyring {
    margin-top: 8px;
  }

  .error {
    margin-top: 10px;
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  .ok-icon {
    color: var(--accent);
    margin-bottom: 10px;
  }

  .checks {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .checks > div {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .checks :global(.ok) {
    color: var(--ok);
  }

  .checks :global(.busy) {
    color: var(--accent-text);
  }

  .bar {
    height: 6px;
    margin-left: 26px;
    border-radius: 3px;
    background: var(--surface);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width var(--t-panel) var(--ease);
  }

  .fill.indeterminate {
    width: 30%;
    animation: slide 1.6s var(--ease) infinite;
  }

  .again {
    align-self: center;
    margin-top: 14px;
  }

  @keyframes step-in {
    from {
      opacity: 0;
      transform: translateX(10px);
    }
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  @media (max-width: 860px) {
    .brand {
      display: none;
    }

    .form {
      padding: 32px 24px;
    }
  }
</style>
