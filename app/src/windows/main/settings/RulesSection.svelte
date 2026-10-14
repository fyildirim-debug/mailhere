<script lang="ts">
  import { onMount } from 'svelte';
  import { call, toAppError, type Rule, type RuleAction, type RuleCondition } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import { folderLabel } from '../../../lib/mail/folders';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { showToast } from '../../../lib/ui/toast.svelte';
  import { mail } from '../../../lib/stores/mail.svelte';
  import { tagById } from '../../../lib/stores/tags.svelte';
  import RuleEditor from './RuleEditor.svelte';

  let rules = $state<Rule[]>([]);
  let loaded = $state(false);
  let loadError = $state<string | null>(null);
  /** Düzenlenen kural; yeni kuralda id boş. */
  let editing = $state<Rule | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    try {
      rules = await call('rules_list');
      loadError = null;
    } catch (e) {
      loadError = toAppError(e).message;
    } finally {
      loaded = true;
    }
  }

  onMount(() => {
    void load();
    // Postadan "Kural oluştur" ile gelindiyse taslak hemen açılır.
    if (mail.ruleDraft) {
      editing = mail.ruleDraft;
      mail.ruleDraft = null;
    }
  });

  function blank(): Rule {
    return {
      id: '',
      name: '',
      enabled: true,
      accountId: mail.accounts.length === 1 ? mail.accounts[0].id : null,
      match: 'all',
      conditions: [{ kind: 'from', op: 'contains', value: '' }],
      actions: [{ kind: 'markRead' }],
      stop: false,
    };
  }

  async function save(rule: Rule) {
    saving = true;
    error = null;
    try {
      const fresh = !rule.id;
      const saved = await call('rule_save', { rule });
      editing = null;
      await load();
      showToast({
        text: t(fresh ? 'rule.created' : 'toast.saved'),
        action: fresh ? { label: t('rule.runNow'), run: () => void run(saved) } : undefined,
      });
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      saving = false;
    }
  }

  async function toggle(rule: Rule, enabled: boolean) {
    try {
      await call('rule_save', { rule: { ...$state.snapshot(rule), enabled } });
      await load();
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  async function remove(rule: Rule) {
    try {
      await call('rule_delete', { id: rule.id });
      await load();
      const copy = { ...$state.snapshot(rule), id: '' };
      showToast({ text: t('rule.deleted', { name: rule.name }), action: { label: t('act.undo'), run: () => void save(copy) } });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  async function shift(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= rules.length) return;
    const next = [...rules];
    [next[i], next[j]] = [next[j], next[i]];
    rules = next;
    try {
      await call('rules_reorder', { ids: next.map((r) => r.id) });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
      await load();
    }
  }

  async function run(rule: Rule) {
    try {
      const n = await call('rule_run', { id: rule.id });
      showToast({ text: n ? t('rule.ran', { n }) : t('rule.ranNone') });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  // ------------------------------------------------------------ özet metni

  const opKey: Record<string, MessageKey> = { contains: 'rule.op.contains', notContains: 'rule.op.notContains', is: 'rule.op.is' };
  const fieldKey: Record<string, MessageKey> = { from: 'rule.c.from', to: 'rule.c.to', subject: 'rule.c.subject', body: 'rule.c.body' };
  const catKey: Record<string, MessageKey> = { people: 'cat.people', notifications: 'cat.notifications', newsletters: 'cat.newsletters' };

  function condText(c: RuleCondition): string {
    if (c.kind === 'hasAttachment') return t('rule.c.hasAttachment');
    if (c.kind === 'category') return `${t('rule.c.category')}: ${t(catKey[c.value])}`;
    return `${t(fieldKey[c.kind])} ${t(opKey[c.op])} “${c.value}”`;
  }

  function actText(a: RuleAction): string {
    switch (a.kind) {
      case 'move': {
        const b = mail.mailboxes.find((x) => x.id === a.mailboxId);
        return t('rule.sum.move', { folder: b ? folderLabel(b) : '?' });
      }
      case 'forward':
        return t('rule.sum.forward', { to: a.to });
      case 'tag':
        return t('tag.sum', { tag: tagById(a.tagId)?.name ?? t('tag.missing') });
      default:
        return t(`rule.a.${a.kind}` as MessageKey);
    }
  }

  function summary(r: Rule): string {
    const join = r.match === 'all' ? t('rule.and') : t('rule.or');
    return `${r.conditions.map(condText).join(` ${join} `)} → ${r.actions.map(actText).join(', ')}`;
  }

  function accountName(id: string | null): string {
    return id ? (mail.accounts.find((a) => a.id === id)?.name ?? '?') : t('nav.allAccounts');
  }
</script>

<h2 class="page">{t('set.nav.rules')}</h2>
<p class="set-lead">{t('rule.lead')}</p>

{#if loadError}
  <p class="set-error" role="alert">{t('rule.loadFailed', { message: loadError })}</p>
{/if}

{#if editing && !editing.id}
  <RuleEditor rule={editing} {saving} {error} onsave={save} oncancel={() => ((editing = null), (error = null))} />
{/if}

<ol class="rules">
  {#each rules as rule, i (rule.id)}
    {#if editing?.id === rule.id}
      <li><RuleEditor rule={editing} {saving} {error} onsave={save} oncancel={() => ((editing = null), (error = null))} /></li>
    {:else}
      <li class="rule" class:off={!rule.enabled}>
        <input
          type="checkbox"
          role="switch"
          class="switch"
          aria-label={t('rule.enabledNamed', { name: rule.name })}
          checked={rule.enabled}
          onchange={(e) => toggle(rule, (e.currentTarget as HTMLInputElement).checked)}
        />
        <div class="text">
          <div class="title">
            <span class="trunc name">{rule.name}</span>
            <span class="tag">{accountName(rule.accountId)}</span>
            {#if rule.stop}<span class="meta">{t('rule.stopsShort')}</span>{/if}
          </div>
          <span class="sum">{summary(rule)}</span>
        </div>
        <div class="tools">
          <button class="btn-icon small" aria-label={t('qr.up')} title={t('qr.up')} disabled={i === 0} onclick={() => shift(i, -1)}><Icon name="arrowUp" size={14} /></button>
          <button class="btn-icon small" aria-label={t('qr.down')} title={t('qr.down')} disabled={i === rules.length - 1} onclick={() => shift(i, 1)}><Icon name="arrowDown" size={14} /></button>
          <button class="btn-ghost" onclick={() => run(rule)}>{t('rule.runNow')}</button>
          <button class="btn-ghost" onclick={() => ((editing = $state.snapshot(rule)), (error = null))}>{t('set.editAccount')}</button>
          <button class="btn-ghost" onclick={() => remove(rule)}>{t('set.remove')}</button>
        </div>
      </li>
    {/if}
  {:else}
    {#if loaded && !editing}
      <li class="empty">
        <Icon name="rules" size={28} />
        <span class="empty-title">{t('rule.emptyTitle')}</span>
        <span class="meta">{t('rule.emptyBody')}</span>
      </li>
    {/if}
  {/each}
</ol>

{#if !editing}
  <button class="btn-soft add" onclick={() => ((editing = blank()), (error = null))}><Icon name="plus" size={16} />{t('rule.new')}</button>
{/if}

<style>
  .page {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .rules {
    display: flex;
    flex-direction: column;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }

  .rule {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 14px 0;
    border-bottom: 1px solid var(--line);
  }

  .rule .switch {
    margin-top: 2px;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .name {
    font-weight: 500;
  }

  .sum {
    font-size: var(--fs-meta);
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }

  .off .name,
  .off .sum {
    color: var(--ink-3);
  }

  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .btn-icon:disabled {
    opacity: 0.35;
    background: transparent;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 40px 16px;
    color: var(--accent);
    text-align: center;
  }

  .empty-title {
    color: var(--ink);
    font-weight: 600;
  }

  .add {
    margin-top: 16px;
  }
</style>
