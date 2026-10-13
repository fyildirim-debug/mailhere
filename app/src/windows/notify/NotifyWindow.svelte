<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { call, EVENTS, isTauri, on, type NotifyEvent } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Logo from '../../lib/ui/Logo.svelte';
  import { displayName } from '../../lib/util/format';
  import { settings } from '../../lib/stores/settings.svelte';

  interface Card extends NotifyEvent {
    key: number;
    at: number;
  }

  const MAX = 3;
  let cards = $state<Card[]>([]);
  let overflow = $state(0);
  let stack = $state<HTMLDivElement>();
  let now = $state(Date.now());
  let seq = 0;

  const fromTop = $derived(settings.value.cardPosition.startsWith('top'));
  const visible = $derived(fromTop ? [...cards].reverse() : cards);

  onMount(() => {
    document.documentElement.dataset.window = 'notify';
    const off = on(EVENTS.notify, (e) => add(e));
    const clock = setInterval(() => (now = Date.now()), 30_000);
    // Pencere açılırken kuyruğa düşen kartları al; tarayıcıda örnek kartla başla.
    if (isTauri) void call('notify_pending').then((list) => list.forEach(add));
    else void call('demo_notify');
    return () => {
      off();
      clearInterval(clock);
    };
  });

  function add(e: NotifyEvent) {
    if (cards.some((c) => c.message.id === e.message.id)) return;
    const next = [...cards, { ...e, key: ++seq, at: Date.now() }];
    if (next.length > MAX) {
      overflow += next.length - MAX;
      next.splice(0, next.length - MAX);
    }
    cards = next;
  }

  function remove(key: number) {
    cards = cards.filter((c) => c.key !== key);
    if (!cards.length) overflow = 0;
  }

  // Pencereyi içeriğe göre boyutlandır; kart kalmayınca gizle.
  $effect(() => {
    void cards.length;
    void overflow;
    void tick().then(() => {
      if (!isTauri) return;
      if (!cards.length) void call('notify_hide');
      else if (stack) void call('notify_layout', { height: Math.ceil(stack.getBoundingClientRect().height) });
    });
  });

  function ago(at: number) {
    const min = Math.floor((now - at) / 60_000);
    return min < 1 ? t('notify.now') : t('notify.minutesAgo', { n: min });
  }

  async function open(c: Card) {
    remove(c.key);
    await call('open_quick', { id: c.message.id });
  }

  async function markRead(c: Card) {
    remove(c.key);
    await call('messages_flag', { ids: [c.message.id], unread: false, starred: null });
  }

  async function archive(c: Card) {
    remove(c.key);
    await call('messages_move', { ids: [c.message.id], target: 'archive' });
  }

  async function mute() {
    cards = [];
    overflow = 0;
    await call('notify_snooze', { minutes: 60 });
  }

  async function openInbox() {
    cards = [];
    overflow = 0;
    await call('open_main', { id: null });
  }
</script>

<div class="stack" class:top={fromTop} bind:this={stack}>
  {#if overflow > 0 && !fromTop}
    <button class="pill" onclick={openInbox}>
      <span class="pill-dot"></span><span class="grow">{t('notify.more', { n: overflow })}</span><span class="meta">{t('notify.openInbox')}</span>
    </button>
  {/if}

  {#each visible as c (c.key)}
    <div class="card" role="alertdialog" aria-label={t('notify.label', { name: displayName(c.message.from) })}>
      <div class="head">
        <Logo size={16} dot={false} />
        <span class="meta trunc">{t('app.name')} · {c.accountName} · {ago(c.at)}</span>
        <span class="grow"></span>
        <button class="btn-icon small" aria-label={t('act.muteHour')} title={t('act.muteHour')} onclick={mute}><Icon name="bellOff" size={16} /></button>
        <button class="btn-icon small" aria-label={t('act.close')} title={t('act.close')} onclick={() => remove(c.key)}><Icon name="x" size={16} /></button>
      </div>
      <div class="content">
        <Avatar address={c.message.from} size={40} />
        <div class="text">
          <span class="sender trunc">{displayName(c.message.from)}</span>
          <span class="subject trunc">{c.message.subject}</span>
          <span class="snippet">{c.message.snippet}</span>
        </div>
      </div>
      <div class="acts">
        <button class="btn-primary small" onclick={() => open(c)}>{t('act.open')}</button>
        <button class="btn-soft small" onclick={() => markRead(c)}>{t('act.markRead')}</button>
        <button class="btn-soft small" onclick={() => archive(c)}>{t('act.archive')}</button>
      </div>
      <span class="bar" style:--count="{settings.value.cardSeconds}s" onanimationend={() => remove(c.key)} aria-hidden="true"></span>
    </div>
  {/each}

  {#if overflow > 0 && fromTop}
    <button class="pill" onclick={openInbox}>
      <span class="pill-dot"></span><span class="grow">{t('notify.more', { n: overflow })}</span><span class="meta">{t('notify.openInbox')}</span>
    </button>
  {/if}

  {#if !isTauri && !cards.length}
    <button class="btn-soft replay" onclick={() => call('demo_notify')}><Icon name="bell" size={16} />{t('notify.replay')}</button>
  {/if}
</div>

<style>
  .stack {
    width: 400px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px;
  }

  .card {
    position: relative;
    overflow: hidden;
    padding: 12px 12px 14px 14px;
    border-radius: var(--r-float);
    background: var(--raised);
    color: var(--ink);
    border: 1px solid var(--line);
    box-shadow: var(--shadow-lg);
    animation: arrive var(--t-arrive) var(--ease) both;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    margin-bottom: 8px;
  }

  .grow {
    flex: 1;
  }

  .content {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr);
    column-gap: 12px;
    align-items: start;
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 20px;
  }

  .sender {
    font-weight: 600;
  }

  .subject {
    font-weight: 500;
  }

  .snippet {
    margin-top: 2px;
    color: var(--ink-2);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .acts {
    display: flex;
    gap: 8px;
    margin-top: 14px;
    padding-left: 52px;
  }

  .bar {
    position: absolute;
    left: 0;
    bottom: 0;
    width: 100%;
    height: 2px;
    background: var(--accent);
    transform-origin: left;
    animation-name: countdown;
    animation-duration: var(--count, 8s);
    animation-timing-function: linear;
    animation-delay: var(--t-arrive);
    animation-fill-mode: both;
  }

  .card:hover .bar,
  .card:focus-within .bar {
    animation-play-state: paused;
  }

  .pill {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    background: var(--raised);
    color: var(--ink-2);
    font-size: var(--fs-meta);
    font-weight: 500;
    text-align: left;
    box-shadow: var(--shadow);
    animation: arrive var(--t-arrive) var(--ease) both;
  }

  .pill:hover {
    color: var(--ink);
  }

  .pill-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }

  .replay {
    align-self: center;
    margin-top: 40px;
  }

  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateX(28px) scale(0.985);
      filter: blur(6px);
    }
    to {
      opacity: 1;
      transform: none;
      filter: blur(0);
    }
  }

  @keyframes countdown {
    from {
      transform: scaleX(1);
    }
    to {
      transform: scaleX(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .card,
    .pill {
      animation: none;
    }

    /* Geri sayım işlevseldir: süresi korunur, yalnız kayma kalkar. */
    .card .bar {
      animation-duration: var(--count, 8s) !important;
      animation-delay: 0s !important;
    }
  }
</style>
