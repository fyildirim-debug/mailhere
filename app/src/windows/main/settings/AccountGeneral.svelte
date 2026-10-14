<script lang="ts">
  import { call, toAppError, type Account } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { isMobileShell } from '../../../lib/shell';
  import { reloadAccounts } from '../../../lib/stores/mail.svelte';
  import { accountUpdate, PALETTE } from './account';

  interface Props {
    account: Account;
  }

  let { account }: Props = $props();
  // İpuçları kabuğa göre: mobilde çekmece ve sistem bildirimi var, kart yok.
  const mobile = isMobileShell;

  // Sayfa açıkken hesap güncellenirse (başka bölümden) form yeniden doldurulur.
  let form = $state({ name: '', displayName: '', color: '', notify: true });
  $effect(() => {
    form = { name: account.name, displayName: account.displayName, color: account.color, notify: account.notify };
  });

  const dirty = $derived(
    form.name !== account.name || form.displayName !== account.displayName || form.color.toLowerCase() !== account.color.toLowerCase() || form.notify !== account.notify,
  );
  let saving = $state(false);
  let error = $state<string | null>(null);
  let confirmRemove = $state(false);

  async function save() {
    if (!dirty || saving || !form.name.trim()) return;
    saving = true;
    error = null;
    try {
      await call('account_update', {
        id: account.id,
        update: { ...accountUpdate(account), name: form.name.trim(), displayName: form.displayName.trim() || form.name.trim(), color: form.color, notify: form.notify },
      });
      await reloadAccounts();
      showToast({ text: t('toast.saved') });
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      saving = false;
    }
  }

  async function remove() {
    try {
      await call('account_remove', { id: account.id });
      await reloadAccounts();
      showToast({ text: t('acc.removed', { name: account.name }) });
    } catch (e) {
      error = toAppError(e).message;
    }
  }

  const authKey = $derived((account.kind === 'jmap' ? 'acc.kind.jmap' : `acc.via.${account.auth}`) as MessageKey);
</script>

<h3 class="set-h">{t('acc.identity')}</h3>
<div class="set-row">
  <label class="set-name" for="acc-name">{t('onb.name')}<span class="set-hint">{t(mobile ? 'mob.settings.accNameHint' : 'acc.nameHint')}</span></label>
  <input id="acc-name" class="input small" maxlength="60" bind:value={form.name} />
</div>
<div class="set-row">
  <label class="set-name" for="acc-display">{t('onb.displayName')}<span class="set-hint">{t('acc.displayHint')}</span></label>
  <input id="acc-display" class="input small" maxlength="120" bind:value={form.displayName} />
</div>
<div class="set-row">
  <span class="set-name" id="acc-color">{t('set.accountColor')}</span>
  <div class="swatches" role="radiogroup" aria-labelledby="acc-color">
    {#each PALETTE as c, i (c)}
      <button
        class="swatch"
        role="radio"
        aria-checked={form.color.toLowerCase() === c.toLowerCase()}
        aria-label="{t('set.accountColor')} {i + 1}"
        style:background={c}
        onclick={() => (form.color = c)}
      ></button>
    {/each}
  </div>
</div>
<div class="set-row top">
  <span class="set-name">{t('acc.notify')}<span class="set-hint">{t(mobile ? 'mob.settings.accNotifyHint' : 'acc.notifyHint')}</span></span>
  <input type="checkbox" role="switch" class="switch" aria-label={t('acc.notify')} bind:checked={form.notify} />
</div>
<div class="set-row">
  <span class="set-name">{t('onb.email')}</span>
  <span class="ro"><span class="trunc">{account.email}</span><span class="meta">{t(authKey)}</span></span>
</div>
{#if error}<p class="set-error" role="alert">{error}</p>{/if}
<div class="set-actions bar">
  {#if dirty}<button class="btn-ghost" onclick={() => (form = { name: account.name, displayName: account.displayName, color: account.color, notify: account.notify })}>{t('act.cancel')}</button>{/if}
  <button class="btn-primary small" disabled={!dirty || saving || !form.name.trim()} onclick={save}>{t('set.save')}</button>
</div>

<h3 class="set-h danger-h">{t('acc.danger')}</h3>
<div class="set-row top">
  <span class="set-name">{t('set.removeAccount')}<span class="set-hint">{t('acc.removeHint')}</span></span>
  {#if confirmRemove}
    <div class="confirm">
      <span class="meta">{t('set.removeConfirm')}</span>
      <div class="set-actions">
        <button class="btn-ghost" onclick={() => (confirmRemove = false)}>{t('act.cancel')}</button>
        <button class="btn-soft danger" onclick={remove}>{t('acc.removeNow')}</button>
      </div>
    </div>
  {:else}
    <button class="btn-soft danger fit" onclick={() => (confirmRemove = true)}>{t('set.removeAccount')}</button>
  {/if}
</div>

<style>
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
    box-shadow: inset 0 0 0 1px var(--line-strong);
    transition: transform var(--t-fast) var(--ease);
  }

  .swatch:hover {
    transform: scale(1.08);
  }

  .swatch[aria-checked='true'] {
    box-shadow:
      0 0 0 2px var(--paper),
      0 0 0 4px var(--accent);
  }

  .ro {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .bar {
    margin-top: 16px;
  }

  .danger-h {
    margin-top: 44px;
  }

  .danger {
    color: var(--danger);
  }

  .fit {
    justify-self: start;
  }

  .confirm {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
  }

  .confirm .set-actions {
    justify-content: flex-start;
  }

  @container settings (max-width: 600px) {
    .swatches {
      flex-wrap: wrap;
    }

    .swatch {
      width: 34px;
      height: 34px;
    }
  }
</style>
