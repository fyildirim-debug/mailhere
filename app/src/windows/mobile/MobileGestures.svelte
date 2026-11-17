<script lang="ts">
  // Ayarlar › Genel'in mobil eki: liste satırını sola / sağa kaydırınca ne
  // olacağı ve okuma ekranında arşivleyip silince nereye gidileceği. Her satır
  // alt sayfada seçenekleri açar (Menu → Sheet); seçim hemen kaydedilir.
  import type { AfterRemove, SwipeAction } from '../../lib/api';
  import { t, type MessageKey } from '../../lib/i18n/index.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { settings, updateSettings } from '../../lib/stores/settings.svelte';
  import PickRow from './PickRow.svelte';

  // Sıra: listeden çıkaranlar, seçici, erteleme, işaretler, kapalı.
  const actions: { key: SwipeAction; label: MessageKey; icon: IconName }[] = [
    { key: 'archive', label: 'mob.swipe.archive', icon: 'archive' },
    { key: 'trash', label: 'mob.swipe.trash', icon: 'trash' },
    { key: 'spam', label: 'mob.swipe.spam', icon: 'spam' },
    { key: 'move', label: 'mob.swipe.move', icon: 'folder' },
    { key: 'snooze', label: 'mob.swipe.snooze', icon: 'clock' },
    { key: 'toggleRead', label: 'mob.swipe.toggleRead', icon: 'mail' },
    { key: 'pin', label: 'mob.swipe.pin', icon: 'pin' },
    { key: 'none', label: 'mob.swipe.none', icon: 'x' },
  ];

  const afters: { key: AfterRemove; label: MessageKey }[] = [
    { key: 'list', label: 'mob.after.list' },
    { key: 'next', label: 'mob.after.next' },
    { key: 'previous', label: 'mob.after.previous' },
  ];

  const s = $derived(settings.value);
  let menu = $state<{ title: string; items: MenuItem[] } | null>(null);

  function actionOf(a: SwipeAction) {
    return actions.find((x) => x.key === a) ?? actions[actions.length - 1];
  }

  function pickSwipe(side: 'swipeLeft' | 'swipeRight') {
    menu = {
      title: t(side === 'swipeLeft' ? 'mob.swipe.left' : 'mob.swipe.right'),
      items: actions.map((a) => ({
        key: a.key,
        label: t(a.label),
        icon: a.icon,
        checked: s[side] === a.key,
        onclick: () => void updateSettings({ [side]: a.key }),
      })),
    };
  }

  function pickAfter() {
    menu = {
      title: t('mob.after.title'),
      items: afters.map((a) => ({ key: a.key, label: t(a.label), checked: s.afterRemove === a.key, onclick: () => void updateSettings({ afterRemove: a.key }) })),
    };
  }
</script>

<h3 class="set-h">{t('mob.gestures.title')}</h3>
<p class="lead set-hint">{t('mob.gestures.lead')}</p>

<PickRow name={t('mob.swipe.left')} value={t(actionOf(s.swipeLeft).label)} icon={actionOf(s.swipeLeft).icon} onclick={() => pickSwipe('swipeLeft')} />
<PickRow name={t('mob.swipe.right')} value={t(actionOf(s.swipeRight).label)} icon={actionOf(s.swipeRight).icon} onclick={() => pickSwipe('swipeRight')} />
<PickRow
  name={t('mob.after.title')}
  hint={t('mob.after.hint')}
  value={t(afters.find((a) => a.key === s.afterRemove)?.label ?? 'mob.after.list')}
  onclick={pickAfter}
/>

{#if menu}<Menu title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}

<style>
  .lead {
    display: block;
    margin: 0 0 4px;
    max-width: 52ch;
  }
</style>
