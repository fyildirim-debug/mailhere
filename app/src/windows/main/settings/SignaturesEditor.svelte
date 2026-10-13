<script lang="ts">
  import { call, toAppError, type Account, type Signature } from '../../../lib/api';
  import { t } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { reloadAccounts } from '../../../lib/stores/mail.svelte';
  import { newLocalId } from '../../../lib/util/id';
  import { accountUpdate } from './account';

  interface Props {
    account: Account;
  }

  let { account }: Props = $props();

  let list = $state<Signature[]>([]);
  let forNew = $state<string | null>(null);
  let forReply = $state<string | null>(null);

  function reset() {
    list = account.signatures.map((s) => ({ ...s }));
    forNew = account.signatureNew;
    forReply = account.signatureReply;
  }
  $effect(() => {
    void account;
    reset();
  });

  const dirty = $derived(
    JSON.stringify(list) !== JSON.stringify(account.signatures) || forNew !== account.signatureNew || forReply !== account.signatureReply,
  );
  let saving = $state(false);
  let error = $state<string | null>(null);

  function add() {
    // Kimliği burada verilir: varsayılan seçimi kaydetmeden önce de gösterebilsin.
    const id = newLocalId();
    list = [...list, { id, name: t('sig.defaultName', { n: list.length + 1 }), body: '' }];
    // İlk imza iki yerde de varsayılan olsun.
    if (list.length === 1) {
      forNew = id;
      forReply = id;
    }
    queueMicrotask(() => document.getElementById(`sig-body-${id}`)?.focus());
  }

  function remove(id: string) {
    list = list.filter((s) => s.id !== id);
    if (forNew === id) forNew = null;
    if (forReply === id) forReply = null;
  }

  async function save() {
    if (!dirty || saving) return;
    saving = true;
    error = null;
    try {
      await call('account_update', {
        id: account.id,
        update: { ...accountUpdate(account), signatures: list.map((s) => ({ ...s })), signatureNew: forNew, signatureReply: forReply },
      });
      await reloadAccounts();
      showToast({ text: t('toast.saved') });
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      saving = false;
    }
  }
</script>

<p class="set-lead">{t('sig.lead')}</p>

<div class="sigs">
  {#each list as sig (sig.id)}
    <div class="sig">
      <div class="sig-head">
        <input class="input small name" aria-label={t('sig.name')} maxlength="60" bind:value={sig.name} />
        <button class="btn-icon small" aria-label={t('sig.remove', { name: sig.name })} title={t('set.remove')} onclick={() => remove(sig.id)}>
          <Icon name="trash" size={16} />
        </button>
      </div>
      <textarea id="sig-body-{sig.id}" class="input body" rows="4" aria-label={t('sig.body', { name: sig.name })} bind:value={sig.body}></textarea>
    </div>
  {:else}
    <p class="meta none">{t('sig.none')}</p>
  {/each}
  <button class="btn-soft fit" disabled={list.length >= 20} onclick={add}><Icon name="plus" size={16} />{t('sig.add')}</button>
</div>

<h3 class="set-h">{t('sig.defaults')}</h3>
<div class="set-row">
  <label class="set-name" for="sig-new">{t('sig.forNew')}</label>
  <select id="sig-new" class="input small pick" bind:value={forNew}>
    <option value={null}>{t('sig.noneOption')}</option>
    {#each list as sig (sig.id)}<option value={sig.id}>{sig.name || t('sig.unnamed')}</option>{/each}
  </select>
</div>
<div class="set-row">
  <label class="set-name" for="sig-reply">{t('sig.forReply')}</label>
  <select id="sig-reply" class="input small pick" bind:value={forReply}>
    <option value={null}>{t('sig.noneOption')}</option>
    {#each list as sig (sig.id)}<option value={sig.id}>{sig.name || t('sig.unnamed')}</option>{/each}
  </select>
</div>
{#if error}<p class="set-error" role="alert">{error}</p>{/if}
<div class="set-actions bar">
  {#if dirty}<button class="btn-ghost" onclick={reset}>{t('act.cancel')}</button>{/if}
  <button class="btn-primary small" disabled={!dirty || saving} onclick={save}>{t('set.save')}</button>
</div>

<style>
  .sigs {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-top: 12px;
  }

  .sig {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-bottom: 14px;
    border-bottom: 1px solid var(--line);
  }

  .sig-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .name {
    max-width: 280px;
    font-weight: 500;
  }

  .body {
    font-family: inherit;
  }

  .none {
    padding: 4px 0;
  }

  .fit {
    align-self: flex-start;
  }

  .pick {
    max-width: 320px;
  }

  .bar {
    margin-top: 16px;
  }
</style>
