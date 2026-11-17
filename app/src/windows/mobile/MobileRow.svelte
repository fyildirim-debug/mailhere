<script lang="ts">
  // Mobil liste satırı (72 px, sıkı 60; sistem yazı ölçeğiyle büyür):
  // gönderen + saat, konu, önizleme. Sol oluk: 8 px okunmamış noktası, 40 px
  // avatar; metin 15 px tek ölçekte, hiyerarşi ağırlık ve mürekkeple kurulur
  // (okunmamış: nokta + 600 + --ink). Birleşik görünümde saatin yanında 8 px
  // köşesi 2 hesap rengi karesi (renkli kenar çizgisi değil; okunmamış
  // noktası yuvarlak kalır).
  //
  // Avatar ayrı düğmedir (44 px hedef): dokunmak seçimi başlatır. Seçim
  // kipinde satırın tamamı işaretler, avatarın yerine işaret gelir. Hareket
  // mantığı (kaydırma, uzun basış) SwipeRow'da.
  //
  // Şüpheli posta (`spam`) ilk satırda `--amber` uyarı ikonu taşır. Taze kod
  // postasında (`otp`, son 60 dakika) konu satırının sonunda kod çipi durur:
  // çip satır düğmesinin içinde değil üstündedir (düğme içinde düğme olmaz),
  // konu satırı ona yer bırakır. Çipe dokunmak kodu kopyalar; postayı açmaz,
  // kaydırmayı ve uzun basışı da başlatmaz (işaretçi olayları satıra çıkmaz).
  import type { Account } from '../../lib/api';
  import { locale, t } from '../../lib/i18n/index.svelte';
  import OtpChip from '../../lib/mail/OtpChip.svelte';
  import { otpFresh } from '../../lib/stores/guard.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { followUpDue } from '../../lib/util/followup';
  import { displayName, listTime } from '../../lib/util/format';
  import type { Row } from '../../lib/stores/mail.svelte';
  import { tagsOf } from '../../lib/stores/tags.svelte';

  interface Props {
    message: Row;
    now: number;
    compact: boolean;
    checked: boolean;
    /** Seçim kipi: dokunma işaretler, açmaz. */
    selecting: boolean;
    /** Birleşik görünümde satırın hesabı (renk karesi); tek hesapta yok. */
    account?: Account;
    onopen: () => void;
    ontoggle: () => void;
  }

  let { message, now, compact, checked, selecting, account, onopen, ontoggle }: Props = $props();

  const tags = $derived(tagsOf(message.tags));
  // Yanıt hatırlatmalı (gönderilmiş) satırda beklenen kişi, yani ilk alıcı görünür.
  const who = $derived(message.followUp != null && message.to[0] ? message.to[0] : message.from);
  const awaiting = $derived(followUpDue(message.followUp, now));
  const selectLabel = $derived(t('list.selectRow', { subject: message.subject || displayName(message.from) }));
  /** Kod çipi: yalnız taze postada ve seçim kipi dışında (seçimde dokunuş işaretler). */
  const code = $derived(!selecting && message.otp && otpFresh(message.date, now) ? message.otp : null);
  /** Çipin genişliği: konu satırı bu kadar yer bırakır (yerleşim oynamaz, metin çipin altına girmez). */
  let chipW = $state(0);
  /** İlk satırın ve konu satırının ölçülen yükseklikleri (sistem yazı ölçeğiyle büyür). */
  let line1H = $state(20);
  let subjectH = $state(20);
  /** Çip (24 px) konu satırının ortasında: satır iç boşluğu 6, sıkı görünümde metin 4 px aşağıda. */
  const chipTop = $derived(6 + (compact ? 4 : 0) + line1H + (subjectH - 24) / 2);

  /** Çipe basış satıra çıkmaz: kaydırma, uzun basış ve aşağı çekme başlamaz. */
  function quiet(node: HTMLElement) {
    const stop = (e: Event) => e.stopPropagation();
    const types = ['pointerdown', 'touchstart', 'contextmenu'] as const;
    for (const type of types) node.addEventListener(type, stop, { passive: true });
    return {
      destroy() {
        for (const type of types) node.removeEventListener(type, stop);
      },
    };
  }
</script>

