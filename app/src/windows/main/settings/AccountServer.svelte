<script lang="ts">
  import { call, toAppError, type Account, type Security, type ServerConfig } from '../../../lib/api';
  import { t } from '../../../lib/i18n/index.svelte';
  import CertDialog, { type CertTarget } from '../../../lib/mail/CertDialog.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { reloadAccounts } from '../../../lib/stores/mail.svelte';

  interface Props {
    account: Account;
  }

  let { account }: Props = $props();

  const editable = $derived(account.kind === 'imap' && account.auth === 'password');
  const oauth = $derived(account.auth !== 'password');
  const securities: Security[] = ['tls', 'starttls'];

  const blank: ServerConfig = { host: '', port: 993, security: 'tls', username: '' };
  let imap = $state<ServerConfig>({ ...blank });
  let smtp = $state<ServerConfig>({ ...blank, port: 465 });
  let password = $state('');
  let showPass = $state(false);

  function reset() {
    imap = { ...(account.imap ?? blank) };
    smtp = { ...(account.smtp ?? { ...blank, port: 465 }) };
    password = '';
  }
  $effect(() => {
    void account;
    reset();
  });

  const same = (a: ServerConfig | null, b: ServerConfig) =>
    !!a && a.host.trim().toLowerCase() === b.host.trim().toLowerCase() && a.port === b.port && a.username.trim() === b.username.trim();
  /** Sunucu adı, kapı ya da kullanıcı adı değişti mi? (Parola yeniden istenir.) */
  const moved = $derived(!same(account.imap, imap) || !same(account.smtp, smtp));
  const dirty = $derived(moved || imap.security !== account.imap?.security || smtp.security !== account.smtp?.security || !!password);
  const valid = $derived(
    [imap, smtp].every((c) => c.host.trim() && c.port > 0 && c.port < 65536 && c.username.trim()) && (!moved || !!password),
  );

  let saving = $state(false);
  let error = $state<string | null>(null);
  let cert = $state<{ targets: CertTarget[]; retry: () => void } | null>(null);

  async function save() {
    if (!dirty || !valid || saving) return;
    saving = true;
    error = null;
    try {
      await call('account_update_server', {
        id: account.id,
        update: { imap: { ...imap, host: imap.host.trim(), username: imap.username.trim() }, smtp: { ...smtp, host: smtp.host.trim(), username: smtp.username.trim() }, password: password || null },
      });
      password = '';
      await reloadAccounts();
      showToast({ text: t('srv.saved') });
    } catch (e) {
      const err = toAppError(e);
      if (err.code === 'untrusted') {
        cert = { targets: [imap, smtp].map(({ host, port, security }) => ({ host: host.trim(), port, security })), retry: () => void save() };
      } else error = err.message;
    } finally {
      saving = false;
    }
  }

  function certClosed(trusted: boolean) {
    const c = cert;
    cert = null;
    if (trusted) c?.retry();
  }

  // ------------------------------------------------------------ parola

  let newPass = $state('');
  let showNew = $state(false);
  let passBusy = $state(false);
  let passError = $state<string | null>(null);

  async function changePassword() {
    if (!newPass || passBusy) return;
    passBusy = true;
    passError = null;
    try {
      await call('account_set_password', { id: account.id, password: newPass });
      newPass = '';
      showToast({ text: t('srv.passwordSaved') });
    } catch (e) {
      passError = toAppError(e).message;
    } finally {
      passBusy = false;
    }
  }
</script>

