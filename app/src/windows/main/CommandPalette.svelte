<script lang="ts">
  // Komut paleti (Ctrl K, /): arama, komutlar, postalar ve kişiler. Her
  // komutun kısayolu yanında yazar; komutlar iki dilde (ve eşanlamlılarıyla)
  // bulanık eşleşir, sık kullanılanlar üste çıkar. Arama işleçleri yazılırken
  // tamamlanır: from: kişi, after: / before: hazır tarih, hesap: hesap.
  import { call, type Address, type MessageSummary } from '../../lib/api';
  import { t, when, type MessageKey } from '../../lib/i18n/index.svelte';
  import { en } from '../../lib/i18n/en';
  import { tr } from '../../lib/i18n/tr';
  import { completeWith, completing, presetDate, type DatePreset } from '../../lib/mail/query';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import { displayName, snoozeOptions } from '../../lib/util/format';
  import { goLabel, KEY_LABEL, type GoTarget } from '../../lib/util/keys';
  import { mail, moveTo, openSettings, pullMail, searchAll, select, selectedIds, setUnread, setView, snooze, toggleStar } from '../../lib/stores/mail.svelte';
  import { togglePin } from '../../lib/stores/bulk.svelte';
  import { resolvedTheme, toggleTheme } from '../../lib/stores/settings.svelte';
  import { matchRange, score } from './fuzzy';
  import { desk, showReader, toggleSidebar } from './layout.svelte';
  import { scale, setScale, stepped } from './scale.svelte';
  import { startTriage } from './triage.svelte';

  interface Props {
    onclose: () => void;
    /** Ana pencerenin yönettiği katmanlar: kısayol sayfası, taşı / etiket seçicisi, yanıt kutusu. */
    onopen: (what: 'keys' | 'move' | 'tag' | 'reply') => void;
  }

  let { onclose, onopen }: Props = $props();

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

  interface Cmd extends Item {
    /** Eşleşmede kullanılan metinler (iki dilde ad + eşanlamlılar). */
    texts: string[];
  }

  // "Aramayı düzenle" paleti sorguyla açar.
  let q = $state(desk.paletteSeed ?? '');
  desk.paletteSeed = null;
  let active = $state(0);
  let found = $state<MessageSummary[]>([]);
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLDivElement>();

  const needle = $derived(q.trim());
  const comp = $derived(completing(q));

  // Yazarken yerel dizinde ara (kısa gecikmeyle). Değeri boş işleç ("from:") aranmaz.
  $effect(() => {
    const query = q.trim();
    if (!query || (comp && !comp.value)) {
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

  // ------------------------------------------------------- kullanım sıklığı

  const USE_KEY = 'mh.palette.use';

  function loadUse(): Record<string, number> {
    try {
      return JSON.parse(localStorage.getItem(USE_KEY) ?? '{}') as Record<string, number>;
    } catch {
      return {};
    }
  }

  const use = loadUse();

  function used(key: string) {
    use[key] = (use[key] ?? 0) + 1;
    try {
      localStorage.setItem(USE_KEY, JSON.stringify(use));
    } catch {
      /* yalnız bu oturum */
    }
  }

  function done(key: string, fn: () => void) {
    return () => {
      used(key);
      onclose();
      fn();
    };
  }

  // ------------------------------------------------------------- komutlar

  // Arama için görünmeyen anahtar sözcükler (iki dilde); arayüzde gösterilmez.
  const SYN: Record<string, string> = {
    compose: 'yaz oluştur e-posta mesaj new write message email',
    archive: 'bitti kaldır done remove',
    snooze: 'sonra hatırlat later remind',
    reply: 'cevap cevapla respond answer',
    delete: 'çöp kaldır trash remove bin',
    star: 'önemli işaretle favori flag important',
    unread: 'okunmamış işaretle mark',
    pin: 'üste tut sabit',
    move: 'klasör folder',
    tag: 'etiket label',
    triage: 'odak tek tek işle sıfır inbox zero focus process one by one',
    'go-inbox': 'gelen inbox',
    'go-starred': 'yıldız starred',
    'go-sent': 'giden gönderilen sent',
    'go-drafts': 'taslak draft',
    'go-archive': 'arşiv archive',
    'go-snoozed': 'ertelenen snoozed',
    'go-trash': 'çöp silinen trash bin deleted',
    theme: 'karanlık aydınlık koyu açık dark light mode tema theme',
    mute: 'sessiz sustur rahatsız etme dnd quiet bildirim notification',
    account: 'hesap ekle add account',
    settings: 'ayar tercih preferences options',
    rules: 'kural süzgeç filter',
    tags: 'etiket label',
    templates: 'şablon hazır yanıt template snippet',
    scheduled: 'planlanan zamanlanmış sonra gönder send later',
    keys: 'kısayol klavye yardım help shortcuts keyboard',
    sidebar: 'kenar çubuğu panel menü sidebar',
    sync: 'eşitle yenile al refresh sync fetch',
    zoomIn: 'büyüt yakınlaştır ölçek zoom larger bigger',
    zoomOut: 'küçült uzaklaştır ölçek zoom smaller',
    zoomReset: 'sıfırla ölçek zoom reset 100',
  };

  function cmd(key: string, label: MessageKey, icon: IconName, run: () => void, kbd?: string, text?: string): Cmd {
    return { key, label: text ?? t(label), kbd, icon, run: done(key, run), texts: [text ?? t(label), tr[label], en[label], SYN[key] ?? ''] };
  }

  const GO: [GoTarget, MessageKey][] = [
    ['inbox', 'palette.goInbox'],
    ['starred', 'palette.goStarred'],
    ['sent', 'palette.goSent'],
    ['drafts', 'palette.goDrafts'],
    ['archive', 'palette.goArchive'],
    ['snoozed', 'palette.goSnoozed'],
    ['trash', 'palette.goTrash'],
  ];

  const commands = $derived.by((): Cmd[] => {
    const sel = mail.selectedId;
    const list: Cmd[] = [cmd('compose', 'act.compose', 'pen', () => call('open_compose', { prefill: { mode: 'new', messageId: null } }), KEY_LABEL.compose)];
    if (sel != null) {
      const pinned = mail.items.find((m) => m.id === sel)?.pinned ?? mail.detail?.pinned ?? false;
      const tomorrow = snoozeOptions(Date.now()).find((o) => o.key === 'tomorrow')!.at;
      list.push(
        cmd('archive', 'act.archive', 'archive', () => moveTo(selectedIds(), 'archive'), KEY_LABEL.archive),
        cmd('snooze', 'act.snooze', 'clock', () => snooze(selectedIds(), tomorrow), KEY_LABEL.snooze, `${t('act.snooze')} · ${t('snooze.tomorrow')}`),
        cmd('reply', 'palette.reply', 'reply', () => onopen('reply'), KEY_LABEL.reply),
        cmd('delete', 'act.delete', 'trash', () => moveTo(selectedIds(), 'trash'), KEY_LABEL.delete),
        cmd('star', 'palette.star', 'star', () => toggleStar(sel), KEY_LABEL.star),
        cmd('unread', 'palette.unread', 'mail', () => setUnread([sel], true), KEY_LABEL.unread),
        cmd('pin', pinned ? 'pin.unpin' : 'pin.pin', 'pin', () => togglePin(selectedIds()), KEY_LABEL.pin),
        cmd('move', 'palette.moveTo', 'folder', () => onopen('move'), KEY_LABEL.movePicker),
        cmd('tag', 'palette.tagWith', 'tag', () => onopen('tag'), KEY_LABEL.tagPicker),
      );
    }
    list.push(cmd('triage', 'palette.triage', 'focus', () => startTriage(), KEY_LABEL.triage));
    for (const [target, label] of GO) list.push(cmd(`go-${target}`, label, target === 'inbox' ? 'inbox' : 'chevronRight', () => setView({ kind: target }, mail.accountId), goLabel(target)));
    list.push(
      cmd('keys', 'palette.keys', 'keyboard', () => onopen('keys'), KEY_LABEL.help),
      cmd('sidebar', 'palette.sidebar', 'sidebar', () => toggleSidebar(), KEY_LABEL.sidebar),
      cmd('sync', 'palette.sync', 'refresh', () => pullMail(), KEY_LABEL.sync),
      cmd('theme', resolvedTheme() === 'dark' ? 'palette.themeLight' : 'palette.themeDark', resolvedTheme() === 'dark' ? 'sun' : 'moon', () => toggleTheme()),
      cmd('zoomIn', 'palette.zoomIn', 'plus', () => setScale(stepped(scale.value, 1)), KEY_LABEL.zoomIn),
      cmd('zoomOut', 'palette.zoomOut', 'minimize', () => setScale(stepped(scale.value, -1)), KEY_LABEL.zoomOut),
      cmd('zoomReset', 'palette.zoomReset', 'reset', () => setScale(1), KEY_LABEL.zoomReset),
      cmd('mute', 'act.muteHour', 'bellOff', () => call('notify_snooze', { minutes: 60 })),
      cmd('account', 'act.addAccount', 'plus', () => (mail.screen = 'onboarding')),
      cmd('settings', 'act.settings', 'settings', () => openSettings(), 'Ctrl ,'),
      cmd('rules', 'palette.rules', 'rules', () => openSettings('rules')),
      cmd('tags', 'tag.manage', 'tag', () => openSettings('tags')),
      cmd('templates', 'palette.templates', 'template', () => openSettings('compose')),
      cmd('scheduled', 'palette.scheduled', 'calendarClock', () => (mail.screen = 'scheduled')),
    );
    for (const a of mail.accounts) {
      const label = t('palette.accountSettings', { name: a.name });
      list.push({ key: `acc-${a.id}`, label, icon: 'user', run: done(`acc-${a.id}`, () => openSettings(`account:${a.id}`)), texts: [label, a.email] });
    }
    return list;
  });

  /** Eşleşen komutlar: puana, sonra kullanım sıklığına göre; boşken sık kullanılanlar. */
  const matched = $derived.by(() => {
    const order = new Map(commands.map((c, i) => [c.key, i]));
    if (!needle) {
      return [...commands].sort((a, b) => (use[b.key] ?? 0) - (use[a.key] ?? 0) || order.get(a.key)! - order.get(b.key)!).slice(0, 6);
    }
    return commands
      .map((c) => ({ c, s: score(c.texts, needle) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s || (use[b.c.key] ?? 0) - (use[a.c.key] ?? 0) || order.get(a.c.key)! - order.get(b.c.key)!)
      .slice(0, 5)
      .map((x) => x.c);
  });

  // ----------------------------------------------------------- tamamlama

  /** Tamamlama seçilince palet açık kalır, sorgu güncellenir. */
  function fill(next: string) {
    q = next;
    active = 0;
    input?.focus();
  }

  const DATES: Record<'after' | 'before', [DatePreset, MessageKey][]> = {
    after: [
      ['today', 'date.today'],
      ['week', 'date.week'],
      ['month', 'date.month'],
      ['last30', 'date.last30'],
    ],
    before: [
      ['today', 'date.beforeToday'],
      ['week', 'date.beforeWeek'],
      ['month', 'date.beforeMonth'],
    ],
  };

  const completions = $derived.by((): { title: MessageKey; items: Item[] } | null => {
    if (!comp) return null;
    const v = comp.value.toLocaleLowerCase('tr');
    if (comp.kind === 'from') {
      const seen = new Map<string, Address>();
      for (const m of [...found, ...mail.items]) {
        const a = m.from;
        const k = a.email.toLowerCase();
        if (seen.has(k)) continue;
        if (v && !`${a.name ?? ''} ${a.email}`.toLocaleLowerCase('tr').includes(v)) continue;
        seen.set(k, a);
      }
      const people = [...seen.values()].slice(0, 6);
      return {
        title: 'palette.people',
        items: people.map((p) => ({ key: `c-${p.email}`, label: displayName(p), meta: p.email, person: p, plain: true, run: () => fill(completeWith(q, 'from', p.email)) })),
      };
    }
    if (comp.kind === 'account') {
      return {
        title: 'chip.account',
        items: mail.accounts
          .filter((a) => !v || `${a.name} ${a.email}`.toLocaleLowerCase('tr').includes(v))
          .map((a) => ({ key: `a-${a.id}`, label: a.name, meta: a.email, icon: 'user' as IconName, plain: true, run: () => fill(completeWith(q, 'account', a.email)) })),
      };
    }
    const now = Date.now();
    const kind = comp.kind;
    return {
      title: 'palette.dates',
      items: DATES[kind].map(([p, label]) => {
        const day = presetDate(p, now);
        return { key: `d-${p}`, label: t(label), meta: `${kind}:${day}`, icon: 'calendar' as IconName, plain: true, run: () => fill(completeWith(q, kind, day)) };
      }),
    };
  });

  const sections = $derived.by(() => {
    const out: { title: MessageKey; items: Item[] }[] = [];
    const searchItem: Item = {
      key: 'search',
      label: t('palette.searchAll', { q: needle }),
      meta: t('palette.searchMeta'),
      icon: 'search',
      plain: true,
      run: () => {
        onclose();
        showList();
        searchAll(q);
      },
    };
    // Değeri yazılmamış işleçte ("from:") önce öneriler; yazılmışsa Enter arar.
    const early = completions && comp && !comp.value;
    if (early && completions.items.length) out.push(completions);
    if (needle && !early) out.push({ title: 'palette.search', items: [searchItem] });
    if (completions && !early && completions.items.length) out.push(completions);
    if (!comp && matched.length) out.push({ title: 'palette.actions', items: matched });
    // Arama işleçleri: tıklayınca sorguya eklenir, palet açık kalır.
    if (!comp && (!needle || /^\S*$/.test(needle))) {
      const ops: [string, MessageKey][] = [
        ['from:', 'palette.op.from'],
        ['has:ek', 'palette.op.attachment'],
        ['is:okunmamış', 'palette.op.unread'],
        ['in:arşiv', 'palette.op.in'],
        ['tag:', 'tag.op'],
        ['after:', 'palette.op.after'],
        ['before:', 'palette.op.before'],
        ...(mail.accounts.length > 1 ? ([['hesap:', 'palette.op.account']] as [string, MessageKey][]) : []),
      ];
      const lower = needle.toLocaleLowerCase('tr');
      const tips = ops
        .filter(([op]) => !lower || op.startsWith(lower))
        .map(([op, meta]) => ({
          key: `op-${op}`,
          label: op,
          meta: t(meta),
          icon: 'filter' as IconName,
          plain: true,
          run: () => fill(`${needle && !op.startsWith(lower) ? `${needle} ` : ''}${op}`),
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
          run: () => {
            onclose();
            void select(m.id);
            showReader();
          },
        })),
      });
    }
    if (needle && !comp) {
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
          items: people.map((p) => ({
            key: `p${p.email}`,
            label: displayName(p),
            meta: p.email,
            person: p,
            run: () => {
              onclose();
              showList();
              searchAll(`from:${p.email}`);
            },
          })),
        });
      }
    }
    return out;
  });

  function showList() {
    desk.reading = false;
  }

  const flat = $derived(sections.flatMap((s) => s.items));
  const total = $derived(flat.length);

  $effect(() => {
    if (active >= total) active = Math.max(0, total - 1);
  });

  $effect(() => {
    listEl?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  function split(label: string, plain?: boolean) {
    const r = plain ? null : matchRange(label, needle);
    if (!r) return { pre: label, mid: '', post: '' };
    return { pre: label.slice(0, r[0]), mid: label.slice(r[0], r[1]), post: label.slice(r[1]) };
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
    } else if (e.key === 'Tab' && !e.shiftKey && completions?.items.length) {
      // Tab ilk öneriyle tamamlar.
      e.preventDefault();
      completions.items[0].run();
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
      {#if q}<button class="btn-ghost" onclick={() => fill('')}>{t('act.clear')}</button>{/if}
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
            {#if it.meta}<span class="meta trunc item-meta">{it.meta}</span>{/if}
            {#if it.kbd}<span class="keys">{#each it.kbd.split(' ') as k, i (i)}<span class="kbd">{k}</span>{/each}</span>{/if}
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
      {#if completions?.items.length}<span class="hint"><span class="kbd">Tab</span>{t('palette.complete')}</span>{/if}
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
    max-height: min(560px, calc(100vh - 96px));
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

  /* Eşleşen parça renkle değil ağırlık ve alt çizgiyle vurgulanır. */
  mark {
    background: transparent;
    color: inherit;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .item-meta {
    flex: none;
    max-width: 220px;
  }

  .keys {
    flex: none;
    display: inline-flex;
    gap: 4px;
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
