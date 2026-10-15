<script lang="ts">
  // Odak (tek tek işleme) kipi: okuma alanı tam genişlikte, sağda kısayol
  // çipli dikey eylem listesi, üstte ilerleme. Eylemden sonra sıradaki posta
  // 120 ms saydamlık geçişiyle gelir; hepsi bitince boş durum.
  import { t } from '../../lib/i18n/index.svelte';
  import SnoozeMenu from '../../lib/mail/SnoozeMenu.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import { KEY_LABEL, type Shortcut } from '../../lib/util/keys';
  import { mail, selectedIds, setUnread, setView, toggleStar } from '../../lib/stores/mail.svelte';
  import Reader from './Reader.svelte';
  import { teach, type Teachable } from './teach.svelte';
  import { archiveCurrent, back, deleteCurrent, finished, progress, skip, snoozeCurrent, stopTriage } from './triage-state.svelte';

  interface Props {
    /** V / L seçicilerini açar (ana pencere yönetir). */
    onpick: (kind: 'move' | 'tag') => void;
  }

  let { onpick }: Props = $props();

  let reader = $state<ReturnType<typeof Reader>>();
  let snoozeOpen = $state(false);
  let snoozeFromMouse = false;

  const done = $derived(finished());
  const p = $derived(progress());
  const inbox = $derived(mail.view.kind === 'inbox' && mail.search == null);

  export function focusReply() {
    reader?.focusReply();
  }

  export function openSnooze() {
    snoozeFromMouse = false;
    snoozeOpen = true;
  }

  interface Act {
    key: Shortcut | 'skip';
    label: string;
    icon: IconName;
    kbd: string;
    teach?: Teachable;
    run: (e: MouseEvent) => void;
    primary?: boolean;
  }

  const starred = $derived(mail.detail?.starred ?? false);

  const acts = $derived<Act[]>([
    {
      key: 'archive',
      label: t('act.archive'),
      icon: 'archive',
      kbd: KEY_LABEL.archive!,
      primary: true,
      run: (e) => void teach('archive', e, archiveCurrent()),
    },
    {
      key: 'snooze',
      label: t('act.snooze'),
      icon: 'clock',
      kbd: KEY_LABEL.snooze!,
      run: (e) => {
        snoozeFromMouse = e.detail > 0;
        snoozeOpen = !snoozeOpen;
      },
    },
    {
      key: 'reply',
      label: t('act.reply'),
      icon: 'reply',
      kbd: KEY_LABEL.reply!,
      run: (e) => {
        focusReply();
        void teach('reply', e);
      },
    },
    { key: 'delete', label: t('act.delete'), icon: 'trash', kbd: KEY_LABEL.delete!, run: (e) => void teach('delete', e, deleteCurrent()) },
    {
      key: 'star',
      label: starred ? t('act.unstar') : t('act.star'),
      icon: 'star',
      kbd: KEY_LABEL.star!,
      run: (e) => {
        if (mail.selectedId != null) void toggleStar(mail.selectedId);
        void teach('star', e);
      },
    },
    { key: 'movePicker', label: t('read.moveTo'), icon: 'folder', kbd: KEY_LABEL.movePicker!, run: () => onpick('move') },
    { key: 'tagPicker', label: t('picker.tag'), icon: 'tag', kbd: KEY_LABEL.tagPicker!, run: () => onpick('tag') },
    {
      key: 'unread',
      label: t('act.markUnread'),
      icon: 'mail',
      kbd: KEY_LABEL.unread!,
      run: (e) => {
        void setUnread(selectedIds(), true);
        void teach('unread', e);
      },
    },
    { key: 'skip', label: t('triage.skip'), icon: 'arrowDown', kbd: KEY_LABEL.next!, run: () => skip() },
    { key: 'prev', label: t('triage.back'), icon: 'arrowUp', kbd: KEY_LABEL.prev!, run: () => back() },
  ]);
</script>

