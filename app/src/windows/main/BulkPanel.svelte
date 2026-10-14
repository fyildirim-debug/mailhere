<script lang="ts">
  // Birden çok satır seçiliyken okuma alanının yerine gelen toplu işlem paneli.
  // Her eylem seçili satırların tüm iletilerine uygulanır ve geri alınabilir.
  import { t } from '../../lib/i18n/index.svelte';
  import { flatten, folderLabel, folderTree } from '../../lib/mail/folders';
  import SnoozeMenu from '../../lib/mail/SnoozeMenu.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { bulkMove, bulkMoveToFolder, bulkSetUnread, bulkSnooze, bulkStar, setPinned, setTag } from '../../lib/stores/bulk.svelte';
  import { mail, openSettings } from '../../lib/stores/mail.svelte';
  import { checkedIds, checkedRows, clearChecked } from '../../lib/stores/selection.svelte';
  import { tagStore } from '../../lib/stores/tags.svelte';

  const rows = $derived(checkedRows());
  const ids = $derived(checkedIds());
  const n = $derived(rows.length);
  const allRead = $derived(rows.every((r) => !r.unread));
  const allStarred = $derived(rows.every((r) => r.starred));
  const allPinned = $derived(rows.every((r) => r.pinned));
  const accounts = $derived([...new Set(rows.map((r) => r.accountId))]);
  const oneAccount = $derived(accounts.length === 1 ? accounts[0] : null);

  let snoozeOpen = $state(false);
  let menu = $state<{ at: { x: number; y: number }; title?: string; items: MenuItem[] } | null>(null);

  function anchorOf(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left, y: r.bottom + 4 };
  }

  function folderMenu(e: MouseEvent) {
    if (!oneAccount) return;
    const nodes = flatten(folderTree(mail.mailboxes, oneAccount, true)).filter((x) => x.mailbox.role !== 'drafts');
    menu = {
      at: anchorOf(e),
      title: t('read.moveTo'),
      items: nodes.map((x) => ({
        label: `${'  '.repeat(x.depth)}${folderLabel(x.mailbox)}`,
        icon: 'folder' as const,
        onclick: () => void bulkMoveToFolder(ids, x.mailbox, folderLabel(x.mailbox)),
      })),
    };
  }

  function tagMenu(e: MouseEvent) {
    const items: MenuItem[] = tagStore.list.map((tg) => {
      const all = rows.every((r) => r.tags.includes(tg.id));
      return { label: tg.name, checked: all, onclick: () => void setTag(ids, tg, !all) };
    });
    items.push({ label: t('tag.manage'), icon: 'settings', onclick: () => openSettings('tags') });
    menu = { at: anchorOf(e), title: t('tag.menu'), items };
  }
</script>

<main class="reader" aria-label={t('bulk.count', { n })}>
  <div class="wrap">
    <div class="head">
      <span class="mark"><Icon name="check" size={20} /></span>
      <div class="text">
        <h2>{t('bulk.count', { n })}</h2>
        <p class="meta">{t('bulk.hint')}</p>
      </div>
      <button class="btn-ghost" onclick={clearChecked}>{t('bulk.clear')} · Esc</button>
    </div>

    <div class="grid">
      <button class="act" onclick={() => bulkMove(ids, 'archive')}>
        <Icon name="archive" /><span class="grow">{t('act.archive')}</span><span class="kbd">E</span>
      </button>
      <button class="act" onclick={() => bulkMove(ids, 'trash')}>
        <Icon name="trash" /><span class="grow">{t('act.delete')}</span><span class="kbd">Del</span>
      </button>
      <button class="act" onclick={() => bulkSetUnread(ids, allRead)}>
        <Icon name="mail" /><span class="grow">{allRead ? t('act.markUnread') : t('act.markRead')}</span>{#if allRead}<span class="kbd">U</span>{/if}
      </button>
      <button class="act" onclick={() => bulkStar(ids, !allStarred)}>
        <span class:star={!allStarred}><Icon name="star" filled={!allStarred} /></span><span class="grow">{allStarred ? t('act.unstar') : t('act.star')}</span><span class="kbd">S</span>
      </button>
      <div class="anchor">
        <button class="act" class:on={snoozeOpen} aria-haspopup="menu" aria-expanded={snoozeOpen} onclick={() => (snoozeOpen = !snoozeOpen)}>
          <Icon name="clock" /><span class="grow">{t('act.snooze')}</span><span class="kbd">H</span>
        </button>
        {#if snoozeOpen}
          <SnoozeMenu
            style="top: 50px; left: 0"
            onclose={() => (snoozeOpen = false)}
            onpick={(at) => {
              snoozeOpen = false;
              void bulkSnooze(ids, at);
            }}
          />
        {/if}
      </div>
      <button class="act" aria-haspopup="menu" disabled={!oneAccount} title={oneAccount ? undefined : t('bulk.mixedAccounts')} onclick={folderMenu}>
        <Icon name="folder" /><span class="grow">{t('read.moveTo')}</span>
      </button>
      <button class="act" aria-haspopup="menu" onclick={tagMenu}>
        <Icon name="tag" /><span class="grow">{t('tag.menu')}</span>
      </button>
      <button class="act" onclick={() => setPinned(ids, !allPinned)}>
        <Icon name="pin" /><span class="grow">{allPinned ? t('pin.unpin') : t('pin.pin')}</span><span class="kbd">P</span>
      </button>
    </div>
    {#if !oneAccount}<p class="meta note">{t('bulk.mixedAccounts')}</p>{/if}

    <ul class="picked" aria-label={t('bulk.count', { n })}>
      {#each rows as r (r.id)}
        <li class="trunc"><span class="who">{r.from.name ?? r.from.email}</span><span class="subj trunc">{r.subject || '—'}</span></li>
      {/each}
    </ul>
  </div>
</main>

{#if menu}<Menu at={menu.at} title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}

<style>
  .reader {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    background: var(--paper);
  }

  .wrap {
    max-width: 640px;
    width: 100%;
    margin: 0 auto;
    padding: 36px 40px 32px;
    animation: fade-up var(--t-state) ease-out;
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: 14px;
  }

  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-ink);
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  h2 {
    font-size: var(--fs-title);
    line-height: 1.25;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .head .btn-ghost {
    margin-top: 6px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
    margin-top: 26px;
  }

  .anchor {
    position: relative;
  }

  .act {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 44px;
    padding: 0 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-row);
    background: var(--paper);
    color: var(--ink);
    font-weight: 500;
    text-align: left;
    transition: background var(--t-fast);
  }

  .act:hover,
  .act.on {
    background: var(--hover);
  }

  .act:disabled {
    opacity: 0.5;
    background: var(--paper);
  }

  .act :global(.icon) {
    color: var(--ink-2);
  }

  .grow {
    flex: 1;
  }

  .star {
    display: inline-flex;
    color: var(--amber);
  }

  .note {
    margin-top: 10px;
  }

  .picked {
    margin: 28px 0 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--line);
  }

  .picked li {
    display: flex;
    gap: 10px;
    padding: 9px 2px;
    border-bottom: 1px solid var(--line);
  }

  .who {
    flex: none;
    max-width: 40%;
    font-weight: 500;
  }

  .subj {
    flex: 1;
    color: var(--ink-2);
  }
</style>
