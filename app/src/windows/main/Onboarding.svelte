<script lang="ts">
  import { onMount } from 'svelte';
  import {
    call,
    toAppError,
    type AuthKind,
    type DiscoveredConfig,
    type OAuthGrant,
    type Security,
    type ServerConfig,
  } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import CertDialog, { type CertTarget } from '../../lib/mail/CertDialog.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Logo from '../../lib/ui/Logo.svelte';
  import { isMobileShell } from '../../lib/shell';
  import { mail, reloadAccounts } from '../../lib/stores/mail.svelte';

  type Step = 'email' | 'found' | 'jmap' | 'oauth' | 'ready' | 'transfer';
  type Provider = Exclude<AuthKind, 'password'>;

  // Mobilde ilk seçenek: bilgisayardan QR ile aktarım. Bileşen tembel yüklenir;
  // masaüstü paketi mobil gezinmeyi taşımaz ve masaüstünde bu seçenek yoktur.
  const receive = isMobileShell ? import('../mobile/TransferReceive.svelte') : null;

  let step = $state<Step>('email');
  let email = $state('');
  let password = $state('');
  let showPass = $state(false);
  let name = $state('');
  let displayName = $state('');
  let finding = $state(false);
  let adding = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let editing = $state(false);
  let notFound = $state(false);
  let config = $state<DiscoveredConfig | null>(null);
  let imap = $state<ServerConfig>({ host: '', port: 993, security: 'tls', username: '' });
  let smtp = $state<ServerConfig>({ host: '', port: 465, security: 'tls', username: '' });
  let newAccountId = $state<string | null>(null);

  // JMAP
  let sessionUrl = $state('');
  let jmapUser = $state('');
  let jmapFinding = $state(false);

  // OAuth
  let providers = $state<AuthKind[]>([]);
  let provider = $state<Provider>('google');
  let grant = $state<OAuthGrant | null>(null);
  let signInToken = 0;

  // Sertifika onayı; onaylanınca `retry` yeniden çalışır.
  let cert = $state<{ targets: CertTarget[]; retry: () => void } | null>(null);

  const valid = $derived(/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim()));
  const domain = $derived(email.trim().split('@')[1]?.toLowerCase() ?? '');
  const domainProvider = $derived<Provider | null>(
    /^(gmail\.com|googlemail\.com)$/.test(domain)
      ? 'google'
      : /^(outlook\.com|hotmail\.com|live\.com|msn\.com)$/.test(domain)
        ? 'microsoft'
        : null,
  );
  const needsAppPassword = $derived(
    /^(gmail\.com|googlemail\.com|outlook\.com|hotmail\.com|live\.com|msn\.com|yahoo\.com|icloud\.com|me\.com|mac\.com)$/.test(domain),
  );
  const hasAccounts = $derived(mail.accounts.length > 0);
  const providerName = $derived(provider === 'google' ? 'Google' : 'Microsoft');

  onMount(async () => {
    try {
      providers = await call('oauth_providers');
    } catch {
      providers = [];
    }
  });

  function capital(s: string) {
    return s.charAt(0).toLocaleUpperCase('tr') + s.slice(1);
  }

  function defaults(addr = email.trim()) {
    const [local = '', dom = ''] = addr.toLowerCase().split('@');
    const known: Record<string, string> = { 'gmail.com': 'Gmail', 'googlemail.com': 'Gmail', 'outlook.com': 'Outlook', 'hotmail.com': 'Outlook', 'live.com': 'Outlook', 'icloud.com': 'iCloud', 'yahoo.com': 'Yahoo' };
    name ||= known[dom] ?? capital(dom.split('.')[0] ?? '');
    displayName ||= capital(local.split(/[._-]/)[0] ?? '');
  }

  function reset() {
    error = null;
    notice = null;
  }

  async function find() {
    if (!valid || finding) return;
    reset();
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
    reset();
    config = null;
    notFound = false;
    editing = true;
    imap = { host: `imap.${domain}`, port: 993, security: 'tls', username: email.trim() };
    smtp = { host: `smtp.${domain}`, port: 465, security: 'tls', username: email.trim() };
    defaults();
    step = 'found';
  }

  /** Hata sertifika güvensizliğiyse onay penceresini açar. */
  function handle(e: unknown, targets: CertTarget[], retry: () => void) {
    const err = toAppError(e);
    if (err.code === 'untrusted' && targets.length) cert = { targets, retry };
    else error = err.message;
  }

  async function added(id: string) {
    password = '';
    newAccountId = id;
    step = 'ready';
    await reloadAccounts();
  }

  async function add() {
    if (!password || adding) return;
    adding = true;
    reset();
    try {
      const account = await call('account_add', {
        input: { name: name.trim() || domain, email: email.trim(), displayName: displayName.trim() || email.trim(), color: null, imap, smtp, password },
      });
      await added(account.id);
    } catch (e) {
      handle(e, [imap, smtp].map(({ host, port, security }) => ({ host, port, security })), () => void add());
    } finally {
      adding = false;
    }
  }

  // ---------------------------------------------------------------- JMAP

  function openJmap() {
    reset();
    step = 'jmap';
    jmapUser ||= email.trim();
    if (valid) {
      defaults();
      if (!sessionUrl) void discoverJmap();
    }
  }

  async function discoverJmap() {
    if (!valid || jmapFinding) return;
    jmapFinding = true;
    notice = null;
    try {
      sessionUrl = await call('jmap_discover', { email: email.trim() });
    } catch {
      notice = t('onb.jmapNotFound');
      sessionUrl ||= `https://${domain}`;
    } finally {
      jmapFinding = false;
    }
  }

  function jmapTarget(): CertTarget[] {
    try {
      const u = new URL(sessionUrl.trim());
      return u.protocol === 'https:' ? [{ host: u.hostname, port: Number(u.port) || 443, security: 'tls' }] : [];
    } catch {
      return [];
    }
  }

  async function addJmap() {
    if (!password || !valid || !sessionUrl.trim() || adding) return;
    adding = true;
    reset();
    defaults();
    try {
      const account = await call('account_add_jmap', {
        input: {
          name: name.trim() || domain,
          email: email.trim(),
          displayName: displayName.trim() || email.trim(),
          color: null,
          sessionUrl: sessionUrl.trim(),
          username: jmapUser.trim() || email.trim(),
          password,
        },
      });
      await added(account.id);
    } catch (e) {
      handle(e, jmapTarget(), () => void addJmap());
    } finally {
      adding = false;
    }
  }

  // ---------------------------------------------------------------- OAuth

  function chooseProvider(p: Provider) {
    reset();
    if (providers.includes(p)) {
      void signIn(p);
      return;
    }
    // Bu derlemede istemci kimliği yok: uygulama parolasıyla IMAP.
    notice = t('onb.oauthUnavailable', { provider: p === 'google' ? 'Google' : 'Microsoft' });
    if (!email.includes('@')) email = p === 'google' ? '@gmail.com' : '@outlook.com';
    document.getElementById('onb-email')?.focus();
  }

  async function signIn(p: Provider) {
    const token = ++signInToken;
    provider = p;
    grant = null;
    reset();
    step = 'oauth';
    try {
      const g = await call('oauth_sign_in', { provider: p, email: valid ? email.trim() : null });
      if (token !== signInToken) return;
      grant = g;
      email = g.email;
      name = '';
      displayName = g.displayName ?? '';
      defaults(g.email);
    } catch (e) {
      if (token === signInToken) error = toAppError(e).message;
    }
  }

  function leaveOauth() {
    signInToken++;
    grant = null;
    reset();
    step = 'email';
  }

  async function addOauth() {
    if (!grant || adding) return;
    adding = true;
    reset();
    try {
      const account = await call('account_add_oauth', {
        input: { grantId: grant.grantId, name: name.trim() || domain, displayName: displayName.trim() || grant.email, color: null },
      });
      grant = null;
      await added(account.id);
    } catch (e) {
      const imapHost = provider === 'google' ? 'imap.gmail.com' : 'outlook.office365.com';
      handle(e, [{ host: imapHost, port: 993, security: 'tls' }], () => void addOauth());
    } finally {
      adding = false;
    }
  }

  // --------------------------------------------------------------- hazır

  // Eşitleme durumu uygulama genelindeki kayıttan okunur: hesap eklenirken
  // gelen olaylar bu adım açılmadan önce düşse de kaçmaz.
  const sync = $derived(newAccountId ? mail.sync[newAccountId] : undefined);
  /** İlk indirme bitti mi? Arka uç "çevrimiçi"yi ilk eşitlemeden sonra bildirir. */
  let downloaded = $state(false);
  $effect(() => {
    if (sync?.state === 'online') downloaded = true;
  });
  const progress = $derived(sync?.state === 'syncing' && sync.total ? sync : null);
  // İlk indirmeden önceki sorun; sonrası kenar çubuğunun durum satırında görünür.
  const problem = $derived.by(() => {
    if (downloaded || !sync) return null;
    if (sync.state === 'error') return sync.message;
    if (sync.state === 'offline') return t('onb.syncOffline');
    if (sync.state === 'untrusted') return t('sync.untrusted', { host: sync.host });
    return null;
  });

  function again() {
    step = 'email';
    email = '';
    password = '';
    name = '';
    displayName = '';
    sessionUrl = '';
    jmapUser = '';
    newAccountId = null;
    downloaded = false;
    reset();
  }

  function openInbox() {
    mail.screen = 'mail';
  }

  function certClosed(trusted: boolean) {
    const c = cert;
    cert = null;
    if (trusted) c?.retry();
  }

  const securities: Security[] = ['tls', 'starttls'];
  const sourceKey = $derived(config ? (`onb.foundFrom.${config.source}` as MessageKey) : null);
  // Parolanın nerede durduğu kabuğa göre: masaüstünde anahtar zinciri, Android'de Keystore.
  const keyringKey: MessageKey = isMobileShell ? 'mob.onboarding.keyring' : 'onb.keyring';
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
        {#if notice}<p class="note top" role="status">{notice}</p>{/if}
        <button class="btn-primary wide" disabled={!valid || finding} onclick={find}>{finding ? t('onb.finding') : t('onb.continue')}</button>
        <div class="or"><span></span>{t('onb.or')}<span></span></div>
        <div class="alts">
          {#if receive}
            <button class="alt" onclick={() => ((step = 'transfer'), reset())}><Icon name="qr" />{t('aktar.onboarding')}</button>
          {/if}
          <button class="alt" onclick={() => chooseProvider('google')}><Icon name="globe" />{t('onb.google')}</button>
          <button class="alt" onclick={() => chooseProvider('microsoft')}><Icon name="grid" />{t('onb.microsoft')}</button>
          <button class="alt" onclick={manual}><Icon name="settings" />{t('onb.manual')}</button>
          <button class="alt" onclick={openJmap}><Icon name="server" />{t('onb.jmap')}</button>
        </div>
      </div>
    {:else if step === 'transfer'}
      <div class="step">
        {#if receive}
          {#await receive then { default: TransferReceive }}
            <TransferReceive onback={() => (step = 'email')} ondone={openInbox} />
          {/await}
        {/if}
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
          {#if config?.otherDomain}
            <p class="note warn" role="note">{t('onb.otherDomain', { domain, host: imap.host })}</p>
          {/if}
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

        {#if domainProvider && providers.includes(domainProvider)}
          <button class="alt browser" onclick={() => signIn(domainProvider)}>
            <Icon name={domainProvider === 'google' ? 'globe' : 'grid'} />{t('onb.useBrowser')}
            <span class="meta">{domainProvider === 'google' ? 'Google' : 'Microsoft'}</span>
          </button>
        {:else if needsAppPassword}
          <p class="note">{t('onb.appPassword')}</p>
        {/if}
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
        <span class="meta keyring">{t(keyringKey)}</span>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <button class="btn-primary wide" disabled={!password || adding} onclick={add}>{adding ? t('onb.adding') : t('onb.add')}</button>
      </div>
    {:else if step === 'jmap'}
      <div class="step">
        <button class="link back" onclick={() => ((step = 'email'), reset())}><Icon name="chevronLeft" size={14} />{t('onb.back')}</button>
        <h1>{t('onb.jmapTitle')}</h1>
        <p class="lead">{t('onb.jmapLead')}</p>

        <label class="label" for="jmap-email">{t('onb.email')}</label>
        <input
          id="jmap-email"
          class="input"
          type="email"
          autocomplete="email"
          placeholder="ad@alanadi.com"
          bind:value={email}
          onblur={() => {
            if (!valid) return;
            jmapUser ||= email.trim();
            defaults();
            if (!sessionUrl) void discoverJmap();
          }}
        />

        <label class="label gap" for="jmap-url">{t('onb.sessionUrl')}</label>
        <div class="with-btn">
          <input id="jmap-url" class="input" type="url" spellcheck="false" placeholder="https://posta.alanadi.com" bind:value={sessionUrl} />
          <button class="btn-soft" disabled={!valid || jmapFinding} onclick={discoverJmap}>{jmapFinding ? t('onb.finding') : t('onb.find')}</button>
        </div>
        {#if notice}<p class="note top" role="status">{notice}</p>{/if}

        <div class="names">
          <label class="span"><span class="label">{t('onb.username')}</span><input class="input small" spellcheck="false" bind:value={jmapUser} /></label>
          <label><span class="label">{t('onb.name')}</span><input class="input small" bind:value={name} /></label>
          <label><span class="label">{t('onb.displayName')}</span><input class="input small" bind:value={displayName} /></label>
        </div>

        <label class="label" for="jmap-pass">{t('onb.password')}</label>
        <div class="pass">
          <input
            id="jmap-pass"
            class="input"
            type={showPass ? 'text' : 'password'}
            autocomplete="current-password"
            bind:value={password}
            onkeydown={(e) => e.key === 'Enter' && addJmap()}
          />
          <button class="link small show" onclick={() => (showPass = !showPass)}>{showPass ? t('onb.hide') : t('onb.show')}</button>
        </div>
        <span class="meta keyring">{t(keyringKey)}</span>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <button class="btn-primary wide" disabled={!password || !valid || !sessionUrl.trim() || adding} onclick={addJmap}>{adding ? t('onb.adding') : t('onb.add')}</button>
      </div>
    {:else if step === 'oauth'}
      <div class="step">
        <button class="link back" onclick={leaveOauth}><Icon name="chevronLeft" size={14} />{t('onb.back')}</button>
        {#if !grant}
          <span class="wait-icon" class:failed={!!error}><Icon name={provider === 'google' ? 'globe' : 'grid'} size={28} /></span>
          <h1>{t('onb.oauthWaiting')}</h1>
          <p class="lead">{t('onb.oauthLead', { provider: providerName })}</p>
          {#if error}
            <p class="error" role="alert">{error}</p>
            <button class="btn-primary wide" onclick={() => signIn(provider)}>{t('act.retry')}</button>
          {:else}
            <div class="bar"><div class="fill indeterminate"></div></div>
          {/if}
        {:else}
          <span class="ok-icon"><Icon name="check" size={32} /></span>
          <h1>{t('onb.oauthDone', { provider: providerName })}</h1>
          <p class="lead">{t('onb.oauthReady', { email: grant.email })}</p>
          <div class="names">
            <label><span class="label">{t('onb.name')}</span><input class="input small" bind:value={name} /></label>
            <label><span class="label">{t('onb.displayName')}</span><input class="input small" bind:value={displayName} /></label>
          </div>
          {#if error}<p class="error" role="alert">{error}</p>{/if}
          <button class="btn-primary wide" disabled={adding} onclick={addOauth}>{adding ? t('onb.adding') : t('onb.add')}</button>
        {/if}
      </div>
    {:else}
      <div class="step">
        <span class="ok-icon"><Icon name="check" size={32} /></span>
        <h1>{t('onb.readyTitle')}</h1>
        <p class="lead">{downloaded ? t('onb.readyLeadDone') : t('onb.readyLead')}</p>
        <div class="checks" role="status" aria-live="polite">
          <div><Icon name="check" size={16} class="ok" />{t('onb.stepFolders')}</div>
          {#if downloaded}
            <div><Icon name="check" size={16} class="ok" />{t('onb.stepMailDone')}</div>
          {:else}
            <div>
              <Icon name="sync" size={16} class="busy spin" />{t('onb.stepMail')}
              {#if progress}<span class="meta num step-count">{progress.done}/{progress.total}</span>{/if}
            </div>
            <div class="bar indent">
              <div class="fill" style:width={progress ? `${Math.max(6, (progress.done / progress.total) * 100)}%` : undefined} class:indeterminate={!progress}></div>
            </div>
          {/if}
          <div class:pending={!downloaded}>
            <Icon name={downloaded ? 'check' : 'bell'} size={16} class={downloaded ? 'ok' : undefined} />{downloaded ? t('onb.stepPush') : t('onb.stepPushWait')}
          </div>
          {#if problem}<p class="error" role="alert">{t('onb.syncProblem', { message: problem })}</p>{/if}
        </div>
        <button class="btn-primary wide" onclick={openInbox}>{t('onb.openInbox')}</button>
        <button class="link small again" onclick={again}>{t('onb.another')}</button>
      </div>
    {/if}
  </section>
</div>

{#if cert}<CertDialog targets={cert.targets} onclose={certClosed} />{/if}

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

  .names .span {
    grid-column: 1 / -1;
  }

  .gap {
    margin-top: 14px;
  }

  .with-btn {
    display: flex;
    gap: 8px;
  }

  .with-btn .btn-soft {
    height: 44px;
    padding: 0 16px;
  }

  .browser {
    margin-bottom: 14px;
  }

  .browser .meta {
    margin-left: auto;
  }

  .note {
    margin-bottom: 12px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .note.warn {
    margin: 10px 0 0;
    color: var(--amber);
  }

  .note.top {
    margin: 10px 0 0;
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

  .wait-icon {
    align-self: flex-start;
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    margin-bottom: 16px;
    border-radius: var(--r-card);
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .wait-icon.failed {
    background: var(--surface);
    color: var(--ink-3);
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

  .checks .pending {
    color: var(--ink-3);
  }

  .checks :global(.spin) {
    animation: spin 1.2s linear infinite;
  }

  .step-count {
    margin-left: auto;
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--surface);
    overflow: hidden;
  }

  .bar.indent {
    margin-left: 26px;
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

  @keyframes spin {
    to {
      transform: rotate(360deg);
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

  /* Mobil kabuk (`container: onboarding`): tam ekran, tek sütun, 44 px hedefler.
     Adım üstten başlar; klavye açılınca odaklı alan kayarak görünür kalır. */
  @container onboarding (max-width: 600px) {
    .form {
      padding: 20px 16px 32px;
    }

    .step {
      margin: 0 0 auto;
    }

    .alts,
    .names {
      grid-template-columns: minmax(0, 1fr);
    }

    .alt {
      height: 48px;
    }

    .back,
    .link.small,
    .again {
      min-height: 44px;
      display: inline-flex;
      align-items: center;
      margin-top: 0;
    }

    .back {
      margin-bottom: 8px;
    }

    .show {
      top: 0;
      height: 44px;
    }

    .with-btn {
      flex-wrap: wrap;
    }

    .with-btn .btn-soft {
      flex: 1;
    }

    .close {
      top: 8px;
      right: 8px;
    }
  }
</style>
