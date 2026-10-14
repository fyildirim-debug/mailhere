<script lang="ts">
  import { onMount } from 'svelte';
  import { call } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Logo from '../../lib/ui/Logo.svelte';
  import ToastHost from '../../lib/ui/ToastHost.svelte';
  import WindowControls from '../../lib/ui/WindowControls.svelte';
  import { runToastAction } from '../../lib/ui/toast.svelte';
  import { snoozeOptions } from '../../lib/util/format';
  import { isEditing, shortcutFor } from '../../lib/util/keys';
  import { initMail, mail, move, moveTo, select, selectedIds, setUnread, toggleStar } from '../../lib/stores/mail.svelte';
  import { bulkMove, bulkSetUnread, bulkSnooze, bulkStar, togglePin } from '../../lib/stores/bulk.svelte';
  import { checkAll, checkedIds, clearChecked, extendChecked, multi } from '../../lib/stores/selection.svelte';
  import { resolvedTheme, toggleTheme } from '../../lib/stores/settings.svelte';
  import { initUpdate } from '../../lib/stores/update.svelte';
  import BulkPanel from './BulkPanel.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import MessageList from './MessageList.svelte';
  import Onboarding from './Onboarding.svelte';
  import Reader from './Reader.svelte';
  import ScheduledView from './ScheduledView.svelte';
  import Settings from './settings/Settings.svelte';
  import Sidebar from './Sidebar.svelte';

  let reader = $state<ReturnType<typeof Reader>>();

  // Hesap ekleme sürerken (ilk hesap dahil) uygulamanın geri kalanı açılmaz:
  // kenar çubuğu, arama ve komut paleti "Gelen kutusunu aç" ile gelir.
  const setup = $derived(!mail.accounts.length || mail.screen === 'onboarding');
  // Birden çok satır seçiliyken okuma alanı yerine toplu işlem paneli.
  const bulk = $derived(multi());

  onMount(() => {
    void initMail();
    initUpdate();
  });

  // Görev çubuğunda okunmamış sayısı.
  $effect(() => {
    const n = mail.counts.inboxUnread;
    document.title = n ? `(${n}) ${t('app.name')}` : t('app.name');
  });

  function onKey(e: KeyboardEvent) {
    if (e.defaultPrevented) return;
    if ((e.ctrlKey || e.metaKey) && e.key === ',') {
      e.preventDefault();
      mail.screen = 'settings';
      return;
    }
    const sc = shortcutFor({ key: e.key, ctrlKey: e.ctrlKey, metaKey: e.metaKey, altKey: e.altKey, shiftKey: e.shiftKey, editing: isEditing(e.target) });
    if (!sc) return;
    if (sc === 'palette') {
      e.preventDefault();
      if (setup) return;
      mail.paletteOpen = !mail.paletteOpen;
      return;
    }
    if (mail.paletteOpen) return;
    if (sc === 'escape') {
      // Önce çoklu seçim bırakılır.
      if (mail.screen === 'mail' && mail.checked.length) clearChecked();
      else if (mail.screen !== 'mail' && mail.accounts.length) mail.screen = 'mail';
      else (document.activeElement as HTMLElement | null)?.blur();
      return;
    }
    if (mail.screen !== 'mail') return;
    const id = mail.selectedId;
    e.preventDefault();
    switch (sc) {
      case 'compose':
        void call('open_compose', { prefill: { mode: 'new', messageId: null } });
        break;
      case 'next':
        move(1);
        break;
      case 'prev':
        move(-1);
        break;
      case 'extendNext':
        extendChecked(1);
        break;
      case 'extendPrev':
        extendChecked(-1);
        break;
      case 'selectAll':
        checkAll();
        break;
      case 'undo':
        void runToastAction();
        break;
      default:
        if (bulk) {
          // Çoklu seçimde tek tuşlar seçili satırların tümüne uygulanır.
          const ids = checkedIds();
          if (sc === 'archive') void bulkMove(ids, 'archive');
          else if (sc === 'delete') void bulkMove(ids, 'trash');
          else if (sc === 'star') void bulkStar(ids, true);
          else if (sc === 'unread') void bulkSetUnread(ids, true);
          else if (sc === 'pin') void togglePin(ids);
          else if (sc === 'snooze') void bulkSnooze(ids, snoozeOptions(Date.now()).find((o) => o.key === 'tomorrow')!.at);
          return;
        }
        if (id == null) return;
        if (sc === 'archive') void moveTo(selectedIds(), 'archive');
        else if (sc === 'delete') void moveTo(selectedIds(), 'trash');
        else if (sc === 'star') void toggleStar(id);
        else if (sc === 'unread') void setUnread([id], true);
        else if (sc === 'pin') void togglePin(selectedIds());
        else if (sc === 'open') void select(id);
        else if (sc === 'reply') reader?.focusReply();
        else if (sc === 'snooze') reader?.openSnooze();
        else if (sc === 'replyAll' || sc === 'forward') void call('open_compose', { prefill: { mode: sc, messageId: id } });
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <Logo size={24} />
      <span class="name" data-tauri-drag-region>{t('app.name')}</span>
    </div>
    <div class="center" data-tauri-drag-region>
      {#if !setup}
        <button class="search" onclick={() => (mail.paletteOpen = true)}>
          <Icon name="search" size={16} />
          <span class="trunc grow">{t('palette.placeholder')}</span>
          <span class="kbd">Ctrl K</span>
        </button>
      {/if}
    </div>
    <div class="right" data-tauri-drag-region>
      <button class="btn-icon" aria-label={t('win.theme')} title={t('win.theme')} onclick={() => toggleTheme()}>
        <Icon name={resolvedTheme() === 'dark' ? 'sun' : 'moon'} />
      </button>
      <span class="sep"></span>
      <WindowControls />
    </div>
  </header>

  <div class="body">
    {#if !setup}<Sidebar />{/if}
    {#if setup}
      <!-- Tek örnek: hesap eklenince de aynı bileşen "Hazırsın" adımında kalır. -->
      {#if mail.ready}<Onboarding />{/if}
    {:else if mail.screen === 'settings'}
      <Settings />
    {:else if mail.screen === 'scheduled'}
      <ScheduledView />
    {:else}
      <MessageList />
      {#if bulk}
        <BulkPanel />
      {:else}
        <Reader bind:this={reader} />
      {/if}
    {/if}
  </div>

  {#if mail.paletteOpen}
    <CommandPalette onclose={() => (mail.paletteOpen = false)} />
  {/if}
  <ToastHost />
</div>

<style>
  .shell {
    height: 100vh;
    min-height: 560px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .titlebar {
    flex: none;
    height: 46px;
    display: grid;
    grid-template-columns: 1fr minmax(240px, 560px) 1fr;
    align-items: center;
    column-gap: 16px;
    padding-left: 14px;
    background: var(--ground);
    border-bottom: 1px solid var(--line);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .name {
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding: 0 6px 0 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-btn);
    background: var(--surface);
    color: var(--ink-3);
    text-align: left;
    transition: border-color var(--t-fast);
  }

  .search:hover {
    border-color: var(--line-strong);
    color: var(--ink-2);
  }

  .grow {
    flex: 1;
  }

  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    align-self: stretch;
  }

  .sep {
    width: 1px;
    height: 20px;
    margin: 0 6px;
    background: var(--line-strong);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  @media (max-width: 1120px) {
    .body :global(.side) {
      display: none;
    }
  }

  @media (max-width: 780px) {
    .body :global(.list) {
      width: 100%;
      border-right: 0;
    }

    .body :global(.reader) {
      display: none;
    }

    .center {
      display: none;
    }
  }
</style>
