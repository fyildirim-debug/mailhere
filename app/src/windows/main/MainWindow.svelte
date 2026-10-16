<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { call, EVENTS, on, type Mailbox } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import { flatten, folderIcon, folderLabel, folderTree } from '../../lib/mail/folders';
  import Icon from '../../lib/ui/Icon.svelte';
  import Logo from '../../lib/ui/Logo.svelte';
  import ToastHost from '../../lib/ui/ToastHost.svelte';
  import WindowControls from '../../lib/ui/WindowControls.svelte';
  import { runToastAction, showToast } from '../../lib/ui/toast.svelte';
  import { snoozeOptions } from '../../lib/util/format';
  import { GO_TIMEOUT, goTarget, isEditing, shortcutFor } from '../../lib/util/keys';
  import { currentRows, initMail, mail, move, moveTo, moveToFolder, openSettings, pullMail, select, selectedIds, setUnread, setView, toggleStar } from '../../lib/stores/mail.svelte';
  import { bulkMove, bulkMoveToFolder, bulkSetUnread, bulkSnooze, bulkStar, setTag, togglePin } from '../../lib/stores/bulk.svelte';
  import { checkAll, checkedIds, checkedRows, clearChecked, extendChecked, multi } from '../../lib/stores/selection.svelte';
  import { resolvedTheme, toggleTheme } from '../../lib/stores/settings.svelte';
  import { tagStore } from '../../lib/stores/tags.svelte';
  import { initUpdate } from '../../lib/stores/update.svelte';
  import { watchWake } from '../../lib/mail/wake';
  import BulkPanel from './BulkPanel.svelte';
  import ChordHint from './ChordHint.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import KeysDialog from './KeysDialog.svelte';
  import MessageList from './MessageList.svelte';
  import Onboarding from './Onboarding.svelte';
  import Picker, { type PickItem } from './Picker.svelte';
  import Rail from './Rail.svelte';
  import Reader from './Reader.svelte';
  import ScheduledView from './ScheduledView.svelte';
  import Settings from './settings/Settings.svelte';
  import Sidebar from './Sidebar.svelte';
  import Splitter from './Splitter.svelte';
  import Triage from './Triage.svelte';
  import {
    desk,
    deskMode,
    LIST,
    listWidth,
    RAIL_W,
    READER_MIN,
    setListWidth,
    setSideWidth,
    showList,
    showReader,
    SIDE,
    sideFull,
    toggleSidebar,
    watchWidth,
  } from './layout.svelte';
  import { applyStoredScale, percent, scale, setScale, stepped } from './scale.svelte';
  import { learned } from './teach.svelte';
  import { archiveCurrent, back, deleteCurrent, moveCurrent, skip, startTriage, stopTriage, triage } from './triage-state.svelte';

  let reader = $state<ReturnType<typeof Reader>>();
  let focusView = $state<ReturnType<typeof Triage>>();
  let keysOpen = $state(false);
  let picker = $state<'move' | 'tag' | null>(null);
  /** G'ye basıldı, ikinci tuş bekleniyor. */
  let chord = $state(false);
  let chordTimer: ReturnType<typeof setTimeout> | null = null;

  // Hesap ekleme sürerken (ilk hesap dahil) uygulamanın geri kalanı açılmaz:
  // kenar çubuğu, arama ve komut paleti "Gelen kutusunu aç" ile gelir.
  const setup = $derived(!mail.accounts.length || mail.screen === 'onboarding');
  // Birden çok satır seçiliyken okuma alanı yerine toplu işlem paneli.
  const bulk = $derived(multi());
  const mode = $derived(deskMode());
  const full = $derived(sideFull());
  const stack = $derived(mode === 'stack');
  const listW = $derived(listWidth());
  const listMax = $derived(Math.max(LIST.min, Math.min(LIST.max, desk.width - (full ? desk.sideW : RAIL_W) - READER_MIN)));

  onMount(() => {
    void initMail();
    initUpdate();
    applyStoredScale();
    const offWidth = watchWidth(() => scale.css);
    // Bildirimden açılan posta dar pencerede okuma alanında görünür.
    const offOpen = on(EVENTS.open, () => showReader());
    // Ağ geri gelince ölü bağlantılar yenilenir, pencereye dönülünce kutular
    // eşitlenir (uyku Rust tarafında yakalanır).
    const offWake = watchWake({ background: false });
    return () => {
      offWidth();
      offOpen();
      offWake();
    };
  });

  // Görev çubuğunda okunmamış sayısı.
  $effect(() => {
    const n = mail.counts.inboxUnread;
    document.title = n ? `(${n}) ${t('app.name')}` : t('app.name');
  });

  // Görünüm ya da arama değişince: yığın listeye döner, katman kapanır, odak kipi biter.
  $effect(() => {
    void mail.view;
    void mail.search;
    void mail.accountId;
    untrack(() => {
      desk.reading = false;
      desk.overlay = false;
      if (triage.on) stopTriage();
    });
  });

  // Yığında okunan posta kalmadıysa (son posta arşivlendi) liste görünür.
  $effect(() => {
    if (stack && desk.reading && mail.selectedId == null && !bulk) untrack(() => showList());
  });

  // Başka ekran (ayarlar, planlananlar) açılınca katman kapanır, odak kipi biter.
  $effect(() => {
    const screen = mail.screen;
    untrack(() => {
      desk.overlay = false;
      if (screen !== 'mail' && triage.on) stopTriage();
    });
  });

  // Katman açılınca odak ilk öğeye, kapanınca raydaki düğmeye döner.
  let overlayWas = false;
  $effect(() => {
    const open = desk.overlay;
    if (open === overlayWas) return;
    overlayWas = open;
    void tick().then(() => {
      if (open) document.querySelector<HTMLElement>('.side.overlay .nav')?.focus();
      else if (document.activeElement === document.body) document.querySelector<HTMLElement>('.rail .expand')?.focus();
    });
  });

  // --------------------------------------------------------------- seçiciler

  /** Seçicinin uygulanacağı iletiler: çoklu seçim ya da okunan konuşma. */
  function targetIds(): number[] {
    return bulk ? checkedIds() : selectedIds();
  }

  function openPicker(kind: 'move' | 'tag') {
    if (setup || mail.screen !== 'mail') return;
    if (!targetIds().length) {
      showToast({ text: t('picker.needMail') });
      return;
    }
    if (kind === 'move' && bulk && new Set(checkedRows().map((r) => r.accountId)).size > 1) {
      showToast({ text: t('bulk.mixedAccounts') });
      return;
    }
    picker = kind;
  }

  /** Taşıma hedefi hesabı ve postanın şu anki klasörü. */
  const moveFrom = $derived.by(() => {
    if (bulk) {
      const rows = checkedRows();
      return { accountId: rows[0]?.accountId ?? null, here: null as string | null };
    }
    const row = currentRows().find((r) => mail.selectedId != null && r.ids.includes(mail.selectedId));
    const accountId = mail.detail?.accountId ?? row?.accountId ?? null;
    return { accountId, here: mail.detail?.mailboxId ?? row?.mailboxId ?? null };
  });

  const folderItems = $derived.by((): PickItem[] => {
    const { accountId, here } = moveFrom;
    if (!accountId) return [];
    return flatten(folderTree(mail.mailboxes, accountId, true))
      .filter((n) => n.mailbox.id !== here && n.mailbox.role !== 'drafts')
      .map((n) => ({ key: n.mailbox.id, label: folderLabel(n.mailbox), depth: n.depth, icon: folderIcon(n.mailbox) }));
  });

  const tagItems = $derived.by((): PickItem[] => {
    const rows = bulk ? checkedRows() : currentRows().filter((r) => mail.selectedId != null && r.ids.includes(mail.selectedId));
    const has = (id: string) => (rows.length ? rows.every((r) => r.tags.includes(id)) : (mail.detail?.tags.includes(id) ?? false));
    return tagStore.list.map((tg) => ({ key: tg.id, label: tg.name, color: tg.color, checked: has(tg.id) }));
  });

  function pickFolder(it: PickItem) {
    const box = mail.mailboxes.find((b) => b.id === it.key) as Mailbox | undefined;
    if (!box) return;
    const label = folderLabel(box);
    if (triage.on) void moveCurrent(box, label);
    else if (bulk) void bulkMoveToFolder(checkedIds(), box, label);
    else void moveToFolder(selectedIds(), box, label);
  }

  function pickTag(it: PickItem) {
    const tag = tagStore.list.find((x) => x.id === it.key);
    if (tag) void setTag(targetIds(), tag, !it.checked);
  }

  // -------------------------------------------------------------- klavye

  function endChord() {
    chord = false;
    if (chordTimer) clearTimeout(chordTimer);
    chordTimer = null;
  }

  function zoom(sc: 'zoomIn' | 'zoomOut' | 'zoomReset') {
    const v = sc === 'zoomReset' ? 1 : stepped(scale.value, sc === 'zoomIn' ? 1 : -1);
    void setScale(v);
    showToast({ text: t('toast.scale', { n: percent(v) }) });
  }

  function paletteAction(what: 'keys' | 'move' | 'tag' | 'reply') {
    if (what === 'keys') keysOpen = true;
    else if (what === 'reply') {
      showReader();
      void tick().then(() => (triage.on ? focusView?.focusReply() : reader?.focusReply()));
    } else openPicker(what);
  }

  function onKey(e: KeyboardEvent) {
    if (e.defaultPrevented) return;
    // G zinciri: ikinci tuş görünümü seçer; başka tuş zinciri bitirir.
    if (chord) {
      if (e.key === 'Shift' || e.key === 'Control' || e.key === 'Alt' || e.key === 'Meta') return;
      endChord();
      const target = goTarget({ key: e.key, ctrlKey: e.ctrlKey, metaKey: e.metaKey, altKey: e.altKey });
      if (target || e.key === 'Escape') e.preventDefault();
      if (target) setView({ kind: target }, mail.accountId);
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === ',') {
      e.preventDefault();
      mail.screen = 'settings';
      return;
    }
    const editing = isEditing(e.target);
    const sc = shortcutFor({ key: e.key, code: e.code, ctrlKey: e.ctrlKey, metaKey: e.metaKey, altKey: e.altKey, shiftKey: e.shiftKey, editing });
    if (!sc) return;
    if (sc === 'zoomIn' || sc === 'zoomOut' || sc === 'zoomReset') {
      e.preventDefault();
      zoom(sc);
      return;
    }
    if (sc === 'sync') {
      e.preventDefault();
      if (!setup) void pullMail();
      return;
    }
    if (sc === 'palette' || sc === 'search') {
      e.preventDefault();
      if (setup) return;
      mail.paletteOpen = sc === 'search' ? true : !mail.paletteOpen;
      return;
    }
    if (mail.paletteOpen || keysOpen || picker) return;
    if (sc === 'escape') {
      // Sıra: katman, başka ekran, yazı alanı, odak kipi, çoklu seçim, yığında okuma.
      if (desk.overlay) desk.overlay = false;
      else if (mail.screen !== 'mail' && mail.accounts.length) mail.screen = 'mail';
      else if (editing) (document.activeElement as HTMLElement | null)?.blur();
      else if (triage.on) stopTriage();
      else if (mail.checked.length) clearChecked();
      else if (stack && desk.reading) showList();
      else (document.activeElement as HTMLElement | null)?.blur();
      return;
    }
    if (setup) return;
    if (sc === 'help') {
      e.preventDefault();
      keysOpen = true;
      return;
    }
    if (sc === 'sidebar') {
      e.preventDefault();
      // Odak kipinde kenar çubuğu gizli; tercih görünmeden değişmesin.
      if (!triage.on) toggleSidebar();
      return;
    }
    if (mail.screen !== 'mail') return;
    if (sc === 'go') {
      e.preventDefault();
      chord = true;
      chordTimer = setTimeout(endChord, GO_TIMEOUT);
      return;
    }
    const id = mail.selectedId;
    e.preventDefault();
    learned(sc);
    if (sc === 'triage') {
      if (triage.on) stopTriage();
      else startTriage();
      return;
    }
    if (sc === 'movePicker' || sc === 'tagPicker') {
      openPicker(sc === 'movePicker' ? 'move' : 'tag');
      return;
    }
    if (sc === 'undo') {
      void runToastAction();
      return;
    }
    if (sc === 'compose') {
      void call('open_compose', { prefill: { mode: 'new', messageId: null } });
      return;
    }
    if (triage.on) {
      // Odak kipinde: listeden çıkaran eylemler sıradakine geçer; J atlar, K geri döner.
      if (sc === 'archive') void archiveCurrent();
      else if (sc === 'delete') void deleteCurrent();
      else if (sc === 'snooze') focusView?.openSnooze();
      else if (sc === 'next') skip();
      else if (sc === 'prev') back();
      else if (sc === 'reply') focusView?.focusReply();
      else if (id == null) return;
      else if (sc === 'star') void toggleStar(id);
      else if (sc === 'unread') void setUnread(selectedIds(), true);
      else if (sc === 'pin') void togglePin(selectedIds());
      else if (sc === 'replyAll' || sc === 'forward') void call('open_compose', { prefill: { mode: sc, messageId: id } });
      return;
    }
    switch (sc) {
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
        else if (sc === 'open') {
          void select(id);
          showReader();
        } else if (sc === 'reply' || sc === 'snooze') {
          // Yığında liste görünürken okuma alanı önce açılır.
          showReader();
          void tick().then(() => (sc === 'reply' ? reader?.focusReply() : reader?.openSnooze()));
        } else if (sc === 'replyAll' || sc === 'forward') void call('open_compose', { prefill: { mode: sc, messageId: id } });
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
      <WindowControls />
    </div>
  </header>

  <div class="body">
    {#if !setup && !triage.on}
      {#if full}
        <Sidebar width={desk.sideW} />
        <Splitter label={t('desk.resizeSide')} value={desk.sideW} min={SIDE.min} max={SIDE.max} def={SIDE.def} onchange={setSideWidth} />
      {:else}
        <Rail />
        <span class="vline" aria-hidden="true"></span>
        {#if desk.overlay}
          <div class="veil" role="presentation" onmousedown={() => (desk.overlay = false)}></div>
          <Sidebar width={desk.sideW} overlay />
        {/if}
      {/if}
    {/if}
    {#if setup}
      <!-- Tek örnek: hesap eklenince de aynı bileşen "Hazırsın" adımında kalır. -->
      {#if mail.ready}<Onboarding />{/if}
    {:else if mail.screen === 'settings'}
      <Settings />
    {:else if mail.screen === 'scheduled'}
      <ScheduledView />
    {:else if triage.on}
      <Triage bind:this={focusView} onpick={openPicker} />
    {:else}
      {#if !stack || !desk.reading}
        <MessageList width={stack ? null : listW} />
      {/if}
      {#if !stack}
        <Splitter label={t('desk.resizeList')} value={listW} min={LIST.min} max={listMax} def={LIST.def} onchange={setListWidth} />
      {:else if bulk && !desk.reading}
        <span class="vline" aria-hidden="true"></span>
      {/if}
      {#if bulk && (!stack || !desk.reading)}
        <BulkPanel />
      {:else if !stack || desk.reading}
        <Reader bind:this={reader} onback={stack ? showList : undefined} />
      {/if}
    {/if}
  </div>

  {#if mail.paletteOpen}
    <CommandPalette onclose={() => (mail.paletteOpen = false)} onopen={paletteAction} />
  {/if}
  {#if keysOpen}<KeysDialog onclose={() => (keysOpen = false)} />{/if}
  {#if picker === 'move'}
    <Picker
      title={t('picker.move')}
      placeholder={t('picker.movePlaceholder')}
      items={folderItems}
      empty={{ title: t('picker.move'), body: t('picker.noFolders') }}
      onpick={pickFolder}
      onclose={() => (picker = null)}
    />
  {:else if picker === 'tag'}
    <Picker
      title={t('picker.tag')}
      placeholder={t('picker.tagPlaceholder')}
      items={tagItems}
      empty={{ title: t('picker.noTagsTitle'), body: t('picker.noTagsBody'), action: { label: t('picker.createTag'), run: () => openSettings('tags') } }}
      onpick={pickTag}
      onclose={() => (picker = null)}
    />
  {/if}
  {#if chord}<ChordHint />{/if}
  <ToastHost />
</div>

<style>
  /* Tarayıcıdaki kök ölçekte (--ui-zoom) yükseklik ölçeğe bölünür; Tauri'de 1. */
  .shell {
    height: calc(100vh / var(--ui-zoom, 1));
    min-height: 560px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .titlebar {
    flex: none;
    height: 46px;
    display: grid;
    grid-template-columns: minmax(max-content, 1fr) minmax(200px, 560px) minmax(max-content, 1fr);
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
    gap: 10px;
    align-self: stretch;
  }

  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }

  /* Ray ile içerik arasında ince çizgi (ayırıcı yoksa). */
  .vline {
    flex: none;
    width: 1px;
    background: var(--line);
  }

  /* Katman açıkken arkası sakin bir örtüyle geri çekilir (bulanıklık yok). */
  .veil {
    position: absolute;
    inset: 0;
    z-index: 24;
    background: color-mix(in srgb, var(--ground) 40%, transparent);
    animation: veil-in var(--t-state) var(--ease);
  }

  @keyframes veil-in {
    from {
      opacity: 0;
    }
  }

  @media (max-width: 720px) {
    .name {
      display: none;
    }
  }

  @media (max-width: 560px) {
    .center {
      display: none;
    }
  }
</style>
