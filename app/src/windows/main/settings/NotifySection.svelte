<script lang="ts">
  import { call, toAppError, type Account, type CardPosition, type NotifyMode } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { mail, reloadAccounts } from '../../../lib/stores/mail.svelte';
  import { settings, updateSettings } from '../../../lib/stores/settings.svelte';
  import { accountUpdate } from './account';

  const s = $derived(settings.value);

  const modes: [NotifyMode, MessageKey][] = [
    ['people', 'set.notify.people'],
    ['all', 'set.notify.all'],
    ['vip', 'set.notify.vip'],
    ['off', 'set.notify.off'],
  ];
  const positions: [CardPosition, MessageKey][] = [
    ['bottomRight', 'set.pos.bottomRight'],
    ['topRight', 'set.pos.topRight'],
    ['bottomLeft', 'set.pos.bottomLeft'],
    ['topLeft', 'set.pos.topLeft'],
  ];
  const cardSeconds = [5, 8, 12, 20];

  let vipText = $state('');
  $effect(() => {
    vipText = settings.value.vip.join('\n');
  });

  function saveVip() {
    const vip = vipText
      .split(/[\n,;]+/)
      .map((x) => x.trim().toLowerCase())
      .filter((x) => x.includes('@'));
    void updateSettings({ vip });
  }

  async function setAccountNotify(a: Account, notify: boolean) {
    try {
      await call('account_update', { id: a.id, update: { ...accountUpdate(a), notify } });
      await reloadAccounts();
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }
</script>

<h2 class="page">{t('set.nav.notifications')}</h2>

<h3 class="set-h">{t('set.notifyWhat')}</h3>
<div class="set-row top">
  <span class="set-name">{t('set.notify')}<span class="set-hint">{t('set.notifyHint')}</span></span>
  <div class="seg-track" role="radiogroup" aria-label={t('set.notify')}>
    {#each modes as [v, k] (v)}
      <button class="seg" role="radio" aria-checked={s.notify === v} onclick={() => updateSettings({ notify: v })}>{t(k)}</button>
    {/each}
  </div>
</div>
{#if s.notify === 'vip'}
  <div class="set-row top">
    <label class="set-name" for="vip">{t('set.vip')}<span class="set-hint">{t('set.vipHint')}</span></label>
    <textarea id="vip" class="input" rows="3" bind:value={vipText} onblur={saveVip}></textarea>
  </div>
{/if}
<div class="set-row top">
  <span class="set-name">{t('set.perAccount')}<span class="set-hint">{t('set.perAccountHint')}</span></span>
  <div class="accs">
    {#each mail.accounts as a (a.id)}
      <label class="acc">
        <span class="dot" style:background={a.color}></span>
        <span class="trunc grow">{a.name}</span>
        <input
          type="checkbox"
          role="switch"
          class="switch"
          aria-label={t('set.accountNotify', { name: a.name })}
          checked={a.notify}
          onchange={(e) => setAccountNotify(a, (e.currentTarget as HTMLInputElement).checked)}
        />
      </label>
    {/each}
  </div>
</div>

<h3 class="set-h">{t('set.cardTitle')}</h3>
<div class="set-row">
  <span class="set-name">{t('set.cardPosition')}</span>
  <div class="seg-track" role="radiogroup" aria-label={t('set.cardPosition')}>
    {#each positions as [v, k] (v)}
      <button class="seg" role="radio" aria-checked={s.cardPosition === v} onclick={() => updateSettings({ cardPosition: v })}>{t(k)}</button>
    {/each}
  </div>
</div>
<div class="set-row">
  <span class="set-name">{t('set.cardSeconds')}</span>
  <div class="seg-track" role="radiogroup" aria-label={t('set.cardSeconds')}>
    {#each cardSeconds as n (n)}
      <button class="seg num" role="radio" aria-checked={s.cardSeconds === n} onclick={() => updateSettings({ cardSeconds: n })}>{t('set.seconds', { n })}</button>
    {/each}
  </div>
</div>
<div class="set-row top">
  <span class="set-name">{t('set.native')}<span class="set-hint">{t('set.nativeHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('set.native')}
    checked={s.nativeNotifications}
    onchange={(e) => updateSettings({ nativeNotifications: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>
<div class="set-row">
  <span class="set-name"></span>
  <button class="btn-soft test" onclick={() => call('demo_notify')}><Icon name="bell" size={16} />{t('set.testCard')}</button>
</div>

<h3 class="set-h">{t('set.quiet')}</h3>
<div class="set-row top">
  <span class="set-name">{t('set.quietOn')}<span class="set-hint">{t('set.quietHint')}</span></span>
  <div class="quiet">
    <input
      type="checkbox"
      role="switch"
      class="switch"
      aria-label={t('set.quiet')}
      checked={s.quietStart != null}
      onchange={(e) =>
        updateSettings(
          (e.currentTarget as HTMLInputElement).checked ? { quietStart: '22:00', quietEnd: '08:00' } : { quietStart: null, quietEnd: null },
        )}
    />
    {#if s.quietStart != null}
      <label class="time"><span class="meta">{t('set.quietFrom')}</span>
        <input class="input small num" type="time" value={s.quietStart} onchange={(e) => updateSettings({ quietStart: (e.currentTarget as HTMLInputElement).value })} />
      </label>
      <label class="time"><span class="meta">{t('set.quietTo')}</span>
        <input class="input small num" type="time" value={s.quietEnd ?? '08:00'} onchange={(e) => updateSettings({ quietEnd: (e.currentTarget as HTMLInputElement).value })} />
      </label>
    {/if}
  </div>
</div>

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .accs {
    display: flex;
    flex-direction: column;
  }

  .acc {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    border-bottom: 1px solid var(--line);
  }

  .acc:last-child {
    border-bottom: 0;
  }

  .grow {
    flex: 1;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
  }

  .test {
    justify-self: start;
  }

  .quiet {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 16px;
  }

  .time {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .time .input {
    width: 128px;
  }
</style>
