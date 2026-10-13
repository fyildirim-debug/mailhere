<script lang="ts">
  import { call, toAppError, type Account, type CardPosition, type Density, type Language, type NotifyMode, type Theme } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { settings, updateSettings } from '../../lib/stores/settings.svelte';
  import { mail, reloadAccounts } from '../../lib/stores/mail.svelte';
  import { checkUpdate, installUpdate, update } from '../../lib/stores/update.svelte';

  const s = $derived(settings.value);

  const themes: [Theme, MessageKey][] = [
    ['system', 'set.theme.system'],
    ['light', 'set.theme.light'],
    ['dark', 'set.theme.dark'],
  ];
  const languages: [Language, string][] = [
    ['tr', 'Türkçe'],
    ['en', 'English'],
  ];
  const densities: [Density, MessageKey][] = [
    ['comfortable', 'set.density.comfortable'],
    ['compact', 'set.density.compact'],
  ];
  const modes: [NotifyMode, MessageKey][] = [
    ['people', 'set.notify.people'],
    ['all', 'set.notify.all'],
    ['vip', 'set.notify.vip'],
    ['off', 'set.notify.off'],
  ];
  const positions: [CardPosition, MessageKey][] = [
    ['bottomRight', 'set.pos.bottomRight'],
    ['topRight', 'set.pos.topRight'],
    ['bottomLeft', 'set.pos.bottomLeft'],
    ['topLeft', 'set.pos.topLeft'],
  ];
  const cardSeconds = [5, 8, 12, 20];
  const undoSeconds = [5, 10, 20, 30];

  let vipText = $state('');
  let confirmRemove = $state<string | null>(null);

  $effect(() => {
    vipText = settings.value.vip.join('\n');
  });

  function saveVip() {
    const vip = vipText
      .split(/[\n,;]+/)
      .map((x) => x.trim().toLowerCase())
      .filter((x) => x.includes('@'));
    void updateSettings({ vip });
  }

  async function removeAccount(id: string) {
    await call('account_remove', { id });
    confirmRemove = null;
    await reloadAccounts();
  }

  // Hesap renkleri kullanıcı verisidir (kenar çubuğu noktası); jeton değildir.
  const palette = ['#4C6FD8', '#2E9A61', '#D08A1E', '#9A5BD6', '#D0566E', '#2A9BA8'];

  let editing = $state<{ id: string; name: string; displayName: string; color: string; signature: string } | null>(null);
  let saving = $state(false);
  let editError = $state<string | null>(null);

  function edit(a: Account) {
    confirmRemove = null;
    editError = null;
    editing = { id: a.id, name: a.name, displayName: a.displayName, color: a.color, signature: a.signature ?? '' };
  }

  async function saveAccount() {
    const e = editing;
    if (!e || saving || !e.name.trim()) return;
    saving = true;
    editError = null;
    try {
      const signature = e.signature.replace(/\s+$/, '');
      await call('account_update', {
        id: e.id,
        update: { name: e.name.trim(), displayName: e.displayName.trim() || e.name.trim(), color: e.color, signature: signature || null },
      });
      editing = null;
      await reloadAccounts();
      showToast({ text: t('toast.saved') });
    } catch (err) {
      editError = toAppError(err).message;
    } finally {
      saving = false;
    }
  }

  function authLabel(a: Account): string {
    return a.kind === 'jmap' ? 'JMAP' : t(`set.auth.${a.auth}` as MessageKey);
  }

  /** Parmak izinin ilk sekiz baytı; tamamı ipucunda. */
  function shortFp(sha: string): string {
    return sha.split(':').slice(0, 8).join(':') + '…';
  }
</script>

