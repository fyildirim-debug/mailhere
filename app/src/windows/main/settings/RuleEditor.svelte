<script lang="ts">
  import type { Category, Rule, RuleAction, RuleCondition, RuleOp } from '../../../lib/api';
  import { t, type MessageKey } from '../../../lib/i18n/index.svelte';
  import { flatten, folderLabel, folderTree } from '../../../lib/mail/folders';
  import Icon from '../../../lib/ui/Icon.svelte';
  import { mail } from '../../../lib/stores/mail.svelte';
  import { tagStore } from '../../../lib/stores/tags.svelte';

  interface Props {
    rule: Rule;
    saving: boolean;
    error: string | null;
    onsave: (rule: Rule) => void;
    oncancel: () => void;
  }

  let { rule, saving, error, onsave, oncancel }: Props = $props();

  // Düzenleyici kendi kopyasında çalışır; Vazgeç hiçbir şeyi değiştirmez.
  const copy = () => structuredClone($state.snapshot(rule));
  let r = $state<Rule>(copy());

  type CondKind = RuleCondition['kind'];
  type ActKind = RuleAction['kind'];

  const condKinds: [CondKind, MessageKey][] = [
    ['from', 'rule.c.from'],
    ['to', 'rule.c.to'],
    ['subject', 'rule.c.subject'],
    ['body', 'rule.c.body'],
    ['hasAttachment', 'rule.c.hasAttachment'],
    ['category', 'rule.c.category'],
  ];
  const ops: [RuleOp, MessageKey][] = [
    ['contains', 'rule.op.contains'],
    ['notContains', 'rule.op.notContains'],
    ['is', 'rule.op.is'],
  ];
  const categories: [Category, MessageKey][] = [
    ['people', 'cat.people'],
    ['notifications', 'cat.notifications'],
    ['newsletters', 'cat.newsletters'],
  ];
  const actKinds: [ActKind, MessageKey][] = [
    ['move', 'rule.a.move'],
    ['archive', 'rule.a.archive'],
    ['trash', 'rule.a.trash'],
    ['markRead', 'rule.a.markRead'],
    ['star', 'rule.a.star'],
    ['mute', 'rule.a.mute'],
    ['forward', 'rule.a.forward'],
    ['tag', 'tag.rule'],
  ];

  const folders = $derived(
    r.accountId ? flatten(folderTree(mail.mailboxes, r.accountId, true)).filter((n) => n.mailbox.role !== 'inbox') : [],
  );

  function setCondKind(i: number, kind: CondKind) {
    const c = r.conditions[i];
    const value = 'value' in c && typeof c.value === 'string' && kind !== 'category' ? c.value : '';
    r.conditions[i] =
      kind === 'hasAttachment'
        ? { kind }
        : kind === 'category'
          ? { kind, value: 'newsletters' }
          : { kind, op: 'op' in c ? c.op : 'contains', value };
  }

  function setActKind(i: number, kind: ActKind) {
    r.actions[i] =
      kind === 'move'
        ? { kind, mailboxId: folders[0]?.mailbox.id ?? '' }
        : kind === 'forward'
          ? { kind, to: '' }
          : kind === 'tag'
            ? { kind, tagId: tagStore.list[0]?.id ?? '' }
            : { kind };
  }

  // Hesap değişince başka hesabın klasörü seçili kalmasın.
  $effect(() => {
    const ids = new Set(folders.map((n) => n.mailbox.id));
    r.actions.forEach((a, i) => {
      if (a.kind === 'move' && !ids.has(a.mailboxId)) r.actions[i] = { kind: 'move', mailboxId: folders[0]?.mailbox.id ?? '' };
    });
  });

  const needsAccount = $derived(r.actions.some((a) => a.kind === 'move') && !r.accountId);
  const ready = $derived(
    !!r.name.trim() &&
      r.conditions.length > 0 &&
      r.actions.length > 0 &&
      !needsAccount &&
      r.conditions.every((c) => !('op' in c) || !!c.value.trim()) &&
      r.actions.every((a) =>
        a.kind === 'move' ? !!a.mailboxId : a.kind === 'forward' ? /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(a.to.trim()) : a.kind === 'tag' ? !!a.tagId : true,
      ),
  );
</script>

