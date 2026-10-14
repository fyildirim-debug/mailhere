<script lang="ts">
  // Postanın üst notları: etiket çipleri, yanıt hatırlatması şeridi ve elek
  // (ilk kez yazan gönderici: kabul / engelle). Masaüstü okuma alanı ve mobil
  // okuma ekranı aynı mantığı kullanır.
  import type { MessageDetail } from '../api';
  import { t, when } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';
  import { followUpDue } from '../util/followup';
  import { displayName } from '../util/format';
  import { screenerDecide, setFollowUp, setView } from '../stores/mail.svelte';
  import { tagsOf } from '../stores/tags.svelte';

  interface Props {
    detail: MessageDetail;
    /** Elek görünümünde: gönderici için kabul / engelle kararı sorulur. */
    screening: boolean;
    /** Etiket çipine dokunulup liste görünümü değişince (mobil listeye döner). */
    onnavigate?: () => void;
    /** Elek kararı verilince (mobil listeye döner). */
    onscreened?: () => void;
  }

  let { detail: d, screening, onnavigate, onscreened }: Props = $props();

  const tags = $derived(tagsOf(d.tags));

  function openTag(id: string) {
    setView({ kind: 'tag', id });
    onnavigate?.();
  }

  async function decide(allow: boolean) {
    await screenerDecide(d.accountId, d.from.email, allow);
    onscreened?.();
  }
</script>

{#if tags.length}
  <div class="tagchips" role="group" aria-label={t('list.tagsOf', { list: tags.map((x) => x.name).join(', ') })}>
    {#each tags as tg (tg.id)}
      <button class="tagchip" title={t('tag.open')} onclick={() => openTag(tg.id)}>
        <span class="tdot" style:background={tg.color}></span>{tg.name}
      </button>
    {/each}
  </div>
{/if}

{#if d.followUp != null}
  <div class="strip follow" role="note">
    <Icon name="followUp" size={16} />
    <span class="grow">{followUpDue(d.followUp, Date.now()) ? t('follow.dueSince', { when: when(d.followUp) }) : t('follow.pending', { when: when(d.followUp) })}</span>
    <button class="link small" onclick={() => setFollowUp(d.id, null)}>{t('follow.clear')}</button>
  </div>
{/if}

{#if screening}
  <div class="strip screen" role="note">
    <Icon name="sieve" size={16} />
    <span class="grow">
      <span class="screen-title">{t('screen.first', { name: displayName(d.from) })}</span>
      <span class="meta">{t('screen.firstHint')}</span>
    </span>
    <button class="btn-primary small" onclick={() => decide(true)}>{t('screen.allow')}</button>
    <button class="btn-soft small danger" onclick={() => decide(false)}>{t('screen.block')}</button>
  </div>
{/if}

<style>
  .tagchips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
  }

  .tagchip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px 0 8px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface);
    color: var(--ink-2);
    font-size: var(--fs-meta);
    font-weight: 500;
    transition: border-color var(--t-fast);
  }

  .tagchip:hover {
    border-color: var(--line-strong);
    color: var(--ink);
  }

  .tdot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .strip {
    margin-top: 18px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
    color: var(--ink-2);
    font-size: var(--fs-meta);
  }

  .strip + .strip {
    margin-top: 8px;
  }

  .strip.follow :global(.icon) {
    color: var(--accent-text);
  }

  .strip.screen {
    padding: 10px 12px;
    color: var(--ink);
    font-size: var(--fs-body);
    flex-wrap: wrap;
  }

  .strip.screen > .grow {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1 1 160px;
  }

  .screen-title {
    font-weight: 500;
  }

  .btn-soft.danger {
    color: var(--danger);
  }

  .grow {
    flex: 1;
  }
</style>