<section class="settings" aria-labelledby="set-title">
  <div class="inner">
    <h1 id="set-title">{t('set.title')}</h1>

    <h2>{t('set.appearance')}</h2>
    <div class="row">
      <span class="name">{t('set.theme')}</span>
      <div class="seg-track" role="radiogroup" aria-label={t('set.theme')}>
        {#each themes as [v, k] (v)}
          <button class="seg" role="radio" aria-checked={s.theme === v} onclick={() => updateSettings({ theme: v })}>{t(k)}</button>
        {/each}
      </div>
    </div>
    <div class="row">
      <span class="name">{t('set.language')}</span>
      <div class="seg-track" role="radiogroup" aria-label={t('set.language')}>
        {#each languages as [v, label] (v)}
          <button class="seg" role="radio" aria-checked={s.language === v} onclick={() => updateSettings({ language: v })}>{label}</button>
        {/each}
      </div>
    </div>
    <div class="row">
      <span class="name">{t('set.density')}</span>
      <div class="seg-track" role="radiogroup" aria-label={t('set.density')}>
        {#each densities as [v, k] (v)}
          <button class="seg" role="radio" aria-checked={s.density === v} onclick={() => updateSettings({ density: v })}>{t(k)}</button>
        {/each}
      </div>
    </div>

    <h2>{t('set.notifications')}</h2>
    <div class="row top">
      <span class="name">{t('set.notify')}<span class="hint">{t('set.notifyHint')}</span></span>
      <div class="seg-track" role="radiogroup" aria-label={t('set.notify')}>
        {#each modes as [v, k] (v)}
          <button class="seg" role="radio" aria-checked={s.notify === v} onclick={() => updateSettings({ notify: v })}>{t(k)}</button>
        {/each}
      </div>
    </div>
    {#if s.notify === 'vip'}
      <div class="row top">
        <label class="name" for="vip">{t('set.vip')}<span class="hint">{t('set.vipHint')}</span></label>
        <textarea id="vip" class="input area" rows="3" bind:value={vipText} onblur={saveVip}></textarea>
      </div>
    {/if}
    <div class="row">
      <span class="name">{t('set.cardPosition')}</span>
      <div class="seg-track" role="radiogroup" aria-label={t('set.cardPosition')}>
        {#each positions as [v, k] (v)}
          <button class="seg" role="radio" aria-checked={s.cardPosition === v} onclick={() => updateSettings({ cardPosition: v })}>{t(k)}</button>
        {/each}
      </div>
    </div>
    <div class="row">
      <span class="name">{t('set.cardSeconds')}</span>
      <div class="seg-track" role="radiogroup" aria-label={t('set.cardSeconds')}>
        {#each cardSeconds as n (n)}
          <button class="seg num" role="radio" aria-checked={s.cardSeconds === n} onclick={() => updateSettings({ cardSeconds: n })}>{t('set.seconds', { n })}</button>
        {/each}
      </div>
    </div>
    <div class="row top">
      <span class="name">{t('set.quiet')}<span class="hint">{t('set.quietHint')}</span></span>
      <div class="quiet">
        <input
          type="checkbox"
          role="switch"
          class="switch"
          aria-label={t('set.quiet')}
          checked={s.quietStart != null}
          onchange={(e) =>
            updateSettings(
              (e.currentTarget as HTMLInputElement).checked ? { quietStart: '22:00', quietEnd: '08:00' } : { quietStart: null, quietEnd: null },
            )}
        />
        {#if s.quietStart != null}
          <label class="time"><span class="meta">{t('set.quietFrom')}</span>
            <input class="input small num" type="time" value={s.quietStart} onchange={(e) => updateSettings({ quietStart: (e.currentTarget as HTMLInputElement).value })} />
          </label>
          <label class="time"><span class="meta">{t('set.quietTo')}</span>
            <input class="input small num" type="time" value={s.quietEnd ?? '08:00'} onchange={(e) => updateSettings({ quietEnd: (e.currentTarget as HTMLInputElement).value })} />
          </label>
        {/if}
      </div>
    </div>
    <div class="row top">
      <span class="name">{t('set.native')}<span class="hint">{t('set.nativeHint')}</span></span>
      <input
        type="checkbox"
        role="switch"
        class="switch"
        aria-label={t('set.native')}
        checked={s.nativeNotifications}
        onchange={(e) => updateSettings({ nativeNotifications: (e.currentTarget as HTMLInputElement).checked })}
      />
    </div>
    <div class="row">
      <span class="name"></span>
      <button class="btn-soft" onclick={() => call('demo_notify')}><Icon name="bell" size={16} />{t('set.testCard')}</button>
    </div>

    <h2>{t('set.sending')}</h2>
    <div class="row">
      <span class="name">{t('set.undo')}</span>
      <div class="seg-track" role="radiogroup" aria-label={t('set.undo')}>
        {#each undoSeconds as n (n)}
          <button class="seg num" role="radio" aria-checked={s.undoSendSeconds === n} onclick={() => updateSettings({ undoSendSeconds: n })}>{t('set.seconds', { n })}</button>
        {/each}
      </div>
    </div>

    <h2>{t('set.privacy')}</h2>
    <div class="row top">
      <span class="name">{t('set.remoteAllow')}</span>
      <div class="list">
        {#each s.remoteAllow as who (who)}
          <div class="list-row">
            <span class="trunc">{who}</span>
            <button class="btn-ghost" onclick={() => updateSettings({ remoteAllow: s.remoteAllow.filter((x) => x !== who) })}>{t('set.remove')}</button>
          </div>
        {:else}
          <span class="meta">{t('set.remoteNone')}</span>
        {/each}
      </div>
    </div>

    <div class="row top">
      <span class="name">{t('set.trusted')}<span class="hint">{t('set.trustedHint')}</span></span>
      <div class="list">
        {#each s.trustedCerts as c (`${c.host}:${c.port}`)}
          <div class="list-row">
            <span class="trunc">{c.host}:{c.port}</span>
            <code class="fp" title={c.sha256}>{shortFp(c.sha256)}</code>
            <span class="grow"></span>
            <button class="btn-ghost" onclick={() => updateSettings({ trustedCerts: s.trustedCerts.filter((x) => x !== c) })}>{t('set.remove')}</button>
          </div>
        {:else}
          <span class="meta">{t('set.trustedNone')}</span>
        {/each}
      </div>
    </div>

    <h2>{t('set.accounts')}</h2>
    <div class="list wide">
      {#each mail.accounts as a (a.id)}
        <div class="list-row">
          <span class="dot" style:background={a.color}></span>
          <span class="acc-name">{a.name}</span>
          <span class="trunc meta">{a.email} · {authLabel(a)}</span>
          <span class="grow"></span>
          {#if confirmRemove === a.id}
            <span class="meta confirm">{t('set.removeConfirm')}</span>
            <button class="btn-ghost" onclick={() => (confirmRemove = null)}>{t('act.cancel')}</button>
            <button class="btn-ghost danger" onclick={() => removeAccount(a.id)}>{t('set.removeAccount')}</button>
          {:else if editing?.id !== a.id}
            <button class="btn-ghost" onclick={() => edit(a)}>{t('set.editAccount')}</button>
            <button class="btn-ghost" onclick={() => ((confirmRemove = a.id), (editing = null))}>{t('set.removeAccount')}</button>
          {/if}
        </div>
        {#if editing?.id === a.id}
          <div class="edit">
            <label><span class="label">{t('onb.name')}</span><input class="input small" bind:value={editing.name} /></label>
            <label><span class="label">{t('onb.displayName')}</span><input class="input small" bind:value={editing.displayName} /></label>
            <div class="span">
              <span class="label" id="color-{a.id}">{t('set.accountColor')}</span>
              <div class="swatches" role="radiogroup" aria-labelledby="color-{a.id}">
                {#each palette as c, i (c)}
                  <button
                    class="swatch"
                    role="radio"
                    aria-checked={editing.color.toLowerCase() === c.toLowerCase()}
                    aria-label="{t('set.accountColor')} {i + 1}"
                    style:background={c}
                    onclick={() => editing && (editing.color = c)}
                  ></button>
                {/each}
              </div>
            </div>
            <label class="span">
              <span class="label">{t('set.signature')}</span>
              <textarea class="input area" rows="4" bind:value={editing.signature}></textarea>
              <span class="hint">{t('set.signatureHint')}</span>
            </label>
            {#if editError}<p class="span error" role="alert">{editError}</p>{/if}
            <div class="span actions">
              <button class="btn-ghost" onclick={() => (editing = null)}>{t('act.cancel')}</button>
              <button class="btn-primary small" disabled={saving || !editing.name.trim()} onclick={saveAccount}>{t('set.save')}</button>
            </div>
          </div>
        {/if}
      {/each}
      <button class="btn-soft add" onclick={() => (mail.screen = 'onboarding')}><Icon name="plus" size={16} />{t('act.addAccount')}</button>
    </div>

    <h2>{t('set.updates')}</h2>
    <div class="row top">
      <span class="name">{t('set.autoUpdate')}<span class="hint">{t('set.autoUpdateHint')}</span></span>
      <input
        type="checkbox"
        role="switch"
        class="switch"
        aria-label={t('set.autoUpdate')}
        checked={s.autoUpdate}
        onchange={(e) => updateSettings({ autoUpdate: (e.currentTarget as HTMLInputElement).checked })}
      />
    </div>
    <div class="row top">
      <span class="name">{t('set.version', { version: '0.1.0' })}<span class="hint">{t('set.license')}</span></span>
      <div class="upd">
        {#if update.info}
          <div class="upd-new">
            <strong>{t('update.available', { version: update.info.version })}</strong>
            {#if update.info.notes}<span class="meta">{update.info.notes}</span>{/if}
          </div>
          <button class="btn-primary small" disabled={update.installing} onclick={installUpdate}>
            {update.installing ? t('update.installing') : t('update.install')}
          </button>
        {:else}
          <button class="btn-soft" disabled={update.checking} onclick={() => checkUpdate()}>
            <Icon name="refresh" size={16} />{update.checking ? t('set.checking') : t('set.checkUpdate')}
          </button>
          {#if update.upToDate}<span class="meta ok"><Icon name="check" size={14} />{t('set.upToDate')}</span>{/if}
        {/if}
        {#if update.error}<span class="error" role="alert">{update.error}</span>{/if}
      </div>
    </div>
  </div>
</section>

<style>
  .settings {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    background: var(--paper);
    border-left: 1px solid var(--line);
  }

  .inner {
    max-width: 760px;
    margin: 0 auto;
    padding: 36px 40px 56px;
    animation: fade-up var(--t-state) ease-out;
  }

  h1 {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  h2 {
    margin: 36px 0 8px;
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
  }

  .row {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    align-items: center;
    gap: 24px;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }

  .row.top {
    align-items: start;
  }

  .row .seg-track {
    justify-self: start;
  }

  .name {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-weight: 500;
  }

  .hint {
    font-size: var(--fs-meta);
    font-weight: 400;
    color: var(--ink-3);
  }

  .area {
    height: auto;
    padding: 10px 12px;
    resize: vertical;
  }

  .quiet {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 16px;
  }

  .time {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .time .input {
    width: 128px;
  }

  .switch {
    appearance: none;
    position: relative;
    width: 40px;
    height: 24px;
    margin: 0;
    flex: none;
    border-radius: 999px;
    background: var(--line-strong);
    cursor: pointer;
    transition: background var(--t-state);
  }

  .switch::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--paper);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
    transition: transform var(--t-state) var(--ease);
  }

  .switch:checked {
    background: var(--accent);
  }

  .switch:checked::after {
    transform: translateX(16px);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .list.wide {
    margin-top: 4px;
  }

  .list-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    padding: 0 4px;
    border-bottom: 1px solid var(--line);
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
  }

  .acc-name {
    font-weight: 500;
  }

  .grow {
    flex: 1;
  }

  .confirm {
    max-width: 260px;
  }

  .danger {
    color: var(--danger);
  }

  .add {
    align-self: flex-start;
    margin-top: 12px;
  }

  .fp {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .edit {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
    margin: 4px 0 8px;
    padding: 16px;
    border-radius: var(--r-card);
    background: var(--surface);
    border: 1px solid var(--line);
    animation: fade-up var(--t-state) ease-out;
  }

  .edit .span {
    grid-column: 1 / -1;
  }

  .edit .hint {
    display: block;
    margin-top: 6px;
  }

  .swatches {
    display: flex;
    gap: 10px;
  }

  .swatch {
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.08);
    transition: transform var(--t-fast) var(--ease);
  }

  .swatch:hover {
    transform: scale(1.08);
  }

  .swatch[aria-checked='true'] {
    box-shadow:
      0 0 0 2px var(--surface),
      0 0 0 4px var(--accent);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .error {
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  .upd {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
  }

  .upd-new {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex-basis: 100%;
  }

  .upd .ok {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ok);
  }
</style>
