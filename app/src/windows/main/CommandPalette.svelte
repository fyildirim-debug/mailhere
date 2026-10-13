<script lang="ts">
  import { call, type MessageSummary } from '../../lib/api';
  import { t, when, type MessageKey } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import { displayName, snoozeOptions } from '../../lib/util/format';
  import { mail, moveTo, openSettings, searchAll, select, setView, snooze } from '../../lib/stores/mail.svelte';
  import { resolvedTheme, toggleTheme } from '../../lib/stores/settings.svelte';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  interface Item {
    key: string;
    label: string;
    meta?: string;
    kbd?: string;
    icon?: IconName;
    person?: MessageSummary['from'];
    plain?: boolean;
    run: () => void;
  }

  let q = $state('');
  let active = $state(0);
  let found = $state<MessageSummary[]>([]);
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLDivElement>();

  const needle = $derived(q.trim().toLocaleLowerCase('tr'));

  // Yazarken yerel dizinde ara (kısa gecikmeyle).
  $effect(() => {
    const query = q.trim();
    if (!query) {
      found = [];
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const res = await call('search', { q: query, limit: 20 });
        if (q.trim() === query) found = res;
      } catch {
        found = [];
      }
    }, 80);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    input?.focus();
  });

  function done(fn: () => void) {
    return () => {
      onclose();
      fn();
    };
  }

  const actions = $derived.by((): Item[] => {
    const sel = mail.selectedId;
    const list: (Item & { label: string })[] = [
      { key: 'compose', label: t('act.compose'), kbd: 'C', icon: 'pen', run: done(() => call('open_compose', { prefill: { mode: 'new', messageId: null } })) },
    ];
    if (sel != null) {
      list.push(
        { key: 'archive', label: t('act.archive'), kbd: 'E', icon: 'archive', run: done(() => moveTo([sel], 'archive')) },
        {
          key: 'snooze',
          label: `${t('act.snooze')} · ${t('snooze.tomorrow')}`,
          kbd: 'H',
          icon: 'clock',
          run: done(() => snooze([sel], snoozeOptions(Date.now()).find((o) => o.key === 'tomorrow')!.at)),
        },
      );
    }
    list.push(
      { key: 'inbox', label: t('palette.goInbox'), icon: 'inbox', run: done(() => setView({ kind: 'inbox' })) },
      { key: 'archiveView', label: t('palette.goArchive'), icon: 'archive', run: done(() => setView({ kind: 'archive' })) },
      {
        key: 'theme',
        label: resolvedTheme() === 'dark' ? t('palette.themeLight') : t('palette.themeDark'),
        icon: resolvedTheme() === 'dark' ? 'sun' : 'moon',
        run: done(() => toggleTheme()),
      },
      { key: 'mute', label: t('act.muteHour'), icon: 'bellOff', run: done(() => call('notify_snooze', { minutes: 60 })) },
      { key: 'account', label: t('act.addAccount'), icon: 'plus', run: done(() => (mail.screen = 'onboarding')) },
      { key: 'settings', label: t('act.settings'), kbd: 'Ctrl ,', icon: 'settings', run: done(() => openSettings()) },
      { key: 'rules', label: t('palette.rules'), icon: 'rules', run: done(() => openSettings('rules')) },
      { key: 'templates', label: t('palette.templates'), icon: 'template', run: done(() => openSettings('compose')) },
      { key: 'scheduled', label: t('palette.scheduled'), icon: 'calendarClock', run: done(() => (mail.screen = 'scheduled')) },
    );
    for (const a of mail.accounts) {
      list.push({ key: `acc-${a.id}`, label: t('palette.accountSettings', { name: a.name }), icon: 'user', run: done(() => openSettings(`account:${a.id}`)) });
    }
    return list;
  });

  function matches(label: string) {
    return !needle || label.toLocaleLowerCase('tr').includes(needle);
  }

  const sections = $derived.by(() => {
    const out: { title: MessageKey; items: Item[] }[] = [];
    if (needle) {
      out.push({
        title: 'palette.search',
        items: [
          {
            key: 'search',
            label: t('palette.searchAll', { q: q.trim() }),
            meta: t('palette.searchMeta'),
            icon: 'search',
            plain: true,
            run: done(() => searchAll(q)),
          },
        ],
      });
    }
    const acts = actions.filter((a) => matches(a.label)).slice(0, needle ? 4 : 6);
    if (acts.length) out.push({ title: 'palette.actions', items: acts });
    // Arama işleçleri: tıklayınca sorguya eklenir, palet açık kalır.
    if (!needle || /^\S*$/.test(q.trim())) {
      const ops: [string, string][] = [
        ['from:', t('palette.op.from')],
        ['has:ek', t('palette.op.attachment')],
        ['is:okunmamış', t('palette.op.unread')],
        ['in:arşiv', t('palette.op.in')],
        ['after:2026-01-01', t('palette.op.after')],
      ];
      const tips = ops
        .filter(([op]) => !needle || op.startsWith(needle))
        .map(([op, meta]) => ({
          key: `op-${op}`,
          label: op,
          meta,
          icon: 'filter' as IconName,
          plain: true,
          run: () => {
            q = `${q.trim() && !op.startsWith(needle) ? `${q.trim()} ` : ''}${op}`;
            input?.focus();
          },
        }));
      if (tips.length && !found.length) out.push({ title: 'palette.ops', items: tips.slice(0, needle ? 3 : 5) });
    }
    const mails = needle ? found.slice(0, 5) : mail.items.slice(0, 3);
    if (mails.length) {
      out.push({
        title: needle ? 'palette.mails' : 'palette.recent',
        items: mails.map((m) => ({
          key: `m${m.id}`,
          label: m.subject || '—',
          meta: `${displayName(m.from)} · ${when(m.date)}`,
          icon: 'mail' as IconName,
          run: done(() => select(m.id)),
        })),
      });
    }
    if (needle) {
      const seen = new Set<string>();
      const people = found
        .map((m) => m.from)
        .filter((a) => {
          const k = a.email.toLowerCase();
          if (seen.has(k)) return false;
          seen.add(k);
          return true;
        })
        .slice(0, 3);
      if (people.length) {
        out.push({
          title: 'palette.people',
          items: people.map((p) => ({ key: `p${p.email}`, label: displayName(p), meta: p.email, person: p, run: done(() => searchAll(p.email)) })),
        });
      }
    }
    return out;
  });

  const flat = $derived(sections.flatMap((s) => s.items));
  const total = $derived(flat.length);

  $effect(() => {
    if (active >= total) active = Math.max(0, total - 1);
  });

  $effect(() => {
    listEl?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  function split(label: string, plain?: boolean) {
    if (plain || !needle) return { pre: label, mid: '', post: '' };
    const i = label.toLocaleLowerCase('tr').indexOf(needle);
    if (i < 0) return { pre: label, mid: '', post: '' };
    return { pre: label.slice(0, i), mid: label.slice(i, i + needle.length), post: label.slice(i + needle.length) };
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(active + 1, total - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      flat[active]?.run();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<div class="scrim" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="palette" role="dialog" aria-modal="true" aria-label={t('palette.label')}>
    <div class="search">
      <Icon name="search" size={20} />
      <input
        bind:this={input}
        bind:value={q}
        oninput={() => (active = 0)}
        onkeydown={onKey}
        placeholder={t('palette.placeholder')}
        aria-label={t('palette.placeholder')}
        aria-controls="palette-list"
        aria-activedescendant={total ? `palette-${active}` : undefined}
        role="combobox"
        aria-expanded="true"
      />
      {#if q}<button class="btn-ghost" onclick={() => (q = '')}>{t('act.clear')}</button>{/if}
      <span class="kbd">Esc</span>
    </div>

    <div class="results" id="palette-list" role="listbox" bind:this={listEl}>
      {#each sections as sec (sec.title)}
        <div class="sec-title">{t(sec.title)}</div>
        {#each sec.items as it (it.key)}
          {@const index = flat.indexOf(it)}
          {@const parts = split(it.label, it.plain)}
          <button
            class="item"
            class:active={index === active}
            id="palette-{index}"
            role="option"
            aria-selected={index === active}
            data-index={index}
            onmouseenter={() => (active = index)}
            onclick={() => it.run()}
          >
            {#if it.person}
              <Avatar address={it.person} size={28} />
            {:else if it.icon}
              <span class="item-icon"><Icon name={it.icon} size={16} /></span>
            {/if}
            <span class="it-label trunc">{parts.pre}<mark>{parts.mid}</mark>{parts.post}</span>
            {#if it.kbd}<span class="kbd">{it.kbd}</span>{:else if it.meta}<span class="meta trunc item-meta">{it.meta}</span>{/if}
          </button>
        {/each}
      {/each}
      {#if !total}
        <div class="nothing">{t('palette.nothing')}</div>
      {/if}
    </div>

    <div class="foot">
      <span class="hint"><span class="kbd"><Icon name="arrowUp" size={12} /></span><span class="kbd"><Icon name="arrowDown" size={12} /></span>{t('palette.navigate')}</span>
      <span class="hint"><span class="kbd">Enter</span>{t('palette.select')}</span>
      <span class="grow"></span>
      <span>{needle ? t('palette.results', { n: total }) : t('palette.hint')}</span>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 64px 16px 16px;
    background: color-mix(in srgb, var(--ground) 55%, transparent);
    animation: fade 120ms ease-out;
  }

  .palette {
    width: 620px;
    max-width: 100%;
    max-height: min(520px, calc(100vh - 96px));
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: var(--r-palette);
    background: var(--paper);
    border: 1px solid var(--line);
    box-shadow: var(--shadow-lg);
    animation: drop var(--t-state) var(--ease);
  }

  .search {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 58px;
    padding: 0 16px;
    border-bottom: 1px solid var(--line);
    color: var(--ink-3);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 40px;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--ink);
    font-size: var(--fs-section);
    letter-spacing: -0.01em;
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 6px 8px 8px;
  }

  .sec-title {
    padding: 12px 12px 6px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    height: 44px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--ink);
    text-align: left;
  }

  .item.active {
    background: var(--sel);
  }

  .item-icon {
    width: 28px;
    height: 28px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    background: var(--surface);
    color: var(--ink-2);
  }

  .item.active .item-icon {
    background: var(--paper);
    color: var(--accent-text);
  }

  .it-label {
    flex: 1;
  }

  mark {
    background: transparent;
    color: var(--accent-text);
    font-weight: 600;
  }

  .item-meta {
    flex: none;
    max-width: 220px;
  }

  .nothing {
    padding: 40px 20px;
    color: var(--ink-3);
    text-align: center;
  }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 16px;
    height: 42px;
    padding: 0 16px;
    border-top: 1px solid var(--line);
    background: var(--surface);
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .grow {
    flex: 1;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-6px) scale(0.985);
    }
  }
</style>
