<script lang="ts">
  // Mobil yazma ekranı: uygulama çubuğu (kapat ×, "Yeni posta" / "Yanıt" /
  // "İlet" / "Taslak", birincil Gönder) + gömülü ComposeWindow. Gönder çubukta
  // durduğu için klavye açıkken de görünür.
  //
  // Kapanış (× ya da geri tuşu): değişiklik varsa ComposeWindow sökülürken
  // taslağa yazar (masaüstünde pencere kapanırken yapılanla aynı) ve
  // "Taslaklara kaydedildi" der. Gönderim giden kutusuna girince ekran hemen
  // kapanır; toast'taki "Geri al" gönderimi durdurur, içeriği taslak olarak
  // kaydeder ve düzenlemeye yeniden açar (masaüstü pencerede kalıp geri sayar).
  import { call, toAppError, type Address, type ComposeMode, type MessageId, type OutgoingMessage } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import ComposeWindow from '../compose/ComposeWindow.svelte';
  import AppBar from './AppBar.svelte';
  import { nav } from './nav.svelte';

  interface Props {
    mode: ComposeMode;
    id: MessageId | null;
    /** Yeni postada önceden doldurulan alıcılar (kişi kartı, `mh:navigate`). */
    to: Address[];
    from: string | null;
  }

  let { mode, id, to, from }: Props = $props();

  let win = $state<ComposeWindow>();

  const title = $derived(
    mode === 'new' ? t('compose.new') : mode === 'draft' ? t('mob.compose.draft') : mode === 'forward' ? t('mob.compose.forward') : t('mob.compose.reply'),
  );

  function close() {
    nav.back();
  }

  /** Gönderim kuyruğa girdi: ekran kapanır, geri alma toast'ı listede görünür. */
  function sent(info: { id: string; sendAt: number; msg: OutgoingMessage }) {
    const wait = Math.max(0, info.sendAt - Date.now());
    nav.back();
    showToast({
      text: t('toast.queued', { n: Math.max(1, Math.round(wait / 1000)) }),
      duration: Math.max(4000, wait),
      action: { label: t('act.undo'), run: () => restore(info) },
    });
  }

  /** Gönderimi durdurur; içerik yeni taslak olur ve düzenlemeye açılır. */
  async function restore(info: { id: string; msg: OutgoingMessage }) {
    try {
      if (!(await call('send_cancel', { id: info.id }))) return;
      // Gönderimde eski taslak silinmişti; ekleri o taslaktan geliyorsa artık okunamaz.
      const lost = info.msg.forwardFrom != null && info.msg.forwardFrom === info.msg.draftId;
      const draftId = await call('draft_save', { msg: { ...info.msg, sendAt: null, draftId: null, forwardFrom: lost ? null : info.msg.forwardFrom } });
      nav.push({ name: 'compose', mode: 'draft', id: draftId, to: [], from: null });
      showToast({ text: t('mob.compose.restored') });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }
</script>

<AppBar leading="close" leadingLabel={t('mob.compose.close')} {title} onleading={close}>
  {#snippet actions()}
    <button class="btn-primary send" onclick={() => void win?.send()}>
      <Icon name="send" size={18} />{t('act.send')}
    </button>
  {/snippet}
</AppBar>

<div class="body">
  <ComposeWindow bind:this={win} embedded {mode} messageId={id} initialTo={to} initialFrom={from} onclose={close} onsent={sent} />
</div>

<style>
  .send {
    height: 40px;
    margin-right: 6px;
    padding: 0 16px 0 14px;
    gap: 8px;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--paper);
  }
</style>
