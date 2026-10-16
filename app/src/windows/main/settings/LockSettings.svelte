<script lang="ts">
  // Ayarlar › Gizlilik ve güvenlik › Parola kilidi: kur / kapat / parolayı
  // değiştir ve "her açışta parola sor" tercihi. Parola iki kez girilir
  // (en az 8 karakter), güç ipucu metinle, unutma uyarısı açık ve onay
  // kutulu. Tüm kripto Rust'ta; burada yalnız form ve komut.
  import { call, toAppError, LOCK_PASSWORD_MIN } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { isMobileShell } from '../../../lib/shell';
  import { lock } from '../../../lib/stores/lock.svelte';

  type Mode = 'enable' | 'disable' | 'change';

  const status = $derived(lock.status);
  const enabled = $derived(status?.enabled ?? false);
  const mobile = isMobileShell;

  let mode = $state<Mode | null>(null);
  let current = $state('');
  let next = $state('');
  let repeat = $state('');
  let ack = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let relockBusy = $state(false);
  let sheet = $state<HTMLDivElement>();

  const needsNew = $derived(mode === 'enable' || mode === 'change');
  const needsCurrent = $derived(mode === 'disable' || mode === 'change');
  const tooShort = $derived(needsNew && next.length > 0 && [...next].length < LOCK_PASSWORD_MIN);
  const mismatch = $derived(needsNew && repeat.length > 0 && next !== repeat);
  const canSubmit = $derived(
    !busy &&
      (!needsCurrent || current.length > 0) &&
      (!needsNew || ([...next].length >= LOCK_PASSWORD_MIN && next === repeat)) &&
      (mode !== 'enable' || ack),
  );

  /** Kaba güç ipucu: uzunluk ve çeşitlilik; yalnız yol gösterir, kural değildir. */
  function strength(p: string): MessageKey | null {
    const n = [...p].length;
    if (n === 0) return null;
    let score = 0;
    if (n >= 12) score++;
    if (n >= 16) score++;
    if (/[a-zçğıöşü]/.test(p) && /[A-ZÇĞİÖŞÜ]/.test(p)) score++;
    if (/\d/.test(p)) score++;
    if (/[^\p{L}\p{N}]/u.test(p)) score++;
    if (n < LOCK_PASSWORD_MIN || score < 2) return 'lock.dlg.weak';
    return score >= 4 ? 'lock.dlg.strong' : 'lock.dlg.fair';
  }

  const level = $derived(strength(next));

  function open(m: Mode) {
    mode = m;
    current = '';
    next = '';
    repeat = '';
    ack = false;
    error = null;
  }

  function close() {
    if (busy) return;
    mode = null;
    current = '';
    next = '';
    repeat = '';
  }

  $effect(() => {
    if (!mode || !sheet) return;
    sheet.querySelector<HTMLInputElement>('input')?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!mode || !canSubmit) return;
    busy = true;
    error = null;
    try {
      if (mode === 'enable') {
        lock.status = await call('lock_enable', { password: next });
        showToast({ text: t('lock.dlg.enabled') });
      } else if (mode === 'disable') {
        lock.status = await call('lock_disable', { password: current });
        showToast({ text: t('lock.dlg.disabled') });
      } else {
        lock.status = await call('lock_change', { current, next });
        showToast({ text: t('lock.dlg.changed') });
      }
      busy = false;
      close();
    } catch (err) {
      error = toAppError(err).message;
      busy = false;
      if (needsCurrent) current = '';
    }
  }

  async function setRelock(on: boolean) {
    relockBusy = true;
    try {
      lock.status = await call('lock_set_relock', { on });
    } catch (err) {
      showToast({ text: t('toast.error', { message: toAppError(err).message }) });
    } finally {
      relockBusy = false;
    }
  }

  const titles: Record<Mode, MessageKey> = { enable: 'lock.dlg.enableTitle', disable: 'lock.dlg.disableTitle', change: 'lock.dlg.changeTitle' };
  const submits: Record<Mode, MessageKey> = { enable: 'lock.dlg.enable', disable: 'lock.dlg.disable', change: 'lock.dlg.change' };
</script>

<h3 class="set-h" id="lock-set-title">{t('lock.set.title')}</h3>
<p class="set-lead">{t(enabled ? 'lock.set.onHint' : 'lock.set.offHint')}</p>