<div class="wrap" class:compact>
  <button
    class="row"
    class:unread={message.unread}
    class:checked
    class:compact
    data-id={message.id}
    aria-pressed={selecting ? checked : undefined}
    aria-label={selecting ? selectLabel : undefined}
    onclick={() => (selecting ? ontoggle() : onopen())}
  >
    <span class="dot" aria-hidden="true"></span>
    <span class="av" aria-hidden="true">
      {#if checked}
        <span class="tick"><Icon name="check" size={20} /></span>
      {:else}
        <Avatar address={who} size={40} />
      {/if}
    </span>
    <span class="text">
      <span class="line1" bind:clientHeight={line1H}>
        <span class="sender trunc">{displayName(who)}</span>
        {#if awaiting}<span class="tag">{t('follow.awaiting')}</span>{/if}
        {#if message.count > 1}<span class="count num">{message.count}</span>{/if}
        {#if message.spam}<span class="warn"><Icon name="alert" size={14} label={t('mob.spam.rowLabel')} /></span>{/if}
        {#if message.hasAttachments}<Icon name="paperclip" size={14} label={t('list.attachment')} class="quiet" />{/if}
        {#if message.starred}<span class="star"><Icon name="star" size={14} filled label={t('list.starred')} /></span>{/if}
        {#if message.pinned}<Icon name="pin" size={14} label={t('list.pinnedMark')} class="quiet" />{/if}
        {#if tags.length}
          <span class="tags" role="img" aria-label={t('list.tagsOf', { list: tags.map((x) => x.name).join(', ') })}>
            {#each tags.slice(0, 3) as tg (tg.id)}<span class="tdot" style:background={tg.color}></span>{/each}
          </span>
        {/if}
        <span class="time num">
          {#if account}<span class="acc" style:background={account.color} role="img" aria-label={t('mob.rowAccount', { name: account.name })}></span>{/if}
          {listTime(message.date, now, locale())}
        </span>
      </span>
      <span class="subject trunc" bind:clientHeight={subjectH} style:margin-right={code ? `${chipW + 8}px` : undefined}>{message.subject || '—'}</span>
      {#if !compact}<span class="snippet trunc">{message.snippet}</span>{/if}
    </span>
  </button>
  <!-- Avatara dokunmak seçimi başlatır (seçim kipinde satırın kendisi işaretler). -->
  {#if !selecting}
    <button class="pick" aria-label={selectLabel} onclick={ontoggle}></button>
  {/if}
  {#if code}
    <span class="code" style:top="{chipTop}px" bind:clientWidth={chipW} use:quiet><OtpChip id={message.id} {code} unread={message.unread} /></span>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    height: 100%;
  }

  .row {
    display: grid;
    grid-template-columns: 10px 40px minmax(0, 1fr);
    column-gap: 10px;
    align-items: start;
    width: 100%;
    height: 100%;
    padding: 6px var(--m-gutter) 6px 8px;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--ink);
    line-height: var(--m-line);
    text-align: left;
    transition: background var(--t-fast);
  }

  .row:active {
    background: var(--hover);
  }

  .row.checked {
    background: var(--sel);
  }

  .row:focus-visible,
  .pick:focus-visible {
    outline-offset: -2px;
  }

  /* Avatarın üstünde görünmez 44 px hedef (oluk + avatar sütunu). */
  .pick {
    position: absolute;
    top: 10px;
    left: 22px;
    width: 52px;
    height: 52px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
  }

  .compact .pick {
    top: 4px;
  }

  .pick:active {
    background: var(--hover);
  }

  /* Sürüklenen satır basma tonu almaz: zemin nötr kalır, eşik rengi yalnız
     alttaki panelde (SwipeRow sürüklerken sarmalayıcıya `dragging` verir). */
  :global(.swipe.dragging) .row:active {
    background: var(--surface);
  }

  :global(.swipe.dragging) .pick:active {
    background: transparent;
  }

  /* Okunmamış noktası ilk satırın ortasında; kanalı hep yerinde durur. */
  .dot {
    width: 8px;
    height: 8px;
    margin: 6px 0 0 1px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0;
    transition: opacity var(--t-fast);
  }

  .compact .dot {
    margin-top: 10px;
  }

  .unread .dot {
    opacity: 1;
  }

  .av {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    margin-top: 10px;
  }

  .compact .av {
    margin-top: 4px;
  }

  .tick {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-ink);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .compact .text {
    margin-top: 4px;
  }

  .line1 {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .sender {
    font-weight: 500;
    color: var(--ink);
  }

  .unread .sender {
    font-weight: 600;
  }

  .count {
    flex: none;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .line1 :global(.quiet) {
    color: var(--ink-3);
  }

  .star,
  .warn {
    flex: none;
    display: inline-flex;
    color: var(--amber);
  }

  /* Kod çipi: konu satırının sağ ucunda, satır düğmesinin üstünde. Görünüşü
     24 px kalır; dokunma alanı görünmez kenarla 44 px'e çıkar. */
  .code {
    position: absolute;
    right: var(--m-gutter);
    display: inline-flex;
  }

  .code :global(.otp-chip) {
    position: relative;
  }

  /* Kenar çizgisi (1 px) hesaba katılır: 24 + 2 × 10 = 44. */
  .code :global(.otp-chip)::before {
    content: '';
    position: absolute;
    inset: -11px -5px;
  }

  .tags {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .tdot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .time {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
    padding-left: 4px;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  /* Hesap rengi: 8 px köşesi 2 kare, saatin solunda; yuvarlak okunmamış
     noktasıyla (vurgu rengine yakın hesap renklerinde de) karışmaz. */
  .acc {
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }

  .unread .time {
    color: var(--ink-2);
    font-weight: 500;
  }

  .subject {
    color: var(--ink-2);
  }

  .unread .subject {
    color: var(--ink);
    font-weight: 500;
  }

  .snippet {
    color: var(--ink-3);
  }
</style>
