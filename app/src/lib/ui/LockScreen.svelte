<script lang="ts">
  // Kilit ekranı: tek parola alanı, "Kilidi aç" (ekranın tek birincil
  // düğmesi), "Parolamı unuttum". Çekirdek kapalıyken (açılış) ve yalnız
  // arayüz kilitliyken (pencere yeniden açıldı / uygulamaya dönüldü) aynı
  // ekran; posta verisi DOM'a girmez. Caps Lock uyarısı, yanlış paroladan
  // sonra artan bekleme sayacı; her şey arka uçtan gelen durumla sürülür.
  import { onMount } from 'svelte';
  import { call, toAppError } from '../api';
  import { t } from '../i18n/index.svelte';
  import { isMobileShell } from '../shell';
  import { lock, unlock } from '../stores/lock.svelte';
  import Icon from './Icon.svelte';
  import Logo from './Logo.svelte';

  interface Props {
    /** İkincil pencere (hızlı okuma, yazma): daha sıkı yerleşim, unutma yolu yok. */
    compact?: boolean;
    onunlocked?: () => void;
  }

  let { compact = false, onunlocked }: Props = $props();

  let password = $state('');
  let show = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let caps = $state(false);
  let forgot = $state(false);
  let resetting = $state(false);
  let restarting = $state(false);
  let input = $state<HTMLInputElement>();
  let now = $state(Date.now());
  /** Beklemenin bittiği an (ms); 0 ise bekleme yok. */
  let until = $state(0);

  const status = $derived(lock.status);
  const coreLocked = $derived(status?.locked ?? true);
  const waitSecs = $derived(Math.max(0, Math.ceil((until - now) / 1000)));
  const waiting = $derived(waitSecs > 0);
  const lead = $derived(coreLocked ? t('lock.leadCore') : isMobileShell ? t('mob.lock.leadUi') : t('lock.leadUi'));

  // Arka uç bekleme süresi verdiyse sayaç ondan kurulur (başka pencereden
  // yapılan denemeler de buraya yansır).
  $effect(() => {
    const ms = status?.retryAfterMs ?? 0;
    if (ms > 0) until = Math.max(until, Date.now() + ms);
  });

  onMount(() => {
    input?.focus();
    const tick = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(tick);
  });

  function onKey(e: KeyboardEvent) {
    caps = typeof e.getModifierState === 'function' && e.getModifierState('CapsLock');
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !password || waiting) return;
    busy = true;
    error = null;
    try {
      await unlock(password);
      password = '';
      onunlocked?.();
    } catch (err) {
      const a = toAppError(err);
      error = a.message;
      // Parola alanı temizlenir; odak yeniden alanda (tekrar denemek için).
      password = '';
      input?.focus();
    } finally {
      busy = false;
    }
  }

  async function reset() {
    if (resetting) return;
    resetting = true;
    error = null;
    try {
      const s = await call('lock_reset_local');
      if (s.enabled) {
        // Çekirdek açıkken sıfırlama yeniden başlatmayla biter.
        restarting = true;
        return;
      }
      // Yerel veri silindi, boş çekirdek açıldı: ilk açılış ekranı.
      location.reload();
    } catch (err) {
      error = toAppError(err).message;
      resetting = false;
    }
  }
</script>

