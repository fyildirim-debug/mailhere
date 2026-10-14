<script lang="ts">
  // Cihazlar: masaüstünde "Telefona aktar" (QR teklifi, yalnız yerel ağ), mobilde
  // "Bilgisayardan al" (TransferReceive). Teklif yalnız bu bölüm açıkken yaşar;
  // bölümden çıkınca iptal edilir. Kod metni varsayılan gizlidir ve günlüğe,
  // toast'a ya da başka bir yere yazılmaz.
  import { onMount } from 'svelte';
  import { call, EVENTS, on, openExternal, toAppError, type TransferOffer, type TransferState, type TransferStateKind } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import type { IconName } from '../../../lib/ui/icons';
  import QrCode from '../../../lib/ui/QrCode.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { isMobileShell } from '../../../lib/shell';

  const RELEASES_URL = 'https://github.com/fyildirim-debug/mailheremobile/releases';
  // Mobil kabukta alma ekranı tembel yüklenir: masaüstü paketi mobil gezinmeyi taşımaz.
  const receive = isMobileShell ? import('../../mobile/TransferReceive.svelte') : null;

  let includeSecrets = $state(true);
  let offer = $state<TransferOffer | null>(null);
  let status = $state<TransferState | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let showText = $state(false);
  let now = $state(Date.now());

  const FINAL: TransferStateKind[] = ['applied', 'failed', 'expired', 'cancelled'];
  const kind = $derived(status?.state ?? null);
  /** Teklif açık ve kod hâlâ okutulabilir. */
  const active = $derived(!!offer && (kind === 'waiting' || kind === 'connected'));
  const finished = $derived(!!offer && kind != null && FINAL.includes(kind));
  const remaining = $derived(offer ? Math.max(0, Math.ceil((offer.expiresAt - now) / 1000)) : 0);
  const clock = $derived(`${String(Math.floor(remaining / 60)).padStart(2, '0')}:${String(remaining % 60).padStart(2, '0')}`);

  const stateIcon: Record<TransferStateKind, IconName> = {
    waiting: 'qr',
    connected: 'devices',
    delivered: 'devices',
    applied: 'check',
    failed: 'x',
    expired: 'clock',
    cancelled: 'x',
  };
  const tone = $derived(kind === 'applied' ? 'ok' : kind === 'failed' ? 'danger' : kind === 'expired' || kind === 'cancelled' ? 'muted' : 'busy');
  const statusText = $derived.by(() => {
    if (!kind) return '';
    const base = t(`aktar.state.${kind}` as MessageKey);
    return kind === 'failed' && status?.message ? `${base}: ${status.message}` : base;
  });

  onMount(() => {
    const off = on(EVENTS.transfer, (s) => (status = s));
    const tick = setInterval(() => (now = Date.now()), 1000);
    return () => {
      off();
      clearInterval(tick);
      // Bölümden çıkınca açık teklif kapanır: kod artık kimseye gösterilmiyor.
      // Sökülürken türetilmiş değer bayat kalabilir; durum doğrudan okunur.
      const k = status?.state;
      if (offer && !(k && FINAL.includes(k))) void call('transfer_cancel');
    };
  });

  async function begin() {
    if (busy) return;
    busy = true;
    error = null;
    showText = false;
    try {
      const o = await call('transfer_begin', { includeSecrets });
      now = Date.now();
      offer = o;
      status = { state: 'waiting', message: null };
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    if (!offer) return;
    try {
      await call('transfer_cancel');
    } catch {
      // Teklif zaten kapanmış olabilir; durum olayla gelir.
    }
    status = { state: 'cancelled', message: null };
  }

  async function copy() {
    if (!offer) return;
    try {
      await navigator.clipboard.writeText(offer.text);
      showToast({ text: t('aktar.copied') });
    } catch {
      showText = true;
      showToast({ text: t('aktar.copyFailed') });
    }
  }
</script>

<h2 class="page">{t('aktar.nav')}</h2>

{#if receive}
  {#await receive then { default: TransferReceive }}
    <TransferReceive />
  {/await}
{:else}
  <h3 class="set-h">{t('aktar.sendTitle')}</h3>
  <p class="set-lead">{t('aktar.sendLead')}</p>
  <ul class="facts">
    <li><Icon name="check" size={18} class="yes" /><span>{t('aktar.what')}</span></li>
    <li><Icon name="block" size={18} class="no" /><span>{t('aktar.whatNot')}</span></li>
  </ul>

  <div class="set-row top">
    <span class="set-name">{t('aktar.includeSecrets')}<span class="set-hint">{t('aktar.includeSecretsHint')}</span></span>
    <input
      type="checkbox"
      role="switch"
      class="switch"
      aria-label={t('aktar.includeSecrets')}
      checked={includeSecrets}
      disabled={active}
      onchange={(e) => (includeSecrets = (e.currentTarget as HTMLInputElement).checked)}
    />
  </div>

  {#if !offer}
    {#if error}<p class="set-error" role="alert">{error}</p>{/if}
    <div class="start">
      <button class="btn-primary" disabled={busy} onclick={begin}><Icon name="qr" size={16} />{busy ? t('aktar.creating') : t('aktar.create')}</button>
    </div>
  {:else}
    <div class="stage">
      <div class="qr-card" class:idle={!active}>
        {#if active}
          <QrCode qr={offer.qr} label={t('aktar.qrLabel')} />
        {:else if kind}
          <span class="glyph {tone}"><Icon name={stateIcon[kind]} size={32} /></span>
        {/if}
      </div>
      <div class="side">
        <p class="status {tone}" role="status" aria-live="polite"><span class="pip"></span><span>{statusText}</span></p>
        {#if active}
          <p class="count">{t('aktar.remaining')} <span class="num">{clock}</span></p>
        {:else if kind === 'expired'}
          <p class="set-lead">{t('aktar.expiredHint')}</p>
        {/if}
        <div class="tools">
          {#if active}
            <button class="btn-soft" onclick={() => (showText = !showText)}>
              <Icon name={showText ? 'eyeOff' : 'eye'} size={16} />{showText ? t('aktar.hideText') : t('aktar.showText')}
            </button>
            <button class="btn-soft" onclick={copy}><Icon name="copy" size={16} />{t('aktar.copy')}</button>
            <button class="btn-ghost" onclick={cancel}>{t('act.cancel')}</button>
          {:else if finished}
            <button class="btn-primary" disabled={busy} onclick={begin}><Icon name="qr" size={16} />{busy ? t('aktar.creating') : t('aktar.createAgain')}</button>
          {:else}
            <button class="btn-ghost" onclick={cancel}>{t('act.cancel')}</button>
          {/if}
        </div>
        {#if error}<p class="set-error" role="alert">{error}</p>{/if}
        {#if showText && active}
          <div class="text">
            <code class="selectable">{offer.text}</code>
            <p class="meta">{t('aktar.textHint')}</p>
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <ul class="hints">
    <li><Icon name="globe" size={16} /><span>{t('aktar.hint.wifi')}</span></li>
    <li><Icon name="shield" size={16} /><span>{t('aktar.hint.firewall')}</span></li>
    <li><Icon name="eyeOff" size={16} /><span>{t('aktar.hint.screen')}</span></li>
  </ul>

  <p class="noapp">
    {t('aktar.noApp')}
    <button class="link" onclick={() => openExternal(RELEASES_URL)}>{t('aktar.download')}<Icon name="external" size={14} /></button>
  </p>
{/if}

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 14px 0 6px;
    padding: 0;
    list-style: none;
    color: var(--ink-2);
  }

  .facts li {
    display: flex;
    gap: 12px;
    max-width: 64ch;
  }

  .facts :global(.icon) {
    margin-top: 2px;
  }

  .facts :global(.yes) {
    color: var(--ok);
  }

  .facts :global(.no) {
    color: var(--ink-3);
  }

  .start {
    margin-top: 18px;
  }

  .stage {
    display: flex;
    align-items: flex-start;
    gap: 28px;
    margin-top: 20px;
  }

  /* QR kartı: her temada beyaz zemin (--qr-paper); sessiz bölge SVG'nin içindedir. */
  .qr-card {
    flex: none;
    display: grid;
    place-items: center;
    width: 264px;
    height: 264px;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    background: var(--qr-paper);
  }

  .qr-card.idle {
    background: var(--surface);
  }

  .glyph {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    border-radius: var(--r-card);
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .glyph.ok {
    background: color-mix(in srgb, var(--ok) 14%, transparent);
    color: var(--ok);
  }

  .glyph.danger {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
  }

  .glyph.muted {
    background: var(--ground);
    color: var(--ink-3);
  }

  .side {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-top: 4px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 500;
  }

  .pip {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--accent);
  }

  .busy .pip {
    animation: pulse 1.6s ease-in-out infinite;
  }

  .ok .pip {
    background: var(--ok);
  }

  .danger .pip {
    background: var(--danger);
  }

  .muted .pip {
    background: var(--ink-3);
  }

  .count {
    color: var(--ink-2);
  }

  .count .num {
    font-weight: 600;
    color: var(--ink);
  }

  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--r-row);
    background: var(--surface);
  }

  .text code {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    line-height: 1.5;
    word-break: break-all;
    color: var(--ink);
  }

  .hints {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 28px 0 0;
    padding: 0;
    list-style: none;
    color: var(--ink-2);
  }

  .hints li {
    display: flex;
    gap: 10px;
    max-width: 64ch;
  }

  .hints :global(.icon) {
    margin-top: 3px;
    color: var(--ink-3);
  }

  .noapp {
    margin-top: 28px;
    color: var(--ink-2);
  }

  .noapp .link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: 4px;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @media (max-width: 1100px) {
    .stage {
      flex-direction: column;
    }
  }
</style>
