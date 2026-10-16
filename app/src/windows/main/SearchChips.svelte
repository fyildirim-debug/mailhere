<script lang="ts">
  // Arama sonuçlarının üstündeki süzgeç çipleri: Kimden, Tarih, Ekli,
  // Okunmamış, Hesap. Çip seçimi sorgu metnine işleç olarak yazılır ve arama
  // yeniden çalışır; çipin durumu da metinden okunur (doğruluk kaynağı metin).
  import { t } from '../../lib/i18n/index.svelte';
  import { presetDate, readChips, withOp, type ChipOp, type DatePreset } from '../../lib/mail/query';
  import Icon from '../../lib/ui/Icon.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import { displayName } from '../../lib/util/format';
  import { mail, searchAll } from '../../lib/stores/mail.svelte';

  const q = $derived(mail.search ?? '');
  const chips = $derived(readChips(q));
  const multiAccount = $derived(mail.accounts.length > 1);

  let menu = $state<{ at: { x: number; y: number }; title: string; items: MenuItem[] } | null>(null);

  function set(op: ChipOp, value: string | boolean | null) {
    searchAll(withOp(q, op, value));
  }

  function anchor(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left, y: r.bottom + 4 };
  }

  function fromMenu(e: MouseEvent) {
    const seen = new Map<string, { name: string; email: string }>();
    for (const m of mail.items) {
      const k = m.from.email.toLowerCase();
      if (!seen.has(k)) seen.set(k, { name: displayName(m.from), email: m.from.email });
    }
    const people = [...seen.values()].slice(0, 8);
    menu = {
      at: anchor(e),
      title: t('chip.from'),
      items: people.length
        ? people.map((p) => ({ key: p.email, label: p.name === p.email ? p.email : `${p.name} · ${p.email}`, icon: 'user' as const, checked: chips.from === p.email, onclick: () => set('from', p.email) }))
        : [{ label: t('chip.noSenders'), disabled: true, onclick: () => {} }],
    };
  }

  const PRESETS: [DatePreset, 'date.today' | 'date.week' | 'date.month' | 'date.last30'][] = [
    ['today', 'date.today'],
    ['week', 'date.week'],
    ['month', 'date.month'],
    ['last30', 'date.last30'],
  ];

  function dateMenu(e: MouseEvent) {
    const now = Date.now();
    menu = {
      at: anchor(e),
      title: t('chip.date'),
      items: PRESETS.map(([p, label]) => {
        const day = presetDate(p, now);
        return { key: p, label: t(label), hint: day, checked: chips.after === day, onclick: () => set('after', day) };
      }),
    };
  }

  function accountMenu(e: MouseEvent) {
    menu = {
      at: anchor(e),
      title: t('chip.account'),
      items: mail.accounts.map((a) => ({
        key: a.id,
        label: a.name,
        hint: a.email,
        checked: chips.account?.toLocaleLowerCase('tr') === a.email.toLocaleLowerCase('tr'),
        onclick: () => set('account', a.email),
      })),
    };
  }

  /** Seçili çipin kısa değeri. */
  function accountName(v: string) {
    const lower = v.toLocaleLowerCase('tr');
    return mail.accounts.find((a) => a.email.toLocaleLowerCase('tr') === lower || a.name.toLocaleLowerCase('tr') === lower)?.name ?? v;
  }

  /** Tarih çipinin değeri: hazır tarihse adı ("Bu hafta"), değilse gün; önce: işleci "< gün". */
  function dateName(c: typeof chips): string | null {
    if (c.after) {
      const now = Date.now();
      const preset = PRESETS.find(([p]) => presetDate(p, now) === c.after);
      return preset ? t(preset[1]) : c.after;
    }
    return c.before ? `< ${c.before}` : null;
  }

  function fromName(v: string) {
    const lower = v.toLowerCase();
    const hit = mail.items.find((m) => m.from.email.toLowerCase() === lower);
    return hit ? displayName(hit.from) : v;
  }
</script>

{#snippet menuChip(op: ChipOp, name: string, value: string | null, open: (e: MouseEvent) => void, also: ChipOp | null = null)}
  {#if value}
    <span class="chip on">
      <button class="chip-main" aria-haspopup="menu" onclick={open}><span class="trunc">{name}: {value}</span></button>
      <button
        class="chip-x"
        aria-label={t('chip.clear', { name })}
        title={t('chip.clear', { name })}
        onclick={() => searchAll(also ? withOp(withOp(q, op, null), also, null) : withOp(q, op, null))}
      >
        <Icon name="x" size={12} />
      </button>
    </span>
  {:else}
    <button class="chip" aria-haspopup="menu" onclick={open}>{name}<Icon name="chevronDown" size={14} /></button>
  {/if}
{/snippet}

<div class="chips" role="group" aria-label={t('chip.label')}>
  {@render menuChip('from', t('chip.from'), chips.from ? fromName(chips.from) : null, fromMenu)}
  {@render menuChip('after', t('chip.date'), dateName(chips), dateMenu, 'before')}
  <button class="chip" class:on={chips.attach} aria-pressed={chips.attach} onclick={() => set('attach', !chips.attach)}>
    <Icon name="paperclip" size={14} />{t('chip.attach')}
  </button>
  <button class="chip" class:on={chips.unread} aria-pressed={chips.unread} onclick={() => set('unread', !chips.unread)}>
    {t('chip.unread')}
  </button>
  {#if multiAccount}
    {@render menuChip('account', t('chip.account'), chips.account ? accountName(chips.account) : null, accountMenu)}
  {/if}
</div>

{#if menu}<Menu at={menu.at} title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: -2px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    max-width: 100%;
    padding: 0 10px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: var(--paper);
    color: var(--ink-2);
    font-size: var(--fs-meta);
    font-weight: 500;
    white-space: nowrap;
    transition:
      background var(--t-fast),
      color var(--t-fast);
  }

  button.chip:hover {
    background: var(--hover);
    color: var(--ink);
  }

  .chip.on {
    border-color: transparent;
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  span.chip.on {
    gap: 0;
    padding: 0 2px 0 0;
  }

  .chip-main {
    min-width: 0;
    max-width: 220px;
    height: 100%;
    display: inline-flex;
    align-items: center;
    padding: 0 6px 0 10px;
    border: 0;
    border-radius: 999px 0 0 999px;
    background: transparent;
    color: inherit;
    font: inherit;
  }

  /* Kaldır düğmesi 28×28 (tık hedefi); negatif kenar boşluğuyla çipin 28 px
     yüksekliğini büyütmez. */
  .chip-x {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    margin: -3px -3px -3px 0;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: inherit;
  }

  .chip-x:hover,
  .chip-main:hover {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
</style>
