<script lang="ts">
  // Hızlı yanıt düğmelerinin düzenleyicisi: ekle, sil, sırala, varsayılana dön.
  // Her değişiklik hemen kaydedilir (yalnız `quick_replies_set`).
  import { toAppError, QUICK_REPLIES_MAX, QUICK_REPLY_CHARS } from '../api';
  import { t } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';
  import { quickReplies, saveQuickReplies, settings } from '../stores/settings.svelte';

  interface Props {
    /** Dar pencere (hızlı okuma) için sıkışık görünüm. */
    compact?: boolean;
  }

  let { compact = false }: Props = $props();

  let list = $state<string[]>([]);
  let draft = $state('');
  let error = $state<string | null>(null);

  // Başka pencereden gelen değişiklik de yansısın.
  $effect(() => {
    list = [...quickReplies(settings.value)];
  });

  const custom = $derived(settings.value.quickReplies != null);
  const full = $derived(list.length >= QUICK_REPLIES_MAX);

  async function save(next: string[] | null) {
    error = null;
    try {
      await saveQuickReplies(next);
    } catch (e) {
      error = toAppError(e).message;
    }
  }

  function commit() {
    void save(list.map((x) => x.trim()).filter(Boolean));
  }

  function add() {
    const v = draft.trim();
    if (!v || full) return;
    list = [...list, v];
    draft = '';
    commit();
  }

  function remove(i: number) {
    list = list.filter((_, j) => j !== i);
    commit();
  }

  function shift(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= list.length) return;
    const next = [...list];
    [next[i], next[j]] = [next[j], next[i]];
    list = next;
    commit();
  }
</script>

<div class="qre" class:compact>
  <ol class="items">
    {#each list as _, i (i)}
      <li class="item">
        <input
          class="input small"
          aria-label={t('qr.item', { n: i + 1 })}
          maxlength={QUICK_REPLY_CHARS}
          bind:value={list[i]}
          onchange={commit}
          onkeydown={(e) => e.key === 'Enter' && (e.currentTarget as HTMLInputElement).blur()}
        />
        <button class="btn-icon small" aria-label={t('qr.up')} title={t('qr.up')} disabled={i === 0} onclick={() => shift(i, -1)}>
          <Icon name="arrowUp" size={14} />
        </button>
        <button class="btn-icon small" aria-label={t('qr.down')} title={t('qr.down')} disabled={i === list.length - 1} onclick={() => shift(i, 1)}>
          <Icon name="arrowDown" size={14} />
        </button>
        <button class="btn-icon small" aria-label={t('qr.remove', { text: list[i] })} title={t('qr.removeShort')} onclick={() => remove(i)}>
          <Icon name="x" size={14} />
        </button>
      </li>
    {:else}
      <li class="none meta">{t('qr.none')}</li>
    {/each}
  </ol>
  <div class="add">
    <input
      class="input small"
      placeholder={full ? t('qr.full', { n: QUICK_REPLIES_MAX }) : t('qr.placeholder')}
      aria-label={t('qr.new')}
      maxlength={QUICK_REPLY_CHARS}
      disabled={full}
      bind:value={draft}
      onkeydown={(e) => e.key === 'Enter' && add()}
    />
    <button class="btn-soft small" disabled={!draft.trim() || full} onclick={add}><Icon name="plus" size={14} />{t('qr.add')}</button>
  </div>
  <div class="foot">
    <span class="meta num">{list.length}/{QUICK_REPLIES_MAX}</span>
    {#if custom}<button class="link" onclick={() => save(null)}>{t('qr.reset')}</button>{/if}
  </div>
  {#if error}<p class="set-error" role="alert">{error}</p>{/if}
</div>

<style>
  .qre {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .items {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item,
  .add {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .item .input,
  .add .input {
    flex: 1;
    min-width: 0;
  }

  .add {
    gap: 8px;
  }

  .add .btn-soft {
    flex: none;
  }

  .btn-icon:disabled {
    opacity: 0.35;
    background: transparent;
  }

  .none {
    padding: 6px 0;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--fs-meta);
  }

  .compact .items {
    max-height: 196px;
    overflow-y: auto;
  }
</style>
