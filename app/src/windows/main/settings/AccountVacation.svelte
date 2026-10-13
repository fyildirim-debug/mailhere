<script lang="ts">
  import { call, toAppError, type Account, type Vacation } from '../../../lib/api';
  import { t } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';

  interface Props {
    account: Account;
  }

  let { account }: Props = $props();

  let loaded = $state(false);
  let supported = $state(false);
  let saved = $state<Vacation | null>(null);
  let form = $state<Vacation>({ enabled: false, fromDate: null, toDate: null, subject: '', body: '' });
  let loadError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);

  $effect(() => {
    const id = account.id;
    let alive = true;
    loaded = false;
    void (async () => {
      try {
        const v = await call('vacation_get', { accountId: id });
        if (!alive) return;
        supported = v != null;
        saved = v;
        if (v) form = { ...v };
      } catch (e) {
        if (alive) loadError = toAppError(e).message;
      } finally {
        if (alive) loaded = true;
      }
    })();
    return () => {
      alive = false;
    };
  });

  /** Tarih alanı (yerel gün) ↔ ms. */
  function dayValue(ms: number | null): string {
    if (ms == null) return '';
    const d = new Date(ms);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }

  function fromDay(v: string, end: boolean): number | null {
    if (!v) return null;
    const [y, m, d] = v.split('-').map(Number);
    return end ? new Date(y, m - 1, d, 23, 59, 59).getTime() : new Date(y, m - 1, d, 0, 0, 0).getTime();
  }

  const dirty = $derived(JSON.stringify(form) !== JSON.stringify(saved));
  const badRange = $derived(form.fromDate != null && form.toDate != null && form.toDate < form.fromDate);

  async function save() {
    if (!dirty || saving || badRange) return;
    saving = true;
    error = null;
    try {
      saved = await call('vacation_set', { accountId: account.id, vacation: { ...form } });
      form = { ...saved };
      showToast({ text: form.enabled ? t('vac.on') : t('vac.off') });
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      saving = false;
    }
  }
</script>

<h3 class="set-h">{t('vac.title')}</h3>
{#if !loaded}
  <p class="meta">{t('vac.loading')}</p>
{:else if loadError}
  <p class="set-error" role="alert">{t('vac.loadFailed', { message: loadError })}</p>
{:else if !supported}
  <p class="set-lead note"><Icon name="info" size={16} />{t('vac.unsupported')}</p>
{:else}
  <p class="set-lead">{t('vac.lead')}</p>
  <div class="set-row">
    <span class="set-name">{t('vac.enabled')}</span>
    <input type="checkbox" role="switch" class="switch" aria-label={t('vac.enabled')} bind:checked={form.enabled} />
  </div>
  <div class="set-row top">
    <span class="set-name">{t('vac.when')}<span class="set-hint">{t('vac.whenHint')}</span></span>
    <div class="dates">
      <label><span class="label">{t('vac.from')}</span><input class="input small num" type="date" value={dayValue(form.fromDate)} onchange={(e) => (form.fromDate = fromDay((e.currentTarget as HTMLInputElement).value, false))} /></label>
      <label><span class="label">{t('vac.to')}</span><input class="input small num" type="date" value={dayValue(form.toDate)} onchange={(e) => (form.toDate = fromDay((e.currentTarget as HTMLInputElement).value, true))} /></label>
    </div>
  </div>
  <div class="set-row">
    <label class="set-name" for="vac-subject">{t('tpl.subject')}</label>
    <input id="vac-subject" class="input small" maxlength="300" placeholder={t('vac.subjectHint')} bind:value={form.subject} />
  </div>
  <div class="set-row top">
    <label class="set-name" for="vac-body">{t('vac.body')}</label>
    <textarea id="vac-body" class="input" rows="6" placeholder={t('vac.bodyHint')} bind:value={form.body}></textarea>
  </div>
  {#if badRange}<p class="set-error" role="alert">{t('vac.badRange')}</p>{/if}
  {#if error}<p class="set-error" role="alert">{error}</p>{/if}
  <div class="set-actions bar">
    {#if dirty && saved}<button class="btn-ghost" onclick={() => saved && (form = { ...saved })}>{t('act.cancel')}</button>{/if}
    <button class="btn-primary small" disabled={!dirty || saving || badRange} onclick={save}>{t('set.save')}</button>
  </div>
{/if}

<style>
  .note {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .note :global(.icon) {
    margin-top: 2px;
  }

  .dates {
    display: flex;
    gap: 12px;
  }

  .dates .input {
    width: 168px;
  }

  .bar {
    margin-top: 16px;
  }
</style>
