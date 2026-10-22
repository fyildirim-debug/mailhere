<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { call, EVENTS, isTauri, on, toAppError, type NotifyAction, type NotifyEvent } from '../../lib/api';
  import { t } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import Logo from '../../lib/ui/Logo.svelte';
  import { displayName } from '../../lib/util/format';
  import { settings } from '../../lib/stores/settings.svelte';
  import { copyText } from '../../lib/mail/otp';
  import { NOTIFY_ACTION_INFO, shownNotifyActions } from '../../lib/notify-actions';
  import NotifyActs from './NotifyActs.svelte';
  import NotifyReply from './NotifyReply.svelte';

  /** Kartın hali: posta, yerinde yanıt ya da onay satırı ("Silindi · Geri al"). */
  type Mode = 'idle' | 'reply' | 'done';

  interface Done {
    text: string;
    sub: string;
    icon: IconName;
    /** Geri al; yoksa satır yalnız bilgi verir. */
    undo: (() => Promise<void>) | null;
    /** Kart bu süre sonra kapanır. */
    ms: number;
    /**
     * Süre gerçek bir saati izler (giden kutusundaki gönderim): üzerine
     * gelince durmaz, çünkü gönderim de durmaz.
     */
    fixed?: boolean;
  }

  interface Card extends NotifyEvent {
    key: number;
    at: number;
    mode: Mode;
    /** Yanıt taslağı: Vazgeç ve geri almada korunur, kart kapanınca gider. */
    draft: string;
    /** "Kime" satırı (Reply-To varsa o). */
    to: string;
    busy: boolean;
    error: string | null;
    done: Done | null;
  }

  const MAX = 3;
  /** Eylemden sonra "Geri al" bu kadar durur (toast gibi). */
  const UNDO_MS = 5000;
  /** Geri alınamayan bilgi satırı. */
  const INFO_MS = 4000;
  /** Gönderim bundan yakınsa "Geri al" sunulmaz (yetişmez). */
  const MIN_UNDO_MS = 1500;

  let cards = $state<Card[]>([]);
  let overflow = $state(0);
  let stack = $state<HTMLDivElement>();
  let now = $state(Date.now());
  let seq = 0;

  const fromTop = $derived(settings.value.cardPosition.startsWith('top'));
  const visible = $derived(fromTop ? [...cards].reverse() : cards);
  const actions = $derived(shownNotifyActions(settings.value.notifyActions));

  onMount(() => {
    document.documentElement.dataset.window = 'notify';
    const off = on(EVENTS.notify, (e) => add(e));
    // Arayüz kilitlenince ekrandaki kartlar da kapanır: gönderen ve konu
    // kilitli pencerenin yanında görünmez (yeni posta içeriksiz sistem bildirimiyle gelir).
    const offLock = on(EVENTS.lock, (s) => {
      if (s.uiLocked || s.locked) {
        cards = [];
        overflow = 0;
      }
    });
    const clock = setInterval(() => (now = Date.now()), 30_000);
    // Yanıt kutusu büyüdükçe pencere de büyür.
    const ro = new ResizeObserver(() => sync(false));
    if (stack) ro.observe(stack);
    // Pencere açılırken kuyruğa düşen kartları al; tarayıcıda örnek kartla başla.
    if (isTauri) void call('notify_pending').then((list) => list.forEach(add));
    else void call('demo_notify');
    return () => {
      off();
      offLock();
      ro.disconnect();
      clearInterval(clock);
    };
  });

  function add(e: NotifyEvent) {
    if (cards.some((c) => c.message.id === e.message.id)) return;
    const next: Card[] = [
      ...cards,
      { ...e, key: ++seq, at: Date.now(), mode: 'idle', draft: '', to: displayName(e.message.from), busy: false, error: null, done: null },
    ];
    // Fazlası özete düşer; yanıt yazılan ya da geri alma bekleyen kart düşmez.
    while (next.length > MAX) {
      const i = next.findIndex((c) => c.mode === 'idle' && !c.busy && !c.error);
      next.splice(i < 0 ? next.length - 1 : i, 1);
      overflow += 1;
    }
    cards = next;
  }

  function remove(key: number) {
    cards = cards.filter((c) => c.key !== key);
    if (!cards.length) overflow = 0;
  }

  // Pencereyi içeriğe göre boyutlandır; kart kalmayınca gizle. Görünmeyen
  // pencerede boyut gözlemcisi çalışmayabilir: kart sayısı ve hali değişince
  // doğrudan ölçülür.
  let lastHeight = 0;
  /** Bu pencere kart gösterdi mi: kartlar bitince pencere kapanır; açılırken (kartlar kuyruktan gelmeden) kapanmasın. */
  let shown = false;
  function sync(force: boolean) {
    void tick().then(() => {
      if (!isTauri) return;
      if (!cards.length) {
        if (!shown) return;
        shown = false;
        lastHeight = 0;
        void call('notify_hide');
        return;
      }
      shown = true;
      if (!stack) return;
      const height = Math.ceil(stack.getBoundingClientRect().height);
      if (!force && height === lastHeight) return;
      lastHeight = height;
      void call('notify_layout', { height });
    });
  }

  $effect(() => {
    void overflow;
    for (const c of cards) {
      void c.mode;
      void c.error;
      void c.done;
    }
    sync(true);
  });

  function ago(at: number) {
    const min = Math.floor((now - at) / 60_000);
    return min < 1 ? t('notify.now') : t('notify.minutesAgo', { n: min });
  }

  function about(c: Card) {
    return `${displayName(c.message.from)} · ${c.message.subject}`;
  }

  /** Klavye odağı kartın içindeyse (düğme kaybolacaksa) yeni hale taşır. */
  function keepFocus(key: number, selector: string) {
    const el = stack?.querySelector(`[data-card="${key}"]`);
    if (!el?.contains(document.activeElement)) return;
    void tick().then(() => el.querySelector<HTMLElement>(selector)?.focus());
  }

  async function open(c: Card) {
    remove(c.key);
    await call('open_quick', { id: c.message.id });
  }

  /** Simgeye (logo, avatar) ya da kartın boş yerine çift tık: ana pencere öne gelir, posta açılır. Düğme ve yazı alanı hariç. */
  function openInMain(c: Card, e: MouseEvent) {
    if ((e.target as Element | null)?.closest?.('button, a, input, textarea')) return;
    remove(c.key);
    void call('open_main', { id: c.message.id });
  }

  function run(c: Card, a: NotifyAction) {
    if (a === 'reply') startReply(c);
    else void act(c, a);
  }

  async function act(c: Card, a: Exclude<NotifyAction, 'reply'>) {
    if (c.busy) return;
    c.busy = true;
    c.error = null;
    try {
      await call('notify_action', { id: c.message.id, action: a });
      keepFocus(c.key, '.undo');
      c.done = { text: t(NOTIFY_ACTION_INFO[a].done!), sub: about(c), icon: 'check', undo: () => revert(c, a), ms: UNDO_MS };
      c.mode = 'done';
    } catch (e) {
      c.error = t('toast.error', { message: toAppError(e).message });
    } finally {
      c.busy = false;
    }
  }

  /** Eylemi geri alır: okundu → okunmadı, taşınan → gelen kutusu. */
  async function revert(c: Card, a: Exclude<NotifyAction, 'reply'>) {
    if (a === 'markRead') await call('messages_flag', { ids: [c.message.id], unread: true, starred: null });
    else await call('messages_move', { ids: [c.message.id], target: 'inbox' });
    keepFocus(c.key, '.btn-primary');
    c.done = null;
    c.mode = 'idle';
  }

  async function undo(c: Card) {
    const u = c.done?.undo;
    if (!u || c.busy) return;
    c.busy = true;
    try {
      await u();
    } catch (e) {
      keepFocus(c.key, '.btn-primary');
      c.done = null;
      c.mode = 'idle';
      c.error = t('toast.error', { message: toAppError(e).message });
    } finally {
      c.busy = false;
    }
  }

  function startReply(c: Card) {
    c.error = null;
    c.mode = 'reply';
    // Kart penceresi odak çalmadan açılır; yazabilmek için odak istenir.
    if (isTauri) void call('notify_focus').catch(() => {});
    void loadTargets(c);
  }

  /** Reply-To varsa "Kime" onu gösterir; gövde de bu arada iner (alıntı için). */
  async function loadTargets(c: Card) {
    try {
      const d = await call('message_get', { id: c.message.id });
      // Hatırlatma kartında gönderen yerine beklenen kişi yazar; ona dokunulmaz.
      const sameSender = d.from.email.toLowerCase() === c.message.from.email.toLowerCase();
      if (sameSender && d.replyTo.length) c.to = d.replyTo.map(displayName).join(', ');
    } catch {
      // Kime satırı gönderenle kalır; yanıtın alıcısını arka uç belirler.
    }
  }

  function cancelReply(c: Card) {
    keepFocus(c.key, '[data-action="reply"]');
    c.error = null;
    c.mode = 'idle';
  }

  async function sendReply(c: Card) {
    if (!c.draft.trim() || c.busy) return;
    c.busy = true;
    c.error = null;
    try {
      const item = await call('notify_reply', { id: c.message.id, text: c.draft });
      const left = item.sendAt - Date.now();
      const canUndo = left >= MIN_UNDO_MS;
      keepFocus(c.key, canUndo ? '.undo' : '.close');
      c.done = {
        text: t('notify.replyQueued'),
        sub: canUndo ? `${t('notify.replyGoes', { n: Math.round(left / 1000) })} · ${c.to}` : c.to,
        icon: 'send',
        undo: canUndo ? () => unsend(c, item.id) : null,
        ms: canUndo ? left : INFO_MS,
        fixed: canUndo,
      };
      c.mode = 'done';
    } catch (e) {
      // Metin kutuda kalır; hata altında yazar.
      c.error = t('toast.error', { message: toAppError(e).message });
    } finally {
      c.busy = false;
    }
  }

  /** Gönderimi geri alır; metin kutuya geri gelir. Yetişmezse söyler. */
  async function unsend(c: Card, outboxId: string) {
    if (await call('send_cancel', { id: outboxId })) {
      c.done = null;
      c.mode = 'reply';
      return;
    }
    keepFocus(c.key, '.close');
    c.done = { text: t('notify.undoLate'), sub: c.to, icon: 'send', undo: null, ms: INFO_MS };
  }

  /** Kodu kopyalanan kart: düğme kısa süre "Kod kopyalandı" der. */
  let copied = $state<number | null>(null);

  async function copyCode(c: Card) {
    if (!c.otp || !(await copyText(c.otp))) return;
    copied = c.key;
    setTimeout(() => {
      if (copied === c.key) copied = null;
    }, 2000);
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
    <div
      class="card"
      data-card={c.key}
      data-mode={c.mode}
      role="alertdialog"
      tabindex="-1"
      aria-label={t('notify.label', { name: displayName(c.message.from) })}
      title={c.mode === 'idle' ? t('notify.dblOpen') : undefined}
      ondblclick={(e) => openInMain(c, e)}
    >
      {#if c.mode === 'done' && c.done}
        {#key c.done}
          <div class="done" role="status">
            <span class="done-icon"><Icon name={c.done.icon} size={18} /></span>
            <span class="done-text">
              <span class="done-title">{c.done.text}</span>
              <span class="meta trunc">{c.done.sub}</span>
            </span>
            {#if c.done.undo}
              <button class="btn-soft small undo" disabled={c.busy} onclick={() => undo(c)}>{t('act.undo')}</button>
            {/if}
            <button class="btn-icon small close" aria-label={t('act.close')} title={t('act.close')} onclick={() => remove(c.key)}><Icon name="x" size={16} /></button>
          </div>
          <span class="bar now" class:fixed={c.done.fixed} style:--count="{c.done.ms}ms" onanimationend={() => remove(c.key)} aria-hidden="true"></span>
        {/key}
      {:else}
        <div class="head">
          <Logo size={16} dot={false} />
          <span class="meta trunc">{t('app.name')} · {c.accountName} · {ago(c.at)}</span>
          <span class="grow"></span>
          <button class="btn-icon small" aria-label={t('act.muteHour')} title={t('act.muteHour')} onclick={mute}><Icon name="bellOff" size={16} /></button>
          <button class="btn-icon small close" aria-label={t('act.close')} title={t('act.close')} onclick={() => remove(c.key)}><Icon name="x" size={16} /></button>
        </div>
        <div class="content">
          <Avatar address={c.message.from} size={40} />
          <div class="text">
            <span class="sender trunc">{displayName(c.message.from)}</span>
            <span class="subject trunc">{c.message.subject}</span>
            <span class="snippet">{c.message.snippet}</span>
          </div>
        </div>
        {#if c.mode === 'reply'}
          <NotifyReply
            to={c.to}
            bind:value={c.draft}
            busy={c.busy}
            error={c.error}
            onsend={() => void sendReply(c)}
            oncancel={() => cancelReply(c)}
          />
        {:else}
          {#if c.error}
            <p class="err" role="alert"><Icon name="alert" size={16} />{c.error}</p>
          {/if}
          <NotifyActs
            {actions}
            otp={c.otp}
            copied={copied === c.key}
            busy={c.busy}
            onopen={() => void open(c)}
            oncopy={() => void copyCode(c)}
            onaction={(a) => run(c, a)}
          />
          <!-- Hata yazan kart kendiliğinden kapanmaz. -->
          {#if !c.error}
            <span class="bar" style:--count="{settings.value.cardSeconds}s" onanimationend={() => remove(c.key)} aria-hidden="true"></span>
          {/if}
        {/if}
      {/if}
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
  /* Kartın gölgesi (--shadow-card) 20 px dolgunun içinde biter; saydam pencere kenarı onu kesmez. */
  .stack {
    width: 412px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 20px;
  }

  .card {
    position: relative;
    overflow: hidden;
    padding: 12px 12px 14px 14px;
    border-radius: var(--r-float);
    background: var(--raised);
    color: var(--ink);
    border: 1px solid var(--line);
    box-shadow: var(--shadow-card);
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

  /* Hata: ne oldu + ne yapılabilir (arka ucun metni); kart kalır. */
  .err {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 12px 0 0;
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--ink-2);
  }

  .err :global(.icon) {
    flex: none;
    color: var(--danger);
  }

  /* Onay satırı: ne oldu, hangi posta, Geri al ve Kapat. */
  .done {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto auto;
    align-items: center;
    column-gap: 10px;
    min-height: 40px;
    animation: fade var(--t-state) var(--ease) both;
  }

  .done-icon {
    display: grid;
    place-items: center;
    color: var(--ok);
  }

  .done-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 20px;
  }

  .done-title {
    font-weight: 500;
  }

  .done .close {
    margin-right: -2px;
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

  /* Onay satırının süresi hemen başlar. */
  .bar.now {
    animation-delay: 0s;
  }

  /*
   * Üzerine gelince geri sayım durur; kartın posta halinde klavye odağı da
   * durdurur. Onay satırında odak ("Geri al"a taşınır) durdurmaz: kart
   * süresi bitince kapanır. Giden kutusu süresi hiç durmaz.
   */
  .card:hover .bar:not(.fixed),
  .card:has(:focus-visible) .bar:not(.now) {
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
    box-shadow: var(--shadow-card);
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

  @keyframes fade {
    from {
      opacity: 0;
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