<section class="triage" aria-label={t('triage.label')}>
  <header class="bar">
    <span class="name">{t('triage.title')}</span>
    <span class="progress num" role="status" aria-label={t('triage.progressLabel', { n: p.n, total: p.total })}>{p.n}/{p.total}</span>
    <span class="grow"></span>
    <button class="btn-ghost exit" onclick={stopTriage}>{t('triage.exit')}<span class="kbd">Esc</span></button>
  </header>

  {#if done}
    <div class="done">
      <span class="empty-icon"><Icon name="check" size={26} /></span>
      <h2 class="empty-title">{t(inbox ? 'triage.doneTitleInbox' : 'triage.doneTitle')}</h2>
      <p class="empty-body">{p.total ? t('triage.doneBody', { n: p.total }) : t('triage.noneBody')}</p>
      <div class="done-acts">
        <button class="btn-primary" onclick={stopTriage}>{t('triage.exit')}</button>
        <button
          class="btn-soft"
          onclick={() => {
            stopTriage();
            setView({ kind: 'snoozed' }, mail.accountId);
          }}>{t('triage.openSnoozed')}</button
        >
      </div>
    </div>
  {:else}
    <div class="main">
      <div class="read">
        <Reader bind:this={reader} focus />
      </div>
      <aside class="acts" aria-label={t('triage.actions')}>
        <p class="acts-title">{t('triage.actions')}</p>
        {#each acts as a (a.key)}
          {#if a.key === 'snooze'}
            <div class="anchor">
              <button class="act" class:on={snoozeOpen} aria-haspopup="menu" aria-expanded={snoozeOpen} onclick={a.run}>
                <Icon name={a.icon} size={16} /><span class="grow trunc">{a.label}</span><span class="kbd">{a.kbd}</span>
              </button>
              {#if snoozeOpen}
                <SnoozeMenu
                  style="top: 42px; right: 0"
                  onclose={() => (snoozeOpen = false)}
                  onpick={(at) => {
                    snoozeOpen = false;
                    const run = snoozeCurrent(at);
                    if (snoozeFromMouse) void teach('snooze', null, run);
                  }}
                />
              {/if}
            </div>
          {:else}
            <button class="act" class:primary={a.primary} onclick={a.run}>
              <Icon name={a.icon} size={16} /><span class="grow trunc">{a.label}</span><span class="kbd">{a.kbd}</span>
            </button>
          {/if}
          {#if a.key === 'tagPicker' || a.key === 'unread'}<span class="gap"></span>{/if}
        {/each}
      </aside>
    </div>
  {/if}
</section>

<style>
  .triage {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--paper);
  }

  .bar {
    flex: none;
    height: 52px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px 0 20px;
    border-bottom: 1px solid var(--line);
  }

  .name {
    font-weight: 600;
  }

  .progress {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-3);
  }

  .grow {
    flex: 1;
  }

  .exit {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 6px 0 10px;
  }

  .main {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .read {
    flex: 1;
    min-width: 0;
    display: flex;
  }

  .acts {
    flex: none;
    width: 232px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 20px 16px;
    border-left: 1px solid var(--line);
    background: var(--surface);
    overflow-y: auto;
  }

  .acts-title {
    margin: 0 0 8px 10px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  .anchor {
    position: relative;
  }

  .act {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 38px;
    padding: 0 8px 0 10px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink);
    text-align: left;
    transition: background var(--t-fast);
  }

  .act :global(svg) {
    color: var(--ink-2);
  }

  .act:hover,
  .act.on {
    background: var(--hover);
  }

  /* Tek birincil eylem: arşivle. */
  .act.primary {
    margin-bottom: 4px;
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 500;
  }

  .act.primary :global(svg) {
    color: var(--accent-ink);
  }

  .act.primary:hover {
    filter: brightness(1.07);
  }

  .act.primary .kbd {
    border-color: color-mix(in srgb, var(--accent-ink) 40%, transparent);
    background: transparent;
    color: var(--accent-ink);
  }

  .gap {
    height: 10px;
  }

  .done {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 40px;
    text-align: center;
    animation: fade-up var(--t-state) var(--ease);
  }

  .empty-icon {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    margin-bottom: 10px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .empty-title {
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
    line-height: 1.3;
  }

  .empty-body {
    max-width: 40ch;
    color: var(--ink-3);
  }

  .done-acts {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
</style>
