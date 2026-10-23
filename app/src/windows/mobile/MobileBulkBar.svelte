<script lang="ts">
  // Seçim kipinde alt eylem çubuğu (56 px + güvenli alan): arşivle, sil,
  // İstenmeyen ve "Diğer". Diğer, yukarı açılan menüde okundu/okunmadı,
  // yıldız, ertele, klasöre taşı ve etiketi toplar (çubuk sıkışmasın). Mantık
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
  import { showToast } from '../../lib/ui/toast.svelte';
  import { junkOf } from './folders';
  import { haptic } from './haptic';

  const rows = $derived(checkedRows());
  const ids = $derived(checkedIds());
  const allRead = $derived(rows.every((r) => !r.unread));
  const allStarred = $derived(rows.every((r) => r.starred));
  const accounts = $derived([...new Set(rows.map((r) => r.accountId))]);
  const oneAccount = $derived(accounts.length === 1 ? accounts[0] : null);
  const junk = $derived(oneAccount ? junkOf(oneAccount) : undefined);

  let menu = $state<'more' | 'snooze' | 'move' | 'tag' | null>(null);

  /** İstenmeyen klasörüne taşır; klasör yoksa ya da seçim birden çok hesaptaysa nedenini söyler. */
  function spam() {
    if (junk) {
      void bulkMoveToFolder(ids, junk, folderLabel(junk));
      return;
    }
    haptic('reject');
    showToast({ text: oneAccount ? t('mob.swipe.noJunk') : t('bulk.mixedAccounts') });
  }

  const moreItems = $derived.by((): MenuItem[] => [
    {
      key: 'read',
      label: allRead ? t('mob.unread') : t('mob.read'),
      icon: 'mail',
      onclick: () => void bulkSetUnread(ids, allRead),
    },
    {
      key: 'star',
      label: allStarred ? t('mob.unstar') : t('mob.star'),
      icon: 'star',
      onclick: () => void bulkStar(ids, !allStarred),
    },
    { key: 'snooze', label: t('act.snooze'), icon: 'clock', onclick: () => (menu = 'snooze') },
    {
      key: 'move',
      label: t('mob.move'),
      icon: 'folder',
      disabled: !oneAccount,
      hint: oneAccount ? undefined : t('bulk.mixedAccounts'),
      onclick: () => (menu = 'move'),
    },
    { key: 'tag', label: t('mob.tag'), icon: 'tag', onclick: () => (menu = 'tag') },
  ]);

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

  /** Menü öğesi alt menü açınca (ertele, taşı, etiket) Menu önce kapanır; alt menü bir sonraki adımda açılır. */
  function close() {
    if (menu === 'more') menu = null;
  }
</script>

<div class="bar" role="toolbar" aria-label={t('bulk.count', { n: rows.length })}>
  <button class="act" onclick={() => bulkMove(ids, 'archive')}>
    <Icon name="archive" size={22} /><span>{t('act.archive')}</span>
  </button>
  <button class="act" onclick={() => bulkMove(ids, 'trash')}>
    <Icon name="trash" size={22} /><span>{t('act.delete')}</span>
  </button>
  <button class="act" onclick={spam}>
    <Icon name="spam" size={22} /><span>{t('mob.swipe.spam')}</span>
  </button>
  <div class="anchor">
    <button
      class="act"
      class:on={menu !== null}
      aria-haspopup="menu"
      aria-expanded={menu !== null}
      onclick={() => (menu = menu === null ? 'more' : null)}
    >
      <Icon name="more" size={22} /><span>{t('mob.more')}</span>
    </button>
    {#if menu === 'more'}
      <Menu style="bottom: calc(100% + 6px); right: 0; top: auto" items={moreItems} onclose={close} />
    {:else if menu === 'snooze'}
      <SnoozeMenu
        style="bottom: calc(100% + 6px); right: 0; left: auto; top: auto"
        onclose={() => (menu = null)}
        onpick={(at) => {
          menu = null;
          void bulkSnooze(ids, at);
        }}
      />
    {:else if menu === 'move'}
      <Menu style="bottom: calc(100% + 6px); right: 0; top: auto" title={t('read.moveTo')} items={moveItems} onclose={() => (menu = null)} />
    {:else if menu === 'tag'}
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
    opacity: var(--dim-disabled);
    background: transparent;
  }

  .act > span:last-child {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
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
