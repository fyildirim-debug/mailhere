<script lang="ts">
  import { call, NOTIFY_ACTIONS_MAX, toAppError, type Account, type CardPosition, type NotifyAction, type NotifyMode } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { isMobileShell } from '../../../lib/shell';
  import { mail, reloadAccounts } from '../../../lib/stores/mail.svelte';
  import { settings, updateSettings } from '../../../lib/stores/settings.svelte';
  import { accountUpdate } from './account';
  import { NOTIFY_ACTION_INFO, NOTIFY_ACTION_ORDER, shownNotifyActions, toggleNotifyAction } from '../../../lib/notify-actions';
  import { notifyPermission, requestNotifyPermission, type NotifyPermission } from '../../../lib/notify-permission';
  import {
    backgroundMode,
    batteryStatus,
    liftBatteryRestriction,
    setBackgroundMode,
    type BatteryStatus,
  } from '../../../lib/notify-health';

  const s = $derived(settings.value);
  // Mobilde kart yok, işletim sistemi bildirimi var: kart konumu/süresi, yerel
  // bildirim anahtarı ve örnek kart gizlenir; ipuçları "bildirim" der.
  const mobile = isMobileShell;

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

  // Bildirim düğmeleri: kanonik sırada en çok üç; üçü seçiliyken diğerleri kapalı.
  const naId = $props.id();
  const chosen = $derived(shownNotifyActions(s.notifyActions));
  const full = $derived(chosen.length >= NOTIFY_ACTIONS_MAX);

  function setNotifyAction(a: NotifyAction, on: boolean) {
    void updateSettings({ notifyActions: toggleNotifyAction(chosen, a, on) });
  }

  function actionHint(a: NotifyAction): MessageKey {
    return mobile && a === 'reply' ? 'mob.notify.replyHint' : NOTIFY_ACTION_INFO[a].hint;
  }

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

  // Android: sistem bildirim izni ve bildirim sağlığı (anlık bildirim, pil
  // kısıtlaması). Ayarlardan dönünce (görünürlük) yeniden okunur; komut yoksa
  // (masaüstü) satırlar gizli kalır.
  let perm = $state<NotifyPermission>('unsupported');
  let instant = $state<boolean | null>(null);
  let instantBusy = $state(false);
  let battery = $state<BatteryStatus | null>(null);
  const batteryHints: Record<BatteryStatus, MessageKey> = {
    unrestricted: 'mob.notify.batteryNoneHint',
    optimized: 'mob.notify.batteryOptimizedHint',
    restricted: 'mob.notify.batteryRestrictedHint',
  };
  $effect(() => {
    if (!mobile) return;
    const read = () => {
      void notifyPermission().then((p) => (perm = p));
      void backgroundMode().then((on) => {
        if (!instantBusy) instant = on;
      });
      void batteryStatus().then((b) => (battery = b));
    };
    read();
    const onVisible = () => {
      if (document.visibilityState === 'visible') read();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => document.removeEventListener('visibilitychange', onVisible);
  });

  async function allowNotifications() {
    perm = await requestNotifyPermission();
  }

  function openNotificationSettings() {
    call('notification_settings').catch((e) => showToast({ text: t('toast.error', { message: toAppError(e).message }) }));
  }

  // Anlık bildirim: anahtar hemen döner; başlatılamazsa son durum okunur, neden yazılır.
  async function toggleInstant(on: boolean) {
    instant = on;
    instantBusy = true;
    try {
      instant = await setBackgroundMode(on);
    } catch (e) {
      instant = await backgroundMode();
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    } finally {
      instantBusy = false;
    }
  }

  async function liftBattery() {
    try {
      battery = await liftBatteryRestriction();
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
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

{#if mobile && perm !== 'unsupported'}
  <div class="set-row">
    <span class="set-name"
      >{t('mob.notify.system')}<span class="set-hint"
        >{t(perm === 'granted' ? 'mob.notify.onHint' : perm === 'denied' ? 'mob.notify.deniedHint' : 'mob.notify.askHint')}</span
      ></span
    >
    {#if perm === 'granted'}
      <span class="perm-on">{t('mob.notify.on')}</span>
    {:else if perm === 'denied'}
      <button class="btn-soft" onclick={openNotificationSettings}>{t('mob.notify.openSettings')}</button>
    {:else}
      <button class="btn-primary" onclick={allowNotifications}>{t('mob.notify.allow')}</button>
    {/if}
  </div>
  {#if instant !== null}
    <div class="set-row top">
      <span class="set-name"
        >{t('mob.notify.instant')}<span class="set-hint">{t(instant ? 'mob.notify.instantOnHint' : 'mob.notify.instantOffHint')}</span></span
      >
      <input
        type="checkbox"
        role="switch"
        class="switch"
        aria-label={t('mob.notify.instant')}
        checked={instant}
        disabled={instantBusy}
        onchange={(e) => toggleInstant((e.currentTarget as HTMLInputElement).checked)}
      />
    </div>
  {/if}
  {#if battery !== null}
    <div class="set-row battery">
      <span class="set-name">{t('mob.notify.battery')}<span class="set-hint">{t(batteryHints[battery])}</span></span>
      {#if battery === 'unrestricted'}
        <span class="perm-on">{t('mob.notify.batteryNone')}</span>
      {:else}
        <button class="btn-soft" onclick={liftBattery}>{t('mob.notify.batteryLift')}</button>
      {/if}
    </div>
    <p class="set-hint vendor">{t('mob.notify.vendorNote')}</p>
  {/if}
{/if}

<h3 class="set-h">{t('set.notifyWhat')}</h3>
<div class="set-row top">
  <span class="set-name">{t(mobile ? 'mob.settings.notify' : 'set.notify')}<span class="set-hint">{t(mobile ? 'mob.settings.notifyHint' : 'set.notifyHint')}</span></span>
  <div class="seg-track" role="radiogroup" aria-label={t(mobile ? 'mob.settings.notify' : 'set.notify')}>
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
  <span class="set-name">{t('set.perAccount')}<span class="set-hint">{t(mobile ? 'mob.settings.perAccountHint' : 'set.perAccountHint')}</span></span>
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

<h3 class="set-h">{t('set.screenerTitle')}</h3>
<div class="set-row top">
  <span class="set-name">{t('set.screener')}<span class="set-hint">{t(mobile ? 'mob.settings.screenerHint' : 'set.screenerHint')}</span></span>
  <input
    type="checkbox"
    role="switch"
    class="switch"
    aria-label={t('set.screener')}
    checked={s.screener}
    onchange={(e) => updateSettings({ screener: (e.currentTarget as HTMLInputElement).checked })}
  />
</div>

{#if !mobile}
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
{/if}

<h3 class="set-h">{t('set.notifyActionsTitle')}</h3>
<div class="set-row top">
  <span class="set-name" id="{naId}-name"
    >{t('set.notifyActions')}<span class="set-hint" id="{naId}-hint">{t(mobile ? 'mob.notify.actionsHint' : 'set.notifyActionsHint')}</span></span
  >
  <div class="na">
    <div class="na-list" role="group" aria-labelledby="{naId}-name" aria-describedby="{naId}-hint">
      {#each NOTIFY_ACTION_ORDER as a (a)}
        {@const info = NOTIFY_ACTION_INFO[a]}
        {@const on = chosen.includes(a)}
        <label class="na-row" class:off={!on && full}>
          <Icon name={info.icon} size={mobile ? 22 : 16} />
          <span class="na-text">
            <span class="na-name">{t(info.label)}</span>
            <span class="set-hint" id="{naId}-{a}">{t(actionHint(a))}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            aria-label={t(info.label)}
            aria-describedby="{naId}-{a}"
            checked={on}
            disabled={!on && full}
            onchange={(e) => setNotifyAction(a, (e.currentTarget as HTMLInputElement).checked)}
          />
        </label>
      {/each}
    </div>
    <p class="set-hint na-full" role="status">{full ? t('set.notifyActionsFull') : ''}</p>
    {#if !mobile}
      <!-- Kartın düğme sırası; yalnız görünüm, etkileşimsiz. -->
      <div class="na-preview" role="img" aria-label={t('set.notifyActionsPreviewLabel', { list: [t('act.open'), ...chosen.map((a) => t(NOTIFY_ACTION_INFO[a].label))].join(', ') })}>
        <span class="meta">{t('set.notifyActionsPreview')}</span>
        <span class="na-btns" aria-hidden="true">
          <span class="btn-primary small">{t('act.open')}</span>
          {#each chosen as a (a)}
            <span class="btn-soft small">{t(NOTIFY_ACTION_INFO[a].label)}</span>
          {/each}
        </span>
      </div>
    {/if}
  </div>
</div>

<h3 class="set-h">{t('set.quiet')}</h3>
<div class="set-row top">
  <span class="set-name">{t('set.quietOn')}<span class="set-hint">{t(mobile ? 'mob.settings.quietHint' : 'set.quietHint')}</span></span>
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
  .perm-on {
    flex: none;
    font-weight: 500;
    color: var(--ok-text);
  }

  /* Üretici notu pil satırının açıklamasıdır: satırla aynı çizgide biter. */
  .battery {
    border-bottom: 0;
    padding-bottom: 4px;
  }

  .vendor {
    margin: 0;
    padding: 0 0 14px;
    border-bottom: 1px solid var(--line);
    max-width: 62ch;
  }

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

  .na {
    display: flex;
    flex-direction: column;
    max-width: 480px;
  }

  .na-list {
    display: flex;
    flex-direction: column;
  }

  /* Satırın tamamı anahtarın etiketi: dokununca açılıp kapanır. */
  .na-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
    cursor: pointer;
  }

  .na-row:last-child {
    border-bottom: 0;
  }

  .na-row > :global(.icon) {
    color: var(--ink-2);
  }

  .na-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .na-name {
    font-weight: 500;
  }

  /* Üçü seçiliyken seçilemeyenler geri çekilir. */
  .na-row.off {
    cursor: default;
  }

  .na-row.off .na-name {
    color: var(--ink-2);
  }

  .na-full {
    margin: 4px 0 0;
  }

  /* Canlı bölge hep yerinde durur (duyurulsun); boşken yer kaplamaz. */
  .na-full:empty {
    margin: 0;
  }

  .na-preview {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    background: var(--surface);
  }

  .na-btns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .na-btns > span {
    pointer-events: none;
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

  @container settings (max-width: 600px) {
    .acc {
      min-height: 44px;
    }

    .na {
      max-width: none;
    }

    /* Mobil ayar listesi: ayraç etiket hizasından başlar (ikon 22 + 14). */
    .na-row {
      position: relative;
      min-height: 56px;
      gap: 14px;
      border-bottom: 0;
    }

    .na-row + .na-row::before {
      content: '';
      position: absolute;
      top: 0;
      left: 36px;
      right: 0;
      border-top: 1px solid var(--line);
    }

    .quiet {
      flex-direction: column;
      align-items: stretch;
      gap: 10px;
    }

    .time {
      justify-content: space-between;
    }

    .time .input {
      width: 160px;
    }
  }
</style>
