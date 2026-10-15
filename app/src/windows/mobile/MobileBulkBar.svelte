<script lang="ts">
  // Seçim kipinde alt eylem çubuğu (56 px + güvenli alan): arşivle, sil,
  // okundu/okunmadı, yıldız, ertele, klasöre taşı, etiket. Mantık
  // bulk.svelte.ts / selection.svelte.ts ile ortak; her eylem geri alınabilir.
  import { t } from '../../lib/i18n/index.svelte';
  import { flatten, folderLabel, folderTree } from '../../lib/mail/folders';
  import SnoozeMenu from '../../lib/mail/SnoozeMenu.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { bulkMove, bulkMoveToFolder, bulkSetUnread, bulkSnooze, bulkStar, setTag } from '../../lib/stores/bulk.svelte';
  import { mail } from '../../lib/stores/mail.svelte';
  import { checkedIds, checkedRows } from '../../lib/stores/selection.svelte';
  import { tagStore } from '../../lib/stores/tags.svelte';

  const rows = $derived(checkedRows());
  const ids = $derived(checkedIds());
  const allRead = $derived(rows.every((r) => !r.unread));
  const allStarred = $derived(rows.every((r) => r.starred));
  const accounts = $derived([...new Set(rows.map((r) => r.accountId))]);
  const oneAccount = $derived(accounts.length === 1 ? accounts[0] : null);

  let menu = $state<'snooze' | 'move' | 'tag' | null>(null);

  const moveItems = $derived.by((): MenuItem[] => {
    if (!oneAccount) return [];
    return flatten(folderTree(mail.mailboxes, oneAccount, true))
      .filter((x) => x.mailbox.role !== 'drafts')
      .map((x) => ({
        key: x.mailbox.id,
        label: `${'  '.repeat(x.depth)}${folderLabel(x.mailbox)}`,
        icon: 'folder' as const,
        onclick: () => void bulkMoveToFolder(ids, x.mailbox, folderLabel(x.mailbox)),
      }));
  });

  const tagItems = $derived.by((): MenuItem[] =>
    tagStore.list.map((tg) => {
      const all = rows.every((r) => r.tags.includes(tg.id));
      return { key: tg.id, label: tg.name, checked: all, onclick: () => void setTag(ids, tg, !all) };
    }),
  );
</script>

<div class="bar" role="toolbar" aria-label={t('bulk.count', { n: rows.length })}>
  <button class="act" onclick={() => bulkMove(ids, 'archive')}>
    <Icon name="archive" size={22} /><span>{t('act.archive')}</span>
  </button>
  <button class="act" onclick={() => bulkMove(ids, 'trash')}>
    <Icon name="trash" size={22} /><span>{t('act.delete')}</span>
  </button>
  <button class="act" onclick={() => bulkSetUnread(ids, allRead)}>
    <Icon name="mail" size={22} /><span>{allRead ? t('mob.unread') : t('mob.read')}</span>
  </button>
  <button class="act" onclick={() => bulkStar(ids, !allStarred)}>
    <span class:star={!allStarred}><Icon name="star" size={22} filled={!allStarred} /></span><span>{allStarred ? t('mob.unstar') : t('mob.star')}</span>
  </button>
  <div class="anchor">
    <button class="act" class:on={menu === 'snooze'} aria-haspopup="menu" aria-expanded={menu === 'snooze'} onclick={() => (menu = menu === 'snooze' ? null : 'snooze')}>
      <Icon name="clock" size={22} /><span>{t('act.snooze')}</span>
    </button>
    {#if menu === 'snooze'}
      <SnoozeMenu
        style="bottom: calc(100% + 6px); left: 0; top: auto"
        onclose={() => (menu = null)}
        onpick={(at) => {
          menu = null;
          void bulkSnooze(ids, at);
        }}
      />
    {/if}
  </div>
  <div class="anchor">
    <button
      class="act"
      class:on={menu === 'move'}
      aria-haspopup="menu"
      aria-expanded={menu === 'move'}
      disabled={!oneAccount}
      title={oneAccount ? undefined : t('bulk.mixedAccounts')}
      onclick={() => (menu = menu === 'move' ? null : 'move')}
    >
      <Icon name="folder" size={22} /><span>{t('mob.move')}</span>
    </button>
    {#if menu === 'move'}
      <Menu style="bottom: calc(100% + 6px); right: 0; top: auto" title={t('read.moveTo')} items={moveItems} onclose={() => (menu = null)} />
    {/if}
  </div>
  <div class="anchor">
    <button class="act" class:on={menu === 'tag'} aria-haspopup="menu" aria-expanded={menu === 'tag'} onclick={() => (menu = menu === 'tag' ? null : 'tag')}>
      <Icon name="tag" size={22} /><span>{t('mob.tag')}</span>
    </button>
    {#if menu === 'tag'}
      <Menu style="bottom: calc(100% + 6px); right: 0; top: auto" title={t('tag.menu')} items={tagItems} onclose={() => (menu = null)} />
    {/if}
  </div>
</div>

<style>
  .bar {
    position: relative;
    z-index: 6;
    flex: none;
    display: flex;
    align-items: stretch;
    min-height: calc(var(--m-bar) + var(--sab));
    padding: 0 calc(4px + var(--sar)) var(--sab) calc(4px + var(--sal));
    background: var(--raised);
    border-top: 1px solid var(--line);
    box-shadow: var(--shadow-sm);
    animation: bar-in var(--t-state) var(--ease);
  }

  .anchor {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
  }

  .act {
    flex: 1;
    min-width: var(--m-touch);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    padding: 4px 0;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink-2);
    font-size: var(--fs-meta);
    font-weight: 500;
    white-space: nowrap;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  .act:active,
  .act.on {
    background: var(--hover);
    color: var(--ink);
  }

  .act:disabled {
    opacity: 0.45;
    background: transparent;
  }

  .act > span:last-child {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .star {
    display: inline-flex;
    color: var(--amber);
  }

  .anchor :global(.popover) {
    max-height: min(60vh, 440px);
    overflow-y: auto;
  }

  @keyframes bar-in {
    from {
      transform: translateY(100%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .bar {
      animation: none;
    }
  }
</style>
