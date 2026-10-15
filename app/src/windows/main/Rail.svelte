<script lang="ts">
  // Dar pencerede kenar çubuğunun yerine 56 px ikon rayı: Yeni posta, kenar
  // çubuğunu açma (Ctrl B), görünümler (12 px sayaçla) ve altta Ayarlar.
  // Görünüm adı ve sayısı ipucunda (title) ve erişilebilir adda.
  import { call } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import { scheduledVisible, VIEW_ITEMS, viewActive, viewCount, viewVisible } from '../../lib/mail/nav';
  import Icon from '../../lib/ui/Icon.svelte';
  import { mail, openSettings, setView, syncLabel } from '../../lib/stores/mail.svelte';
  import { desk, toggleSidebar } from './layout.svelte';

  const scope = $derived(mail.accountId);
  const sync = $derived(syncLabel());

  function named(label: string, n: number) {
    return n > 0 ? t('desk.countTitle', { name: label, n }) : label;
  }

  function short(n: number) {
    return n > 99 ? '99+' : String(n);
  }
</script>

<nav class="rail" aria-label={t('desk.rail')}>
  <button
    class="compose"
    aria-label={t('act.compose')}
    title="{t('act.compose')} · C"
    onclick={() => call('open_compose', { prefill: { mode: 'new', messageId: null } })}
  >
    <Icon name="pen" />
  </button>
  <button
    class="item expand"
    aria-label={t('desk.expand')}
    aria-expanded={desk.overlay}
    title="{t('desk.expand')} · Ctrl B"
    onclick={toggleSidebar}
  >
    <Icon name="sidebar" />
  </button>

  <div class="views">
    {#each VIEW_ITEMS as item (item.kind)}
      {#if viewVisible(item, mail)}
        {@const active = viewActive(item.kind, mail)}
        {@const n = viewCount(item, mail.counts, scope)}
        {@const label = t(item.label)}
        <button
          class="item"
          class:active
          aria-current={active ? 'page' : undefined}
          aria-label={named(label, n)}
          title={named(label, n)}
          onclick={() => setView({ kind: item.kind }, scope)}
        >
          <Icon name={item.icon} />
          {#if n > 0}<span class="count num" aria-hidden="true">{short(n)}</span>{/if}
        </button>
      {/if}
      {#if item.kind === 'drafts' && scheduledVisible(mail)}
        {@const on = mail.screen === 'scheduled'}
        {@const label = t('nav.scheduled')}
        <button
          class="item"
          class:active={on}
          aria-current={on ? 'page' : undefined}
          aria-label={named(label, mail.counts.scheduled)}
          title={named(label, mail.counts.scheduled)}
          onclick={() => (mail.screen = 'scheduled')}
        >
          <Icon name="calendarClock" />
          {#if mail.counts.scheduled > 0}<span class="count num" aria-hidden="true">{short(mail.counts.scheduled)}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>

  <span class="state {sync.tone}" title={sync.text} role="img" aria-label={sync.text}></span>
  <button
    class="item"
    class:active={mail.screen === 'settings'}
    aria-label={t('act.settings')}
    title="{t('act.settings')} · Ctrl ,"
    onclick={() => (mail.screen === 'settings' ? (mail.screen = 'mail') : openSettings())}
  >
    <Icon name="settings" />
  </button>
</nav>

<style>
  .rail {
    flex: none;
    width: 56px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 12px 0 10px;
    background: var(--ground);
    overflow-y: auto;
    scrollbar-width: none;
  }

  .rail::-webkit-scrollbar {
    display: none;
  }

  /* Ekrandaki tek birincil düğme. */
  .compose {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    margin-bottom: 6px;
    border: 0;
    border-radius: var(--r-btn);
    background: var(--accent);
    color: var(--accent-ink);
    transition: filter var(--t-fast);
  }

  .compose:hover {
    filter: brightness(1.07);
  }

  .views {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    margin-top: 8px;
  }

  .item {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 38px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink-2);
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .item:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .item.active {
    background: var(--paper);
    color: var(--ink);
    box-shadow: var(--seg-on-shadow);
  }

  :global(:root[data-theme='dark']) .item.active {
    background: var(--raised);
    box-shadow: none;
  }

  /* 12 px sayaç sağ üst köşede; zemin düğmeninkini alır, ikon çizgisini örter. */
  .count {
    position: absolute;
    top: 1px;
    right: 0;
    min-width: 16px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--ground);
    color: var(--ink-2);
    font-size: var(--fs-meta);
    font-weight: 600;
    line-height: 15px;
    text-align: center;
  }

  .item:hover .count {
    color: var(--ink);
  }

  .item.active .count {
    background: var(--paper);
    color: var(--ink);
  }

  :global(:root[data-theme='dark']) .item.active .count {
    background: var(--raised);
  }

  .state {
    flex: none;
    width: 7px;
    height: 7px;
    margin: auto 0 10px;
    border-radius: 50%;
    background: var(--ok);
  }

  .state.busy {
    background: var(--accent);
  }

  .state.warn {
    background: var(--amber);
  }
</style>