<div class="lock" class:compact class:mobile={isMobileShell} data-testid="lock-screen">
  {#if forgot}
    <section class="panel" aria-labelledby="lock-forgot-title">
      <span class="mark warn"><Icon name="alert" size={26} /></span>
      <h1 id="lock-forgot-title">{t('lock.forgotTitle')}</h1>
      <p class="lead">{t('lock.forgotLead')}</p>
      <ul class="facts">
        <li><Icon name="trash" size={18} /><span>{t('lock.forgotLoses')}</span></li>
        <li><Icon name="server" size={18} /><span>{t('lock.forgotKeeps')}</span></li>
        <li><Icon name="key" size={18} /><span>{t('lock.forgotAccounts')}</span></li>
      </ul>
      {#if restarting}
        <p class="hint" role="status">{t('lock.forgotRestart')}</p>
      {:else}
        {#if error}
          <p class="err" role="alert"><Icon name="alert" size={16} />{error}</p>
        {/if}
        <div class="actions">
          <button type="button" class="btn-soft" disabled={resetting} onclick={() => (forgot = false)}>{t('act.cancel')}</button>
          <button type="button" class="btn-soft danger" disabled={resetting} onclick={reset}>
            {resetting ? t('lock.forgotBusy') : t('lock.forgotConfirm')}
          </button>
        </div>
      {/if}
    </section>
  {:else}
    <form class="panel" aria-labelledby="lock-title" onsubmit={submit}>
      <span class="mark"><Logo size={40} /></span>
      <h1 id="lock-title">{t('lock.title')}</h1>
      <p class="lead">{lead}</p>
      <label class="label" for="lock-password">{t('lock.password')}</label>
      <div class="field">
        <input
          id="lock-password"
          class="input"
          type={show ? 'text' : 'password'}
          autocomplete="current-password"
          spellcheck="false"
          autocapitalize="off"
          bind:this={input}
          bind:value={password}
          disabled={busy}
          aria-describedby={caps ? 'lock-caps' : undefined}
          aria-invalid={error ? 'true' : undefined}
          onkeydown={onKey}
          onkeyup={onKey}
        />
        <button
          type="button"
          class="btn-icon eye"
          aria-label={show ? t('lock.hide') : t('lock.show')}
          title={show ? t('lock.hide') : t('lock.show')}
          aria-pressed={show}
          onclick={() => (show = !show)}
        >
          <Icon name={show ? 'eyeOff' : 'eye'} size={18} />
        </button>
      </div>
      {#if caps}
        <p class="hint caps" id="lock-caps" role="status"><Icon name="alert" size={16} />{t('lock.capsLock')}</p>
      {/if}
      {#if error}
        <p class="err" role="alert"><Icon name="alert" size={16} />{error}</p>
      {/if}
      {#if waiting}
        <p class="hint" role="status">{t('lock.wait', { n: waitSecs })}</p>
      {/if}
      <button class="btn-primary" type="submit" disabled={busy || !password || waiting}>
        {busy ? t('lock.unlocking') : t('lock.unlock')}
      </button>
      {#if !compact}
        <button type="button" class="btn-ghost forgot" onclick={() => (forgot = true)}>{t('lock.forgot')}</button>
      {/if}
    </form>
  {/if}
</div>

<style>
  .lock {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 32px 24px;
    background: var(--paper);
    overflow-y: auto;
  }

  .mobile {
    padding: calc(var(--sat, 0px) + 32px) calc(var(--sar, 0px) + 24px) calc(var(--sab, 0px) + 32px) calc(var(--sal, 0px) + 24px);
  }

  .panel {
    width: 100%;
    max-width: 360px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    animation: lock-in var(--t-panel) var(--ease);
  }

  .compact .panel {
    max-width: 320px;
    gap: 10px;
  }

  .mark {
    display: inline-flex;
    margin-bottom: 8px;
  }

  .warn {
    width: 56px;
    height: 56px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  h1 {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .compact h1 {
    font-size: var(--fs-section);
  }

  .lead {
    margin: -4px 0 8px;
    color: var(--ink-2);
    text-wrap: pretty;
  }

  .label {
    margin-bottom: -6px;
  }

  .field {
    position: relative;
  }

  .field .input {
    padding-right: 48px;
  }

  .mobile .field .input {
    height: 48px;
  }

  .eye {
    position: absolute;
    top: 50%;
    right: 5px;
    transform: translateY(-50%);
  }

  .mobile .eye {
    width: 44px;
    height: 44px;
    right: 2px;
  }

  .hint,
  .err {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .hint :global(.icon),
  .err :global(.icon) {
    flex: none;
    margin-top: 1px;
  }

  .caps {
    color: var(--amber-text);
  }

  .err {
    color: var(--danger);
  }

  .btn-primary {
    margin-top: 4px;
    width: 100%;
  }

  .mobile .btn-primary {
    height: 48px;
  }

  .forgot {
    align-self: center;
    margin-top: 4px;
    color: var(--ink-2);
  }

  .mobile .forgot {
    min-height: 44px;
  }

  .facts {
    margin: 4px 0 8px;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
    color: var(--ink-2);
  }

  .facts li {
    display: flex;
    gap: 10px;
  }

  .facts :global(.icon) {
    flex: none;
    margin-top: 2px;
    color: var(--ink-3);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }

  .mobile .actions .btn-soft {
    min-height: 44px;
  }

  .danger {
    color: var(--danger);
  }

  @keyframes lock-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .panel {
      animation: none;
    }
  }
</style>
