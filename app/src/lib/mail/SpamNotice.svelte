<script lang="ts">
  // Şüpheli posta uyarısı (okuma alanı; masaüstü ve mobil ortak): başlık, her
  // neden için kısa bir cümle ve üç eylem. "İstenmeyen'e taşı" posta zaten o
  // kutudaysa ya da hesapta İstenmeyen kutusu yoksa görünmez; "Göndereni
  // engelle" gönderen zaten engelliyse görünmez. Eylemler ortak yardımcıdan
  // geçer (`guard.svelte.ts`): bildirim ve geri alma iki kabukta aynıdır.
  // `readonly` yalnız nedenleri gösterir (hızlı okuma penceresi: orada liste,
  // bildirim ve geri alma yok; eylemler için posta uygulamada açılır).
  import type { MessageDetail, SpamVerdict } from '../api';
  import { t } from '../i18n/index.svelte';
  import { blockSenderOf, trustSenderOf } from '../stores/guard.svelte';
  import { mail, moveToFolder } from '../stores/mail.svelte';
  import Icon from '../ui/Icon.svelte';
  import { folderLabel } from './folders';

  interface Props {
    detail: MessageDetail;
    verdict: SpamVerdict;
    /** "Güvenli" dendi: kapsayıcı kendi kopyasından uyarıyı kaldırır (konuşma görünümü). */
    onupdate?: (d: MessageDetail) => void;
    /** Yalnız başlık ve nedenler; eylem düğmeleri yok. */
    readonly?: boolean;
  }

  let { detail: d, verdict, onupdate, readonly = false }: Props = $props();

  const uid = $props.id();
  const box = $derived(mail.mailboxes.find((b) => b.id === d.mailboxId));
  /** Hesabın İstenmeyen kutusu; posta zaten oradaysa taşıma sunulmaz. */
  const junk = $derived(box?.role === 'junk' ? undefined : mail.mailboxes.find((b) => b.accountId === d.accountId && b.role === 'junk'));
  const blocked = $derived(verdict.reasons.includes('blocked'));
  let busy = $state(false);

  async function run(action: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    try {
      await action();
    } finally {
      busy = false;
    }
  }

  function toJunk() {
    if (junk) void run(() => moveToFolder([d.id], junk, folderLabel(junk)));
  }

  function trust() {
    void run(async () => {
      if (await trustSenderOf(d.id)) onupdate?.({ ...d, spam: false, spamVerdict: null });
    });
  }
</script>

<section class="spam" aria-labelledby="{uid}-title">
  <span class="mark"><Icon name="alert" size={18} /></span>
  <div class="text">
    <h3 id="{uid}-title">{t('spam.title')}</h3>
    <ul>
      {#each verdict.reasons as r (r)}
        <li>{t(`spam.reason.${r}`)}</li>
      {/each}
    </ul>
  </div>
  {#if !readonly}
  <div class="acts">
    {#if junk}
      <button class="btn-soft small" disabled={busy} onclick={toJunk}><Icon name="spam" size={16} />{t('spam.moveJunk')}</button>
    {/if}
    {#if !blocked}
      <button class="btn-soft small" disabled={busy} title={d.from.email} onclick={() => void run(() => blockSenderOf(d.id))}>
        <Icon name="block" size={16} />{t('spam.block')}
      </button>
    {/if}
    <button class="btn-soft small" disabled={busy} title={t('spam.safeHint')} onclick={trust}><Icon name="check" size={16} />{t('spam.safe')}</button>
  </div>
  {/if}
</section>

<style>
  /* Tek kutu: --surface zemin, --line kenar; renkli kenar çizgisi ve iç kart yok.
     Amber yalnız ikonda (--amber) ve başlıkta (--amber-text). */
  .spam {
    margin-top: 12px;
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr);
    column-gap: 10px;
    row-gap: 12px;
    padding: 12px 14px 14px 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-row);
    background: var(--surface);
  }

  .mark {
    display: flex;
    height: 20px;
    align-items: center;
    color: var(--amber);
  }

  .text {
    min-width: 0;
  }

  h3 {
    font-size: var(--fs-body);
    font-weight: 600;
    line-height: 20px;
    color: var(--amber-text);
  }

  ul {
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    color: var(--ink-2);
    line-height: 1.45;
    overflow-wrap: anywhere;
  }

  .acts {
    grid-column: 2;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    min-width: 0;
  }

  /* Telefonda dokunma hedefi 44 px (mobil ölçü jetonu yalnız orada tanımlı). */
  .acts button {
    min-height: var(--m-touch, 32px);
  }
</style>
