<script lang="ts">
  // Ayarlar › Gizlilik ve güvenlik'in mobil eki: istenmeyen koruması (şüpheli
  // postaya ne yapılacağı), engellenen göndericiler (liste, kaldır, elle ekle)
  // ve doğrulama kodu kolaylıkları. Seçenekler alt sayfada açılır (Menu →
  // Sheet); her değişiklik hemen kaydedilir (`updateSettings`).
  //
  // Elle eklemede girdi çekirdekle aynı kuralla doğrulanır (blocked.ts); neden
  // eklenemediği ve ne yapılacağı girdinin altında söylenir. Kaldırma toast'taki
  // "Geri al" ile döner.
  import { BLOCKED_MAX, toAppError, type Settings, type SpamFilter } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import { isLockedError } from '../../lib/stores/lock.svelte';
  import { mail } from '../../lib/stores/mail.svelte';
  import { settings, updateSettings } from '../../lib/stores/settings.svelte';
  import { checkBlockedEntry, type BlockedReject } from './blocked';
  import PickRow from './PickRow.svelte';

  const s = $derived(settings.value);
  let menu = $state<{ title: string; items: MenuItem[] } | null>(null);

  /** Ayarı yazar; olmazsa ne olduğunu söyler (ayar eski değerine arka uçtan döner). */
  async function save(patch: Partial<Settings>): Promise<boolean> {
    try {
      await updateSettings(patch);
      return true;
    } catch (e) {
      if (!isLockedError(e)) showToast({ text: t('toast.error', { message: toAppError(e).message }) });
      return false;
    }
  }

  // ------------------------------------------------------ istenmeyen koruması

  const filters: { key: SpamFilter; label: MessageKey; hint: MessageKey }[] = [
    { key: 'off', label: 'mob.spam.off', hint: 'mob.spam.offHint' },
    { key: 'warn', label: 'mob.spam.warn', hint: 'mob.spam.warnHint' },
    { key: 'move', label: 'mob.spam.move', hint: 'mob.spam.moveHint' },
  ];
  const filter = $derived(filters.find((f) => f.key === s.spamFilter) ?? filters[1]);

  function pickFilter() {
    menu = {
      title: t('mob.spam.filter'),
      items: filters.map((f) => ({ key: f.key, label: t(f.label), checked: s.spamFilter === f.key, onclick: () => void save({ spamFilter: f.key }) })),
    };
  }

  // ------------------------------------------------- engellenen göndericiler

  let entry = $state('');
  let error = $state<string | null>(null);
  let adding = $state(false);

  const rejects: Record<BlockedReject, MessageKey> = {
    invalid: 'mob.spam.addInvalid',
    duplicate: 'mob.spam.addDup',
    own: 'mob.spam.addOwn',
    full: 'mob.spam.addFull',
  };

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (adding) return;
    const r = checkBlockedEntry(
      s.blocked,
      entry,
      mail.accounts.map((a) => a.email),
    );
    if (!r.ok) {
      error = t(rejects[r.reason], { entry: r.entry ?? entry.trim(), n: BLOCKED_MAX });
      return;
    }
    error = null;
    adding = true;
    if (await save({ blocked: r.list })) entry = '';
    adding = false;
  }

  /** Engeli kaldırır; "Geri al" girdiyi eski sırasına koyar. */
  async function remove(who: string) {
    const at = s.blocked.indexOf(who);
    if (at < 0 || !(await save({ blocked: s.blocked.filter((x) => x !== who) }))) return;
    showToast({
      text: t('mob.spam.removed', { entry: who }),
      action: {
        label: t('act.undo'),
        run: async () => {
          const now = settings.value.blocked;
          if (now.includes(who)) return;
          await save({ blocked: [...now.slice(0, at), who, ...now.slice(at)] });
        },
      },
    });
  }

  // --------------------------------------------------------- doğrulama kodları

  const expires: { hours: number; label: MessageKey }[] = [
    { hours: 0, label: 'mob.otp.expireOff' },
    { hours: 1, label: 'mob.otp.expire1' },
    { hours: 24, label: 'mob.otp.expire24' },
  ];
  /** Sunulan üç değer dışında bir süre (başka cihazdan gelen ayar) saat olarak yazılır. */
  const expireLabel = $derived.by(() => {
    const known = expires.find((x) => x.hours === s.otpExpireHours);
    return known ? t(known.label) : t('mob.otp.expireHours', { n: s.otpExpireHours });
  });

  function pickExpire() {
    menu = {
      title: t('mob.otp.expire'),
      items: expires.map((x) => ({
        key: String(x.hours),
        label: t(x.label),
        checked: s.otpExpireHours === x.hours,
        onclick: () => void save({ otpExpireHours: x.hours }),
      })),
    };
  }
