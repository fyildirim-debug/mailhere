<script lang="ts">
  import { toAppError, type Template } from '../../../lib/api';
  import { t } from '../../../lib/i18n/index.svelte';
  import QuickRepliesEditor from '../../../lib/mail/QuickRepliesEditor.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { openSettings } from '../../../lib/stores/mail.svelte';
  import { settings, updateSettings } from '../../../lib/stores/settings.svelte';

  const s = $derived(settings.value);
  const undoSeconds = [5, 10, 20, 30];

  // ---------------------------------------------------------------- şablonlar

  let editing = $state<Template | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);

  function startNew() {
    error = null;
    editing = { id: '', name: '', subject: '', body: '' };
  }

  async function save() {
    const e = editing;
    if (!e || saving || (!e.name.trim() && !e.body.trim())) return;
    saving = true;
    error = null;
    try {
      const list = e.id ? s.templates.map((x) => (x.id === e.id ? { ...e } : x)) : [...s.templates, { ...e }];
      await updateSettings({ templates: list });
      editing = null;
      showToast({ text: t('toast.saved') });
    } catch (err) {
      error = toAppError(err).message;
    } finally {
      saving = false;
    }
  }

  async function remove(tpl: Template) {
    const before = s.templates;
    await updateSettings({ templates: before.filter((x) => x.id !== tpl.id) });
    showToast({
      text: t('tpl.removed', { name: tpl.name }),
      action: { label: t('act.undo'), run: () => updateSettings({ templates: before }) },
    });
  }
</script>

<h2 class="page">{t('set.nav.compose')}</h2>

<h3 class="set-h">{t('set.sending')}</h3>
<div class="set-row top">
  <span class="set-name">{t('set.undo')}<span class="set-hint">{t('set.undoHint')}</span></span>
  <div class="seg-track" role="radiogroup" aria-label={t('set.undo')}>
    {#each undoSeconds as n (n)}
      <button class="seg num" role="radio" aria-checked={s.undoSendSeconds === n} onclick={() => updateSettings({ undoSendSeconds: n })}>{t('set.seconds', { n })}</button>
    {/each}
  </div>
</div>
<div class="set-row top">
  <span class="set-name">{t('set.signatures')}<span class="set-hint">{t('set.signaturesHint')}</span></span>
  <button class="btn-soft fit" onclick={() => openSettings('accounts')}><Icon name="pen" size={16} />{t('set.signaturesGo')}</button>
</div>

<h3 class="set-h">{t('qr.title')}</h3>
<p class="set-lead">{t('qr.lead')}</p>
<div class="box">
  <QuickRepliesEditor />
</div>

<h3 class="set-h">{t('tpl.title')}</h3>
<p class="set-lead">{t('tpl.lead')}</p>
<div class="tpls">
  {#each s.templates as tpl (tpl.id)}
    {#if editing?.id === tpl.id}
      {@render form()}
    {:else}
      <div class="tpl">
        <Icon name="template" size={18} />
        <div class="tpl-text">
          <span class="trunc tpl-name">{tpl.name}</span>
          <span class="trunc meta">{tpl.subject || tpl.body.replace(/\s+/g, ' ').slice(0, 120) || t('tpl.empty')}</span>
        </div>
        <button class="btn-ghost" onclick={() => ((editing = { ...tpl }), (error = null))}>{t('set.editAccount')}</button>
        <button class="btn-ghost" onclick={() => remove(tpl)}>{t('set.remove')}</button>
      </div>
    {/if}
  {:else}
    {#if !editing}<p class="meta none">{t('tpl.none')}</p>{/if}
  {/each}
  {#if editing && !editing.id}{@render form()}{/if}
  {#if !editing}
    <button class="btn-soft fit add" onclick={startNew}><Icon name="plus" size={16} />{t('tpl.new')}</button>
  {/if}
</div>

{#snippet form()}
  {#if editing}
    <div class="edit">
      <label><span class="label">{t('tpl.name')}</span><input class="input small" maxlength="80" bind:value={editing.name} /></label>
      <label><span class="label">{t('tpl.subject')}</span><input class="input small" maxlength="300" bind:value={editing.subject} placeholder={t('tpl.subjectHint')} /></label>
      <label class="span"><span class="label">{t('tpl.body')}</span><textarea class="input" rows="6" bind:value={editing.body}></textarea></label>
      {#if error}<p class="span set-error" role="alert">{error}</p>{/if}
      <div class="span set-actions">
        <button class="btn-ghost" onclick={() => (editing = null)}>{t('act.cancel')}</button>
        <button class="btn-primary small" disabled={saving || (!editing.name.trim() && !editing.body.trim())} onclick={save}>{t('set.save')}</button>
      </div>
    </div>
  {/if}
{/snippet}

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .fit {
    justify-self: start;
    align-self: start;
  }

  .box {
    max-width: 560px;
    padding: 4px 0 8px;
  }

  .tpls {
    display: flex;
    flex-direction: column;
  }

  .tpl {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 56px;
    border-bottom: 1px solid var(--line);
    color: var(--ink-2);
  }

  .tpl-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .tpl-name {
    color: var(--ink);
    font-weight: 500;
  }

  .none {
    padding: 8px 0;
  }

  .add {
    margin-top: 14px;
  }

  .edit {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
    margin: 8px 0;
    padding: 16px;
    border-radius: var(--r-card);
    background: var(--surface);
    border: 1px solid var(--line);
    animation: fade-up var(--t-state) ease-out;
  }

  .span {
    grid-column: 1 / -1;
  }
</style>
