<script lang="ts">
  import { call, toAppError, type Account, type Mailbox, type SpecialRole } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import { flatten, folderIcon, folderLabel, folderTree } from '../../../lib/mail/folders';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { mail, reloadMailboxes } from '../../../lib/stores/mail.svelte';

  interface Props {
    account: Account;
  }

  let { account }: Props = $props();

  const rows = $derived(flatten(folderTree(mail.mailboxes, account.id, true)));
  const own = $derived(mail.mailboxes.filter((b) => b.accountId === account.id));

  const roles: { role: SpecialRole; label: MessageKey; hint: MessageKey }[] = [
    { role: 'sent', label: 'nav.sent', hint: 'fld.sentHint' },
    { role: 'drafts', label: 'nav.drafts', hint: 'fld.draftsHint' },
    { role: 'archive', label: 'nav.archive', hint: 'fld.archiveHint' },
    { role: 'junk', label: 'nav.junk', hint: 'fld.junkHint' },
    { role: 'trash', label: 'nav.trash', hint: 'fld.trashHint' },
  ];

  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = toAppError(e).message;
  }

  async function setHidden(b: Mailbox, hidden: boolean) {
    error = null;
    try {
      await call('mailbox_set_hidden', { id: b.id, hidden });
      await reloadMailboxes();
    } catch (e) {
      fail(e);
    }
  }

  async function setRole(role: SpecialRole, id: string) {
    if (!id) return;
    error = null;
    try {
      await call('mailbox_set_role', { id, role });
      await reloadMailboxes();
      showToast({ text: t('fld.roleSet') });
    } catch (e) {
      fail(e);
    }
  }

  async function resetRoles() {
    error = null;
    try {
      await call('mailbox_roles_reset', { accountId: account.id });
      await reloadMailboxes();
      showToast({ text: t('fld.rolesReset') });
    } catch (e) {
      fail(e);
    }
  }

  /** Özel klasör seçimi için ağaçtaki yol ("Projeler / 2024"). */
  function pathLabel(b: Mailbox): string {
    const parts = [folderLabel(b)];
    let p = b.parentId ? own.find((x) => x.id === b.parentId) : undefined;
    for (let guard = 0; p && guard < 16; guard++) {
      parts.unshift(folderLabel(p));
      p = p.parentId ? own.find((x) => x.id === p!.parentId) : undefined;
    }
    return parts.join(' / ');
  }

  const choices = $derived(own.filter((b) => b.role !== 'inbox').map((b) => ({ id: b.id, label: pathLabel(b) })).sort((a, b) => a.label.localeCompare(b.label, 'tr')));
</script>

<h3 class="set-h">{t('fld.rolesTitle')}</h3>
<p class="set-lead">{t('fld.rolesLead')}</p>
{#each roles as r (r.role)}
  {@const current = own.find((b) => b.role === r.role)}
  <div class="set-row">
    <label class="set-name" for="role-{r.role}">{t(r.label)}<span class="set-hint">{t(r.hint)}</span></label>
    <select id="role-{r.role}" class="input small pick" value={current?.id ?? ''} onchange={(e) => setRole(r.role, (e.currentTarget as HTMLSelectElement).value)}>
      {#if !current}<option value="" disabled>{t('fld.noFolder')}</option>{/if}
      {#each choices as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
    </select>
  </div>
{/each}
<div class="set-actions bar">
  <button class="btn-ghost" onclick={resetRoles}><Icon name="reset" size={16} />{t('fld.reset')}</button>
</div>

<h3 class="set-h">{t('fld.visibleTitle')}</h3>
<p class="set-lead">{t('fld.visibleLead')}</p>
{#if error}<p class="set-error" role="alert">{error}</p>{/if}
<ul class="tree">
  {#each rows as node (node.mailbox.id)}
    {@const b = node.mailbox}
    <li class="folder" class:hidden={b.hidden} style:--depth={node.depth}>
      <Icon name={folderIcon(b)} size={16} />
      <span class="trunc grow">{folderLabel(b)}</span>
      <span class="meta num">{t('fld.counts', { total: b.total, unread: b.unread })}</span>
      <input
        type="checkbox"
        role="switch"
        class="switch"
        aria-label={t('fld.show', { name: folderLabel(b) })}
        checked={!b.hidden}
        disabled={b.role === 'inbox'}
        onchange={(e) => setHidden(b, !(e.currentTarget as HTMLInputElement).checked)}
      />
    </li>
  {:else}
    <li class="meta">{t('fld.none')}</li>
  {/each}
</ul>

<style>
  .pick {
    max-width: 360px;
  }

  .bar {
    margin-top: 12px;
    justify-content: flex-start;
  }

  .bar .btn-ghost {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .tree {
    display: flex;
    flex-direction: column;
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--line);
  }

  .folder {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding-left: calc(4px + var(--depth) * 20px);
    border-bottom: 1px solid var(--line);
    color: var(--ink-2);
  }

  .grow {
    flex: 1;
    color: var(--ink);
  }

  .hidden .grow {
    color: var(--ink-3);
  }

  @container settings (max-width: 600px) {
    .pick {
      max-width: none;
    }

    .folder {
      padding-left: calc(var(--depth) * 16px);
    }
  }
</style>