</script>

<h3 class="set-h">{t('mob.spam.title')}</h3>
<p class="set-lead">{t('mob.spam.lead')}</p>
<PickRow name={t('mob.spam.filter')} hint={t(filter.hint)} value={t(filter.label)} onclick={pickFilter} />

<h3 class="set-h">{t('mob.spam.blocked')}</h3>
<p class="set-lead">{t('mob.spam.blockedLead')}</p>
{#if s.blocked.length}
  <ul class="blocked" aria-label={t('mob.spam.blocked')}>
    {#each s.blocked as who (who)}
      <li class="blocked-row">
        <Icon name="block" size={18} />
        <span class="trunc grow">{who}</span>
        <button class="btn-ghost remove" aria-label={t('mob.spam.remove', { entry: who })} onclick={() => void remove(who)}>{t('set.remove')}</button>
      </li>
    {/each}
  </ul>
{:else}
  <p class="meta none">{t('mob.spam.blockedNone')}</p>
{/if}
<form class="add" novalidate onsubmit={add}>
  <label class="label" for="mob-block-entry">{t('mob.spam.addLabel')}</label>
  <div class="add-row">
    <input
      id="mob-block-entry"
      class="input"
      type="text"
      inputmode="email"
      autocapitalize="none"
      autocomplete="off"
      spellcheck="false"
      placeholder={t('mob.spam.addPlaceholder')}
      aria-invalid={error ? 'true' : undefined}
      aria-describedby={error ? 'mob-block-error' : undefined}
      bind:value={entry}
      oninput={() => (error = null)}
    />
    <button class="btn-soft" type="submit" disabled={!entry.trim()} aria-busy={adding}>{t('mob.spam.add')}</button>
  </div>
  {#if error}<p class="set-error" id="mob-block-error" role="alert">{error}</p>{/if}
</form>

<h3 class="set-h">{t('mob.otp.title')}</h3>
<div class="set-row top">
  <span class="set-name">{t('mob.otp.autoCopy')}<span class="set-hint">{t('mob.otp.autoCopyHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('mob.otp.autoCopy')}
    checked={s.otpAutoCopy}
    onchange={(e) => void save({ otpAutoCopy: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>
<div class="set-row top">
  <span class="set-name">{t('mob.otp.markRead')}<span class="set-hint">{t('mob.otp.markReadHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('mob.otp.markRead')}
    checked={s.otpMarkRead}
    onchange={(e) => void save({ otpMarkRead: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>
<PickRow name={t('mob.otp.expire')} hint={t('mob.otp.expireHint')} value={expireLabel} onclick={pickExpire} />

{#if menu}<Menu title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}

<style>
  .blocked {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--line);
    list-style: none;
  }

  .blocked-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding-left: 4px;
    border-bottom: 1px solid var(--line);
  }

  .blocked-row :global(.icon) {
    color: var(--ink-3);
  }

  .grow {
    flex: 1;
  }

  /* Metin düğmesi; dokunma hedefi 44 px. */
  .remove {
    flex: none;
    min-height: var(--m-touch);
    color: var(--accent-text);
  }

  .none {
    margin: 0;
    padding: 10px 0 2px;
    max-width: 62ch;
  }

  .add {
    margin-top: 14px;
  }

  .add-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .add-row .input {
    flex: 1;
    min-width: 0;
  }

  .add-row .btn-soft:disabled {
    opacity: var(--dim-disabled);
  }

  .add .set-error {
    margin: 6px 0 0;
  }
</style>
