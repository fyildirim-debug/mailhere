<script lang="ts">
  // Ekler alt sayfası (mobil okuma): her ek için Aç, Kaydet ve Paylaş. Satırın
  // kendisi eki açar; sağdaki iki ikon düğme (44 px) kaydeder ve paylaşır.
  // Kaydetme Android'in belge oluşturucusunu (`attachment_save`), paylaşma
  // sistemin paylaşım sayfasını (`attachment_share`) açar.
  //
  // İş bitince sayfa kapanır ve sonuç toast'ta söylenir ("Kaydedildi"; hata:
  // ne oldu + ne yapılabilir). Kullanıcı kaydetmekten vazgeçerse (`null`)
  // hiçbir şey söylenmez, sayfa açık kalır.
  import { call, toAppError, type Attachment, type MessageDetail } from '../../lib/api';
  import { locale, t, type MessageKey } from '../../lib/i18n/index.svelte';
  import FileBadge from '../../lib/ui/FileBadge.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Sheet from '../../lib/ui/Sheet.svelte';
  import { isLockedError } from '../../lib/stores/lock.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { fileSize } from '../../lib/util/format';

  interface Props {
    detail: MessageDetail;
    onclose: () => void;
  }

  let { detail: d, onclose }: Props = $props();

  type Kind = 'open' | 'save' | 'share';
  const failed: Record<Kind, MessageKey> = { open: 'mob.att.openFailed', save: 'mob.att.saveFailed', share: 'mob.att.shareFailed' };

  /** Süren iş ("save-2"); bitene dek aynı düğme yeniden çalışmaz. */
  let busy = $state<string | null>(null);

  async function run(kind: Kind, a: Attachment) {
    if (busy) return;
    busy = `${kind}-${a.index}`;
    try {
      if (kind === 'save') {
        const where = await call('attachment_save', { id: d.id, index: a.index });
        // Vazgeçildi: sessizce sayfada kalınır.
        if (where == null) return;
        onclose();
        showToast({ text: t('mob.att.saved') });
        return;
      }
      await call(kind === 'open' ? 'attachment_open' : 'attachment_share', { id: d.id, index: a.index });
      onclose();
    } catch (e) {
      onclose();
      if (!isLockedError(e)) showToast({ text: t(failed[kind], { name: a.name, message: toAppError(e).message }) });
    } finally {
      busy = null;
    }
  }
</script>

<Sheet title={t('mob.att.title')} {onclose}>
  <ul class="atts">
    {#each d.attachments as a (a.index)}
      <li class="item">
        <button class="open" aria-label={t('mob.att.open', { name: a.name })} aria-busy={busy === `open-${a.index}`} onclick={() => void run('open', a)}>
          <FileBadge name={a.name} mime={a.mime} />
          <span class="text">
            <span class="trunc name">{a.name}</span>
            <span class="meta num">{fileSize(a.size, locale())} · {t('read.preview')}</span>
          </span>
        </button>
        <button
          class="btn-icon"
          aria-label={t('mob.att.save', { name: a.name })}
          title={t('mob.att.save', { name: a.name })}
          aria-busy={busy === `save-${a.index}`}
          onclick={() => void run('save', a)}
        >
          <Icon name="download" size={22} />
        </button>
        <button
          class="btn-icon"
          aria-label={t('mob.att.share', { name: a.name })}
          title={t('mob.att.share', { name: a.name })}
          aria-busy={busy === `share-${a.index}`}
          onclick={() => void run('share', a)}
        >
          <Icon name="share" size={22} />
        </button>
      </li>
    {/each}
  </ul>
</Sheet>

<style>
  .atts {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 56px;
    padding-right: 4px;
  }

  /* Satırın kendisi eki açar; ad kısalır, düğmeler hep ekranda kalır. */
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 56px;
    padding: 6px 8px 6px 12px;
    border: 0;
    border-radius: var(--r-row);
    background: transparent;
    color: var(--ink);
    text-align: left;
    transition: background var(--t-fast);
  }

  .open:active {
    background: var(--hover);
  }

  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.35;
  }

  .name {
    font-weight: 500;
  }
</style>
