<script lang="ts">
  // Gizlilik ve güvenlik › İstenmeyen koruması, Engellenen göndericiler ve
  // Doğrulama kodları (masaüstü). Mobil ayarlar aynı ayarları kendi bölümüyle
  // (MobileGuard) sunar; PrivacySection bunu mobil kabukta çizmez.
  import { BLOCKED_MAX, normalizeBlocked, normalizeBlockedEntry, toAppError, type SpamFilter } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import { mail } from '../../../lib/stores/mail.svelte';
  import { refreshSettings, settings, updateSettings } from '../../../lib/stores/settings.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';

  const s = $derived(settings.value);
  const uid = $props.id();

  const filters: SpamFilter[] = ['off', 'warn', 'move'];
  /** Kod postalarını silme süresi (saat): kapalı, 1 saat, 1 gün. */
  const expiries: [number, MessageKey][] = [
    [0, 'otp.expire.off'],
    [1, 'otp.expire.hour'],
    [24, 'otp.expire.day'],
  ];

  /** Ayarı yazar; arka uç reddederse nedenini söyler ve kayıtlı ayara dönülür. */
  async function save(patch: Parameters<typeof updateSettings>[0]) {
    try {
      await updateSettings(patch);
    } catch (e) {
      void refreshSettings();
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  // ------------------------------------------------------ engellenenler

  let entry = $state('');
  let error = $state<string | null>(null);
  let input = $state<HTMLInputElement>();

  /** Girdiyi doğrular: adres ya da `@alanadi`, kendi adresimiz değil, listede yok, liste dolu değil. */
  function check(raw: string): { value: string } | { error: string } {
    const value = normalizeBlockedEntry(raw);
    if (!value) return { error: t('block.addInvalid') };
    if (mail.accounts.some((a) => a.email.trim().toLowerCase() === value)) return { error: t('block.addOwn') };
    if (s.blocked.includes(value)) return { error: t('block.addDuplicate', { entry: value }) };
    if (s.blocked.length >= BLOCKED_MAX) return { error: t('block.addFull', { n: BLOCKED_MAX }) };
    return { value };
  }

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!entry.trim()) {
      error = t('block.addEmpty');
      input?.focus();
      return;
    }
    const r = check(entry);
    if ('error' in r) {
      error = r.error;
      input?.focus();
      return;
    }
    error = null;
    entry = '';
    await save({ blocked: normalizeBlocked([...s.blocked, r.value]) });
    input?.focus();
  }

  async function remove(value: string) {
    const had = s.blocked;
    await save({ blocked: had.filter((x) => x !== value) });
    showToast({
      text: t('block.removed', { entry: value }),
      action: {
        label: t('act.undo'),
        // Sırası korunarak geri gelir; bu arada eklenenler kalır.
        run: () => save({ blocked: normalizeBlocked([...had, ...settings.value.blocked]) }),
      },
    });
  }
</script>

<h3 class="set-h">{t('spam.set.title')}</h3>
<p class="set-lead">{t('spam.set.lead')}</p>
<div class="set-row top">
  <span class="set-name">{t('spam.set.filter')}<span class="set-hint" role="status">{t(`spam.set.hint.${s.spamFilter}`)}</span></span>
  <div class="seg-track" role="radiogroup" aria-label={t('spam.set.filter')}>
    {#each filters as v (v)}
      <button class="seg" role="radio" aria-checked={s.spamFilter === v} onclick={() => save({ spamFilter: v })}>{t(`spam.set.mode.${v}`)}</button>
    {/each}
  </div>
</div>

<h3 class="set-h">{t('block.set.title')}</h3>
<p class="set-lead">{t('block.set.lead')}</p>
<form class="add" novalidate onsubmit={add}>
  <label class="sr-only" for="{uid}-entry">{t('block.set.inputLabel')}</label>
  <input
    id="{uid}-entry"
    class="input small"
    type="text"
    inputmode="email"
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
    placeholder={t('block.set.placeholder')}
    aria-invalid={error != null}
    aria-describedby={error ? `${uid}-error` : undefined}
    bind:this={input}
    bind:value={entry}
    oninput={() => (error = null)}
  />
  <button class="btn-soft" type="submit">{t('block.set.add')}</button>
</form>
{#if error}<p id="{uid}-error" class="set-error add-error" role="alert">{error}</p>{/if}
{#if s.blocked.length}
  <p class="meta count">{t('block.set.count', { n: s.blocked.length, max: BLOCKED_MAX })}</p>
  <ul class="list" aria-label={t('block.set.title')}>
    {#each s.blocked as who (who)}
      <li class="list-row">
        <span class="trunc who">{who}</span>
        {#if who.startsWith('@')}<span class="meta kind">{t('block.set.domain')}</span>{/if}
        <span class="grow"></span>
        <button class="btn-ghost" aria-label={t('block.set.removeLabel', { entry: who })} onclick={() => remove(who)}>{t('set.remove')}</button>
      </li>
    {/each}
  </ul>
{:else}
  <p class="meta none">{t('block.set.none')}</p>
{/if}

<h3 class="set-h">{t('otp.set.title')}</h3>
<div class="set-row top">
  <span class="set-name">{t('otp.set.autoCopy')}<span class="set-hint">{t('otp.set.autoCopyHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('otp.set.autoCopy')}
    checked={s.otpAutoCopy}
    onchange={(e) => save({ otpAutoCopy: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>
<div class="set-row top">
  <span class="set-name">{t('otp.set.markRead')}<span class="set-hint">{t('otp.set.markReadHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('otp.set.markRead')}
    checked={s.otpMarkRead}
    onchange={(e) => save({ otpMarkRead: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>
<div class="set-row top">
  <span class="set-name">{t('otp.set.expire')}<span class="set-hint">{t('otp.set.expireHint')}</span></span>
  <div class="seg-track" role="radiogroup" aria-label={t('otp.set.expire')}>
    {#each expiries as [v, k] (v)}
      <button class="seg" role="radio" aria-checked={s.otpExpireHours === v} onclick={() => save({ otpExpireHours: v })}>{t(k)}</button>
    {/each}
  </div>
</div>

<style>
  .add {
    display: flex;
    gap: 8px;
    max-width: 460px;
    margin-top: 4px;
  }

  .add .input {
    flex: 1;
    min-width: 0;
  }

  .add button {
    flex: none;
  }

  .add-error {
    margin-top: 6px;
    max-width: 62ch;
  }

  .count {
    margin-top: 14px;
  }

  /* Uzun liste sayfayı uzatmaz: kendi içinde kayar. */
  .list {
    display: flex;
    flex-direction: column;
    max-height: 320px;
    margin: 6px 0 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
    border-top: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
  }

  .list-row {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 0 4px;
    border-bottom: 1px solid var(--line);
  }

  .list-row:last-child {
    border-bottom: 0;
  }

  .who {
    min-width: 0;
  }

  .kind {
    flex: none;
  }

  .grow {
    flex: 1;
  }

  .none {
    max-width: 62ch;
    margin-top: 12px;
  }
</style>