<div class="set-row top">
  <span class="set-name">{t('lock.set.state')}<span class="set-hint">{t('lock.set.note')}</span></span>
  <div class="acts">
    {#if enabled}
      <span class="on"><Icon name="lock" size={16} />{t('lock.set.onShort')}</span>
      <button class="btn-soft" disabled={status == null} onclick={() => open('change')}>{t('lock.set.change')}</button>
      <button class="btn-soft" disabled={status == null} onclick={() => open('disable')}>{t('lock.set.disable')}</button>
    {:else}
      <button class="btn-primary small" disabled={status == null} onclick={() => open('enable')}>{t('lock.set.enable')}</button>
    {/if}
  </div>
</div>

{#if enabled}
  <div class="set-row top">
    <span class="set-name"
      >{t(mobile ? 'mob.lock.relock' : 'lock.set.relock')}<span class="set-hint">{t(mobile ? 'mob.lock.relockHint' : 'lock.set.relockHint')}</span></span
    >
    <input
      type="checkbox"
      role="switch"
      class="switch"
      aria-label={t(mobile ? 'mob.lock.relock' : 'lock.set.relock')}
      checked={status?.relockOnOpen ?? true}
      disabled={relockBusy}
      onchange={(e) => setRelock((e.currentTarget as HTMLInputElement).checked)}
    />
  </div>
{/if}

{#if mode}
  <div class="scrim" role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
    <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="lock-dlg-title" bind:this={sheet}>
    <form class="dbody" onsubmit={submit}>
      <div class="head">
        <span class="dmark"><Icon name="lock" /></span>
        <h2 id="lock-dlg-title">{t(titles[mode])}</h2>
        <button type="button" class="btn-icon" aria-label={t('act.close')} title="{t('act.close')} · Esc" onclick={close}><Icon name="x" /></button>
      </div>

      {#if mode === 'disable'}
        <p class="dlead">{t('lock.dlg.disableLead')}</p>
      {/if}

      {#if needsCurrent}
        <label class="field">
          <span class="label">{t('lock.dlg.current')}</span>
          <input class="input" type="password" autocomplete="current-password" bind:value={current} disabled={busy} />
        </label>
      {/if}

      {#if needsNew}
        <label class="field">
          <span class="label">{t('lock.dlg.new')}</span>
          <input class="input" type="password" autocomplete="new-password" bind:value={next} disabled={busy} aria-describedby="lock-dlg-hint" />
        </label>
        <p class="meta hint" id="lock-dlg-hint">
          {#if tooShort}
            {t('lock.dlg.min', { n: LOCK_PASSWORD_MIN })}
          {:else if level}
            <span class:weak={level === 'lock.dlg.weak'} class:strong={level === 'lock.dlg.strong'}>{t('lock.dlg.strength', { level: t(level) })}</span>
          {:else}
            {t('lock.dlg.min', { n: LOCK_PASSWORD_MIN })}
          {/if}
        </p>
        <label class="field">
          <span class="label">{t('lock.dlg.repeat')}</span>
          <input class="input" type="password" autocomplete="new-password" bind:value={repeat} disabled={busy} aria-invalid={mismatch ? 'true' : undefined} />
        </label>
        {#if mismatch}
          <p class="err" role="alert"><Icon name="alert" size={16} />{t('lock.dlg.mismatch')}</p>
        {/if}
      {/if}

      {#if mode === 'enable'}
        <p class="warn"><Icon name="alert" size={18} /><span>{t('lock.dlg.warn')}</span></p>
        <label class="ack">
          <input type="checkbox" bind:checked={ack} disabled={busy} />
          <span>{t('lock.dlg.ack')}</span>
        </label>
      {/if}

      {#if error}
        <p class="err" role="alert"><Icon name="alert" size={16} />{error}</p>
      {/if}

      <div class="set-actions">
        <button type="button" class="btn-ghost" onclick={close} disabled={busy}>{t('act.cancel')}</button>
        <button type="submit" class="btn-primary small" disabled={!canSubmit}>{busy ? t('lock.dlg.busy') : t(submits[mode])}</button>
      </div>
    </form>
    </div>
  </div>
{/if}

<style>
  .acts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .on {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-right: 4px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ok-text);
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 56px 16px 16px;
    background: color-mix(in srgb, var(--ground) 55%, transparent);
    animation: fade var(--t-fast) var(--ease);
  }

  .sheet {
    width: 440px;
    max-width: 100%;
    max-height: calc(100vh - 88px);
    display: flex;
    flex-direction: column;
    padding: 16px 20px 18px;
    overflow-y: auto;
    border-radius: var(--r-palette);
    background: var(--paper);
    border: 1px solid var(--line);
    box-shadow: var(--shadow-lg);
    animation: drop var(--t-state) var(--ease);
  }

  .dbody {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .head h2 {
    flex: 1;
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
  }

  .dmark {
    display: inline-grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: var(--r-icon);
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .dlead {
    color: var(--ink-2);
  }

  .field {
    display: block;
  }

  .hint {
    margin-top: -6px;
  }

  .weak {
    color: var(--amber-text);
  }

  .strong {
    color: var(--ok-text);
  }

  .warn {
    display: flex;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
    color: var(--ink-2);
    font-size: var(--fs-meta);
  }

  .warn :global(.icon) {
    flex: none;
    margin-top: 1px;
    color: var(--amber);
  }

  .ack {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    min-height: 32px;
    cursor: pointer;
  }

  .ack input {
    margin: 2px 0 0;
    width: 18px;
    height: 18px;
    accent-color: var(--accent);
  }

  .err {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  .err :global(.icon) {
    flex: none;
    margin-top: 1px;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-8px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .sheet {
      animation: none;
    }
  }

  @container settings (max-width: 600px) {
    .acts {
      justify-self: start;
    }
  }
</style>