{#if account.kind === 'jmap'}
  <h3 class="set-h">{t('srv.jmapTitle')}</h3>
  <div class="set-row">
    <span class="set-name">{t('onb.sessionUrl')}</span>
    <span class="trunc mono" title={account.jmapUrl ?? ''}>{account.jmapUrl}</span>
  </div>
  <p class="set-lead note">{t('srv.jmapNote')}</p>
{:else}
  <h3 class="set-h">{t('srv.title')}</h3>
  {#if oauth}
    <p class="set-lead note"><Icon name="lock" size={16} />{t('srv.oauthNote', { provider: account.auth === 'google' ? 'Google' : 'Microsoft' })}</p>
  {:else}
    <p class="set-lead">{t('srv.lead')}</p>
  {/if}
  {#each [{ key: 'onb.incoming', cfg: imap, kind: 'IMAP' }, { key: 'onb.outgoing', cfg: smtp, kind: 'SMTP' }] as row (row.kind)}
    <fieldset class="srv" disabled={!editable}>
      <legend>{t(row.key as 'onb.incoming')} <span class="meta">{row.kind}</span></legend>
      <label class="host"><span class="label">{t('onb.host')}</span><input class="input small" spellcheck="false" bind:value={row.cfg.host} /></label>
      <label><span class="label">{t('onb.port')}</span><input class="input small num" type="number" min="1" max="65535" bind:value={row.cfg.port} /></label>
      <label><span class="label">{t('onb.security')}</span>
        <select class="input small" bind:value={row.cfg.security}>
          {#each securities as s (s)}<option value={s}>{s === 'tls' ? 'SSL/TLS' : 'STARTTLS'}</option>{/each}
        </select>
      </label>
      <label class="user"><span class="label">{t('onb.username')}</span><input class="input small" spellcheck="false" bind:value={row.cfg.username} /></label>
    </fieldset>
  {/each}
  {#if editable}
    {#if moved}
      <div class="set-row top warn-row">
        <label class="set-name" for="srv-pass">{t('srv.passwordAgain')}<span class="set-hint">{t('srv.passwordAgainHint')}</span></label>
        <div class="pass">
          <input id="srv-pass" class="input small" type={showPass ? 'text' : 'password'} autocomplete="current-password" bind:value={password} />
          <button class="link show" onclick={() => (showPass = !showPass)}>{showPass ? t('onb.hide') : t('onb.show')}</button>
        </div>
      </div>
    {/if}
    {#if error}<p class="set-error" role="alert">{error}</p>{/if}
    <div class="set-actions bar">
      {#if dirty}<button class="btn-ghost" onclick={reset}>{t('act.cancel')}</button>{/if}
      <button class="btn-primary small" disabled={!dirty || !valid || saving} onclick={save}>{saving ? t('srv.testing') : t('srv.save')}</button>
    </div>
  {/if}
{/if}

{#if !oauth}
  <h3 class="set-h">{t('srv.passwordTitle')}</h3>
  <p class="set-lead">{t('srv.passwordLead')}</p>
  <div class="set-row">
    <label class="set-name" for="srv-new">{t('srv.newPassword')}</label>
    <div class="pass">
      <input
        id="srv-new"
        class="input small"
        type={showNew ? 'text' : 'password'}
        autocomplete="new-password"
        bind:value={newPass}
        onkeydown={(e) => e.key === 'Enter' && changePassword()}
      />
      <button class="link show" onclick={() => (showNew = !showNew)}>{showNew ? t('onb.hide') : t('onb.show')}</button>
    </div>
  </div>
  {#if passError}<p class="set-error" role="alert">{passError}</p>{/if}
  <div class="set-actions bar">
    <button class="btn-primary small" disabled={!newPass || passBusy} onclick={changePassword}>{passBusy ? t('srv.testing') : t('srv.updatePassword')}</button>
  </div>
{/if}

{#if cert}<CertDialog targets={cert.targets} onclose={certClosed} />{/if}

<style>
  .srv {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 96px 132px;
    gap: 10px 12px;
    margin: 14px 0 0;
    padding: 12px 16px 16px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
  }

  .srv:disabled {
    color: var(--ink-2);
  }

  .srv:disabled .input {
    background: var(--surface);
    color: var(--ink-2);
  }

  legend {
    padding: 0 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .user {
    grid-column: 1 / -1;
  }

  .note {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
  }

  .pass {
    position: relative;
    max-width: 360px;
  }

  .pass .input {
    padding-right: 72px;
  }

  .show {
    position: absolute;
    right: 10px;
    top: 8px;
    font-size: var(--fs-meta);
  }

  .warn-row {
    margin-top: 8px;
  }

  .bar {
    margin-top: 16px;
  }
</style>
