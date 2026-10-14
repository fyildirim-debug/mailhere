<script lang="ts">
  // Etiket yönetimi: ekle, yeniden adlandır, renk seç, sil. Etiketler yereldir.
  import { toAppError, TAG_COLORS, TAG_NAME_CHARS, TAGS_MAX, type Tag } from '../../../lib/api';
  import { t } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { refreshList, setView } from '../../../lib/stores/mail.svelte';
  import { deleteTag, saveTag, tagStore } from '../../../lib/stores/tags.svelte';

  let editing = $state<Tag | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);
  let confirmId = $state<string | null>(null);

  const full = $derived(tagStore.list.length >= TAGS_MAX);

  function startNew() {
    error = null;
    confirmId = null;
    editing = { id: '', name: '', color: TAG_COLORS[tagStore.list.length % TAG_COLORS.length] };
  }

  function startEdit(tag: Tag) {
    error = null;
    confirmId = null;
    editing = { ...tag };
  }

  async function save() {
    const e = editing;
    if (!e || saving || !e.name.trim()) return;
    saving = true;
    error = null;
    try {
      await saveTag($state.snapshot(e));
      editing = null;
      void refreshList();
      showToast({ text: t('toast.saved') });
    } catch (err) {
      error = toAppError(err).message;
    } finally {
      saving = false;
    }
  }

  async function remove(tag: Tag) {
    confirmId = null;
    try {
      await deleteTag(tag.id);
      void refreshList();
      showToast({ text: t('tag.removed', { name: tag.name }) });
    } catch (err) {
      showToast({ text: t('toast.error', { message: toAppError(err).message }) });
    }
  }
</script>

<h2 class="page">{t('tag.title')}</h2>
<p class="set-lead">{t('tag.lead')}</p>

<div class="tags">
  {#each tagStore.list as tag (tag.id)}
    {#if editing?.id === tag.id}
      {@render form()}
    {:else}
      <div class="row">
        <span class="dot" style:background={tag.color}></span>
        <span class="trunc name">{tag.name}</span>
        <button class="btn-ghost" onclick={() => setView({ kind: 'tag', id: tag.id })}>{t('tag.open')}</button>
        <button class="btn-ghost" onclick={() => startEdit(tag)}>{t('set.editAccount')}</button>
        {#if confirmId === tag.id}
          <span class="meta">{t('tag.removeHint')}</span>
          <button class="btn-ghost" onclick={() => (confirmId = null)}>{t('act.cancel')}</button>
          <button class="btn-ghost danger" onclick={() => remove(tag)}>{t('acc.removeNow')}</button>
        {:else}
          <button class="btn-ghost" onclick={() => ((confirmId = tag.id), (editing = null))}>{t('set.remove')}</button>
        {/if}
      </div>
    {/if}
  {:else}
    {#if !editing}<p class="meta none">{t('tag.none')}</p>{/if}
  {/each}
  {#if editing && !editing.id}{@render form()}{/if}
  {#if !editing}
    <button class="btn-soft fit add" disabled={full} title={full ? t('tag.full', { n: TAGS_MAX }) : undefined} onclick={startNew}>
      <Icon name="plus" size={16} />{t('tag.new')}
    </button>
  {/if}
</div>

<h3 class="set-h">{t('pin.pin')}</h3>
<p class="set-lead">{t('pin.local')}</p>

{#snippet form()}
  {#if editing}
    <div class="edit">
      <label><span class="label">{t('tag.name')}</span><input class="input small" maxlength={TAG_NAME_CHARS} placeholder={t('tag.namePlaceholder')} bind:value={editing.name} /></label>
      <div>
        <span class="label" id="tag-color">{t('tag.color')}</span>
        <div class="swatches" role="radiogroup" aria-labelledby="tag-color">
          {#each TAG_COLORS as c, i (c)}
            <button
              class="swatch"
              role="radio"
              aria-checked={editing.color.toLowerCase() === c.toLowerCase()}
              aria-label={t('tag.colorN', { n: i + 1 })}
              style:background={c}
              onclick={() => {
                if (editing) editing.color = c;
              }}
            ></button>
          {/each}
        </div>
      </div>
      {#if error}<p class="span set-error" role="alert">{error}</p>{/if}
      <div class="span set-actions">
        <button class="btn-ghost" onclick={() => ((editing = null), (error = null))}>{t('act.cancel')}</button>
        <button class="btn-primary small" disabled={saving || !editing.name.trim()} onclick={save}>{t('set.save')}</button>
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

  .tags {
    display: flex;
    flex-direction: column;
    margin-top: 8px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 52px;
    border-bottom: 1px solid var(--line);
  }

  .dot {
    flex: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }

  .name {
    flex: 1;
    font-weight: 500;
  }

  .danger {
    color: var(--danger);
  }

  .none {
    padding: 8px 0;
    max-width: 62ch;
  }

  .fit {
    align-self: flex-start;
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

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    padding-top: 4px;
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
</style>