<div class="editor">
  <div class="top">
    <label class="grow"><span class="label">{t('rule.name')}</span><input class="input small" maxlength="80" placeholder={t('rule.namePlaceholder')} bind:value={r.name} /></label>
    <label><span class="label">{t('rule.account')}</span>
      <select class="input small" bind:value={r.accountId}>
        <option value={null}>{t('nav.allAccounts')}</option>
        {#each mail.accounts as a (a.id)}<option value={a.id}>{a.name}</option>{/each}
      </select>
    </label>
  </div>

  <div class="block">
    <div class="block-head">
      <span class="block-title">{t('rule.when')}</span>
      <div class="seg-track" role="radiogroup" aria-label={t('rule.match')}>
        <button class="seg" role="radio" aria-checked={r.match === 'all'} onclick={() => (r.match = 'all')}>{t('rule.matchAll')}</button>
        <button class="seg" role="radio" aria-checked={r.match === 'any'} onclick={() => (r.match = 'any')}>{t('rule.matchAny')}</button>
      </div>
    </div>
    {#each r.conditions as c, i (i)}
      <div class="line">
        <select class="input small kind" aria-label={t('rule.condition', { n: i + 1 })} value={c.kind} onchange={(e) => setCondKind(i, (e.currentTarget as HTMLSelectElement).value as CondKind)}>
          {#each condKinds as [k, label] (k)}<option value={k}>{t(label)}</option>{/each}
        </select>
        {#if c.kind === 'category'}
          <select class="input small grow" aria-label={t('rule.c.category')} bind:value={c.value}>
            {#each categories as [k, label] (k)}<option value={k}>{t(label)}</option>{/each}
          </select>
        {:else if c.kind !== 'hasAttachment'}
          <select class="input small op" aria-label={t('rule.operator')} bind:value={c.op}>
            {#each ops as [k, label] (k)}<option value={k}>{t(label)}</option>{/each}
          </select>
          <input class="input small grow" aria-label={t('rule.value')} maxlength="500" placeholder={c.kind === 'from' || c.kind === 'to' ? 'ad@alanadi.com' : ''} bind:value={c.value} />
        {:else}
          <span class="grow"></span>
        {/if}
        <button class="btn-icon small" aria-label={t('rule.removeCondition')} title={t('set.remove')} disabled={r.conditions.length === 1} onclick={() => r.conditions.splice(i, 1)}>
          <Icon name="x" size={14} />
        </button>
      </div>
    {/each}
    <button class="link add" disabled={r.conditions.length >= 20} onclick={() => r.conditions.push({ kind: 'from', op: 'contains', value: '' })}>
      <Icon name="plus" size={14} />{t('rule.addCondition')}
    </button>
  </div>

  <div class="block">
    <div class="block-head"><span class="block-title">{t('rule.then')}</span></div>
    {#each r.actions as a, i (i)}
      <div class="line">
        <select class="input small kind" aria-label={t('rule.action', { n: i + 1 })} value={a.kind} onchange={(e) => setActKind(i, (e.currentTarget as HTMLSelectElement).value as ActKind)}>
          {#each actKinds as [k, label] (k)}<option value={k}>{t(label)}</option>{/each}
        </select>
        {#if a.kind === 'move'}
          <select class="input small grow" aria-label={t('rule.folder')} bind:value={a.mailboxId} disabled={!r.accountId}>
            {#each folders as n (n.mailbox.id)}<option value={n.mailbox.id}>{'· '.repeat(n.depth)}{folderLabel(n.mailbox)}</option>{/each}
          </select>
        {:else if a.kind === 'forward'}
          <input class="input small grow" type="email" aria-label={t('rule.forwardTo')} placeholder="ad@alanadi.com" bind:value={a.to} />
        {:else if a.kind === 'tag'}
          <select class="input small grow" aria-label={t('tag.name')} bind:value={a.tagId} disabled={!tagStore.list.length}>
            {#each tagStore.list as tg (tg.id)}<option value={tg.id}>{tg.name}</option>{/each}
          </select>
        {:else}
          <span class="grow"></span>
        {/if}
        <button class="btn-icon small" aria-label={t('rule.removeAction')} title={t('set.remove')} disabled={r.actions.length === 1} onclick={() => r.actions.splice(i, 1)}>
          <Icon name="x" size={14} />
        </button>
      </div>
    {/each}
    <button class="link add" disabled={r.actions.length >= 10} onclick={() => r.actions.push({ kind: 'markRead' })}>
      <Icon name="plus" size={14} />{t('rule.addAction')}
    </button>
    {#if needsAccount}<p class="hint warn">{t('rule.needsAccount')}</p>{/if}
    {#if r.actions.some((x) => x.kind === 'forward')}<p class="hint">{t('rule.forwardNote')}</p>{/if}
    {#if r.actions.some((x) => x.kind === 'tag') && !tagStore.list.length}<p class="hint warn">{t('tag.none')}</p>{/if}
  </div>

  <label class="stop">
    <input type="checkbox" bind:checked={r.stop} />
    <span>{t('rule.stop')}</span>
  </label>

  {#if error}<p class="set-error" role="alert">{error}</p>{/if}
  <div class="set-actions">
    <button class="btn-ghost" onclick={oncancel}>{t('act.cancel')}</button>
    <button class="btn-primary small" disabled={!ready || saving} onclick={() => onsave($state.snapshot(r))}>{t('rule.save')}</button>
  </div>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: 16px;
    margin: 8px 0 12px;
    padding: 18px;
    border-radius: var(--r-card);
    background: var(--surface);
    border: 1px solid var(--line);
    animation: fade-up var(--t-state) ease-out;
  }

  .top {
    display: flex;
    gap: 12px;
  }

  .top select {
    width: 200px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .block-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 34px;
  }

  .block-title {
    font-weight: 600;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .kind {
    width: 172px;
    flex: none;
  }

  .op {
    width: 132px;
    flex: none;
  }

  .add {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    font-size: var(--fs-meta);
  }

  .btn-icon:disabled {
    opacity: var(--dim-disabled);
    background: transparent;
  }

  .hint {
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .hint.warn {
    color: var(--amber-text);
  }

  .stop {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--ink-2);
  }

  .stop input {
    accent-color: var(--accent);
  }

  /* Mobil: her koşul ve eylem iki satır olur — üstte tür ve kaldır düğmesi,
     altta işleç ve değer. */
  @container settings (max-width: 600px) {
    .editor {
      padding: 14px;
    }

    .top {
      flex-direction: column;
    }

    .top select {
      width: 100%;
    }

    .block-head {
      flex-wrap: wrap;
    }

    .block-head .seg-track {
      flex: 1 1 100%;
    }

    .line {
      flex-wrap: wrap;
    }

    .kind {
      order: 0;
      flex: 1 1 60%;
      width: auto;
    }

    .line .btn-icon {
      order: 1;
    }

    .op {
      order: 2;
      flex: 0 0 40%;
      width: auto;
    }

    .line .grow {
      order: 3;
      flex: 1 1 120px;
    }

    .line span.grow {
      display: none;
    }

    .stop {
      min-height: 44px;
    }

    .stop input {
      width: 20px;
      height: 20px;
    }
  }
</style>
