<script lang="ts">
  import { onMount } from 'svelte';
  import {
    appWindow,
    call,
    EVENTS,
    on,
    pickFiles,
    toAppError,
    type Account,
    type Address,
    type Attachment,
    type ComposeMode,
    type MessageDetail,
    type MessageId,
    type OutgoingMessage,
  } from '../../lib/api';
  import { locale, t, when, type MessageKey } from '../../lib/i18n/index.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import FileBadge from '../../lib/ui/FileBadge.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import LockScreen from '../../lib/ui/LockScreen.svelte';
  import Menu, { type MenuItem } from '../../lib/ui/Menu.svelte';
  import Sheet from '../../lib/ui/Sheet.svelte';
  import { showToast } from '../../lib/ui/toast.svelte';
  import WindowControls from '../../lib/ui/WindowControls.svelte';
  import FollowUpMenu from '../../lib/mail/FollowUpMenu.svelte';
  import { menuKeys } from '../../lib/mail/menukeys';
  import PickTime from '../../lib/mail/PickTime.svelte';
  import { sendIssues, splitRecipients, type SendIssues } from '../../lib/mail/precheck';
  import { settings } from '../../lib/stores/settings.svelte';
  import { initLock, isLocked, lock } from '../../lib/stores/lock.svelte';
  import { isMobileShell } from '../../lib/shell';
  import { baseSubject, displayName, fileSize, snoozeOptions } from '../../lib/util/format';
  import { parseAddress } from '../../lib/util/recipients';
  import { fillVariables } from '../../lib/util/variables';
  import SendConfirm from './SendConfirm.svelte';

  type Field = 'to' | 'cc' | 'bcc';

  interface Props {
    mode: ComposeMode;
    messageId: number | null;
    /** Yeni postada önceden doldurulan alıcılar (kişi kartından). */
    initialTo?: Address[];
    /** Yeni postanın gönderen hesabı (kişi kartının postasının hesabı). */
    initialFrom?: string | null;
    /**
     * Metnin başına yazılacak hazır metin (mobil yanıt alanından "tam ekran
     * yaz"): değişiklik sayılır, taslağa kaydedilir; imleç sonunda durur.
     */
    initialBody?: string;
    /**
     * Başka bir ekranın içinde (mobil kabuk): pencere başlığı ve düğmeleri yok,
     * gönder düğmesi kapsayıcıda (`send()`), "sonra gönder" alt sayfada, kapanış
     * pencereyi değil `onclose`u çağırır; sökülürken bekleyen değişiklik taslağa yazılır.
     */
    embedded?: boolean;
    onclose?: () => void;
    /** Gömülü: gönderim giden kutusuna girdi; kapsayıcı ekranı kapatır ve geri alma toast'ı gösterir. */
    onsent?: (info: { id: string; sendAt: number; msg: OutgoingMessage }) => void;
  }

  let { mode, messageId, initialTo = [], initialFrom = null, initialBody = '', embedded = false, onclose, onsent }: Props = $props();

  let accounts = $state<Account[]>([]);
  let fromId = $state<string>('');
  let to = $state<Address[]>([]);
  let cc = $state<Address[]>([]);
  let bcc = $state<Address[]>([]);
  let showCc = $state(false);
  let showBcc = $state(false);
  let subject = $state('');
  let body = $state('');
  let attachments = $state<string[]>([]);
  let original = $state<MessageDetail | null>(null);
  let laterOpen = $state(false);
  let laterEl = $state<HTMLDivElement>();
  let moreBtn = $state<HTMLButtonElement>();
  let sendAt = $state<number | null>(null);
  /** Yanıt gelmezse hatırlatma anı; gönderimle birlikte arka uca gider. */
  let followUpAt = $state<number | null>(null);
  let queued = $state<{ id: string; at: number } | null>(null);
  let error = $state<string | null>(null);
  /** Hata, bu alıcı alanında kalan geçersiz parçayı söylüyor; alan düzenlenince kalkar. */
  let badField: Field | null = null;
  /** Gönderim öncesi onay bekleyen durumlar (unutulmuş ek, boş konu). */
  let confirm = $state<SendIssues | null>(null);
  /** Yanıtta ve iletmede özgün postadan gelen konu; ek sözcüğü aranırken sayılmaz. */
  let prefilledSubject = '';
  let bodyEl = $state<HTMLTextAreaElement>();
  let drafts = $state<Record<Field, string>>({ to: '', cc: '', bcc: '' });
  let suggest = $state<{ field: Field; items: Address[]; active: number } | null>(null);
  let suggestTimer: ReturnType<typeof setTimeout> | null = null;
  /** Gömülü (mobil): son yazışılan kişiler; Kime boşken ve odaktayken çip olarak. */
  let recentPeople = $state<Address[]>([]);
  let toFocused = $state(false);

  // Taslak: sunucudaki Taslaklar kutusuna kendiliğinden kaydedilir.
  let draftId = $state<MessageId | null>(null);
  /** İletilen (ya da yeniden açılan taslaktaki) postanın ekleri; gönderimde arka uç ekler. */
  let forwardFrom = $state<MessageId | null>(null);
  let carried = $state<Attachment[]>([]);
  let draftState = $state<'idle' | 'saving' | 'saved' | 'error'>('idle');
  let draftError = $state<string | null>(null);
  let baseline = '';
  let draftTimer: ReturnType<typeof setTimeout> | null = null;
  let saving: Promise<void> | null = null;
  let ready = $state(false);
  let discarded = false;

  const from = $derived(accounts.find((a) => a.id === fromId));
  /** Gönderim sunucuya gidiyor: ikinci Ctrl Enter ya da çift tık ikinci posta yaratmasın. */
  let sending = $state(false);
  const locked = $derived(queued != null || sending);
  const showRecent = $derived(embedded && toFocused && !to.length && !drafts.to.trim() && recentPeople.length > 0 && !locked);

  // İmza: gönderirken metnin sonuna eklenir. Hesap değişince o hesabın varsayılanı gelir.
  let sigId = $state<string | null>(null);
  $effect(() => {
    const a = from;
    sigId = a ? (mode === 'new' || mode === 'draft' ? a.signatureNew : a.signatureReply) : null;
  });
  const signature = $derived(from?.signatures.find((x) => x.id === sigId) ?? null);

  let menu = $state<{ at: { x: number; y: number }; title: string; items: MenuItem[] } | null>(null);

  function below(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left, y: r.bottom + 4 };
  }

  function signatureMenu(e: MouseEvent) {
    if (!from) return;
    menu = {
      at: below(e),
      title: t('sig.pick'),
      items: [
        { label: t('sig.noneOption'), checked: sigId == null, onclick: () => (sigId = null) },
        ...from.signatures.map((x) => ({ key: x.id, label: x.name || t('sig.unnamed'), checked: x.id === sigId, onclick: () => (sigId = x.id) })),
      ],
    };
  }

  /** Şablon değişkenleri: ilk alıcı ve gönderen hesap. */
  function variables() {
    return {
      recipient: to[0] ?? cc[0] ?? null,
      sender: from ? { name: from.displayName, email: from.email } : null,
      locale: locale(),
    };
  }

  /** Şablonu (değişkenleri doldurarak) imlecin olduğu yere ekler; konu boşsa şablonun konusunu yazar. */
  function insertTemplate(id: string) {
    const tpl = settings.value.templates.find((x) => x.id === id);
    if (!tpl) return;
    const ctx = variables();
    if (tpl.subject && (!subject.trim() || mode === 'new')) subject = fillVariables(tpl.subject, ctx);
    const text = fillVariables(tpl.body, ctx);
    const el = bodyEl;
    // İmleç konumu odak gitse de textarea'da kalır.
    const at = el ? el.selectionStart : 0;
    body = body.slice(0, at) + text + body.slice(at);
    queueMicrotask(() => {
      el?.focus();
      el?.setSelectionRange(at + text.length, at + text.length);
    });
  }

  function templateMenu(e: MouseEvent) {
    const list = settings.value.templates;
    menu = {
      at: below(e),
      title: t('tpl.insert'),
      items: list.length
        ? list.map((x) => ({ key: x.id, label: x.name, icon: 'template' as const, onclick: () => insertTemplate(x.id) }))
        : [{ label: t('tpl.noneCompose'), disabled: true, onclick: () => {} }],
    };
  }

  /** Yazma biter: pencere kapanır; gömülüyse kapsayıcı ekranı kapatır. */
  async function close() {
    if (embedded) onclose?.();
    else await appWindow.close();
  }

  /** Kapatırken taslak kaydı en çok bu kadar beklenir (ms); bitmezse pencere açık kalır. */
  const CLOSE_SAVE_WAIT = 5000;
  let closing = false;
  /** Kayıt olmadı ve kullanıcı uyarıldı: yeniden kapatırsa pencere kaydetmeden kapanır. */
  let closeWarned = false;

  /**
   * Başlık çubuğundaki kapat (X): bekleyen değişiklik taslağa yazıldıktan
   * SONRA pencere kapanır (`pagehide` anında başlatılan kayıt, pencere yok
   * olunca yarıda kalabiliyordu). Kayıt olmazsa pencere açık kalır ve nedenini
   * söyler; ikinci kapatış kaydetmeden kapatır.
   *
   * İşletim sisteminin kapatma yolu (Alt F4, görev çubuğu) buradan geçmez;
   * orada yine `pagehide` kaydı denenir.
   */
  async function closeWindow() {
    if (closing) return;
    if (discarded || locked || closeWarned || !dirty()) {
      await appWindow.close();
      return;
    }
    closing = true;
    if (draftTimer) clearTimeout(draftTimer);
    let timer: ReturnType<typeof setTimeout> | undefined;
    const slow = new Promise<'slow'>((resolve) => (timer = setTimeout(() => resolve('slow'), CLOSE_SAVE_WAIT)));
    const result = await Promise.race([saveDraft().then(() => 'done' as const), slow]);
    clearTimeout(timer);
    closing = false;
    if (result === 'done' && draftState !== 'error') {
      await appWindow.close();
      return;
    }
    closeWarned = true;
    error = result === 'slow' ? t('compose.closeSlow') : t('compose.closeUnsaved', { message: draftError ?? '' });
  }

  // Ayrı yazma penceresi arayüz kilitlenince örtülür: taslak bellekte kalır,
  // içerik görünmez ve etkileşimsizdir (`inert`); gömülü yazma (mobil) kabuğun
  // kilit ekranının altında kalır.
  const lockShown = $derived(!embedded && isLocked(lock.status));

  onMount(() => {
    if (!embedded) void initLock();
    void init();
    const off = on(EVENTS.outbox, (e) => {
      if (!queued || e.id !== queued.id) return;
      if (e.ok) void close();
      else {
        error = t('toast.sendFailed', { message: e.error ?? '' });
        queued = null;
      }
    });
    // Pencere kapanırken bekleyen değişikliği kaydetmeyi dene.
    const flush = () => {
      if (!discarded && !queued && dirty()) void saveDraft();
    };
    window.addEventListener('pagehide', flush);
    return () => {
      off();
      window.removeEventListener('pagehide', flush);
      if (draftTimer) clearTimeout(draftTimer);
      // Gömülü ekran geri tuşuyla kapandı: pencere kapanışıyla aynı, değişiklik taslağa.
      if (embedded && !discarded && !queued && dirty()) {
        void saveDraft().then(() => {
          if (draftState === 'saved') showToast({ text: t('mob.compose.draftSaved') });
        });
      }
    };
  });

  async function init() {
    accounts = await call('accounts_list');
    if (embedded) void loadRecentPeople();
    fromId = (accounts.find((a) => a.id === initialFrom) ?? accounts[0])?.id ?? '';
    if (messageId == null && initialTo.length) to = [...initialTo];
    if (messageId != null) {
      try {
        const d = await call('message_get', { id: messageId });
        fromId = d.accountId;
        if (mode === 'draft') openDraft(d);
        else {
          original = d;
          prefill(d);
        }
      } catch (e) {
        error = toAppError(e).message;
      }
    }
    baseline = snapshot();
    // Taşınan metin taban çizgisinden sonra yazılır: kapatınca taslakta kalır.
    const carriedText = initialBody.trimEnd();
    if (carriedText) body = carriedText + body;
    ready = true;
    if (!embedded) document.title = title();
    setTimeout(() => {
      if (to.length && bodyEl) {
        bodyEl.focus();
        bodyEl.setSelectionRange(carriedText.length, carriedText.length);
        bodyEl.scrollTop = 0;
      } else document.getElementById('compose-to')?.focus();
    });
  }

  function same(a: Address, b: Address) {
    return a.email.toLowerCase() === b.email.toLowerCase();
  }

  function prefill(d: MessageDetail) {
    const me = accounts.find((a) => a.id === d.accountId)?.email.toLowerCase();
    const notMe = (a: Address) => a.email.toLowerCase() !== me;
    const quoted = (d.bodyText ?? d.snippet)
      .split('\n')
      .map((l) => `> ${l}`)
      .join('\n');
    const header = t('compose.wrote', { date: when(d.date), name: displayName(d.from) });
    if (mode === 'forward') {
      subject = prefilledSubject = `Fwd: ${baseSubject(d.subject)}`;
      body = `\n\n${t('compose.forwarded')}\n${displayName(d.from)} <${d.from.email}>\n${when(d.date)}\n${d.subject}\n\n${d.bodyText ?? d.snippet}`;
      if (d.attachments.length) {
        forwardFrom = d.id;
        carried = d.attachments;
      }
      return;
    }
    subject = prefilledSubject = `Re: ${baseSubject(d.subject)}`;
    to = (d.replyTo.length ? d.replyTo : [d.from]).filter(notMe);
    if (!to.length) to = d.to.filter(notMe);
    if (mode === 'replyAll') {
      cc = [...d.to, ...d.cc].filter(notMe).filter((a) => !to.some((x) => same(x, a)));
      showCc = cc.length > 0;
    }
    body = `\n\n${header}\n${quoted}`;
  }

  /** Kayıtlı bir taslağı düzenlemek için açar. */
  function openDraft(d: MessageDetail) {
    draftId = d.id;
    to = [...d.to];
    cc = [...d.cc];
    bcc = [...d.bcc];
    showCc = cc.length > 0;
    showBcc = bcc.length > 0;
    subject = d.subject;
    body = d.bodyText ?? '';
    if (d.attachments.length) {
      forwardFrom = d.id;
      carried = d.attachments;
    }
  }

  function title() {
    if (mode === 'draft') return t('compose.draft', { subject: subject.trim() || t('compose.noSubject') });
    if (mode === 'forward' && original) return t('compose.forward', { subject: baseSubject(original.subject) });
    if (original) return t('compose.reply', { subject: baseSubject(original.subject) });
    return t('compose.new');
  }

  function outgoing(): OutgoingMessage | null {
    if (!from) return null;
    return {
      accountId: from.id,
      to: [...to],
      cc: [...cc],
      bcc: [...bcc],
      subject,
      bodyText: body,
      inReplyTo: original?.id ?? null,
      attachments: [...attachments],
      sendAt: null,
      forwardFrom,
      draftId,
      followUpAt,
    };
  }

  function snapshot(): string {
    return JSON.stringify([fromId, to, cc, bcc, subject, body, attachments, forwardFrom]);
  }

  function dirty(): boolean {
    return ready && snapshot() !== baseline;
  }

  // Her değişiklikten 1,5 sn sonra kaydet (yazarken her tuşta değil).
  $effect(() => {
    const snap = snapshot();
    if (!ready || locked || snap === baseline) return;
    // Yeni değişiklik: kapatırken yeniden kayıt denenir.
    closeWarned = false;
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(() => void saveDraft(), 1500);
  });

  async function saveDraft(): Promise<void> {
    if (saving) {
      await saving;
      if (!dirty()) return;
    }
    const msg = outgoing();
    if (!msg || discarded || queued) return;
    const snap = snapshot();
    draftState = 'saving';
    saving = (async () => {
      try {
        try {
          draftId = await call('draft_save', { msg });
        } catch (e) {
          // Taslak bu arada silinmiş olabilir (ör. gönderildi ve geri alındı): yenisini yaz.
          // Ekler o taslaktan geliyorsa artık okunamaz; onlarsız kaydet.
          if (msg.draftId == null || toAppError(e).code !== 'notFound') throw e;
          const lost = msg.forwardFrom != null && msg.forwardFrom === msg.draftId;
          if (lost) {
            forwardFrom = null;
            carried = [];
          }
          draftId = await call('draft_save', { msg: { ...msg, draftId: null, forwardFrom: lost ? null : msg.forwardFrom } });
        }
        baseline = snap;
        draftState = 'saved';
        draftError = null;
        // Kapatırken verilen "kaydedilemedi" uyarısı artık geçerli değil.
        if (closeWarned) {
          closeWarned = false;
          error = null;
        }
      } catch (e) {
        draftState = 'error';
        draftError = toAppError(e).message;
      } finally {
        saving = null;
      }
    })();
    await saving;
  }

  async function discard() {
    discarded = true;
    if (draftTimer) clearTimeout(draftTimer);
    if (saving) await saving;
    if (draftId != null) {
      try {
        await call('draft_delete', { id: draftId });
      } catch {
        /* zaten yoksa sorun değil */
      }
    }
    await close();
  }

  /** Alana yazılanı çiplere çevirir; adres olmayan parçalar alanda kalır ve döner. */
  function commit(field: Field): string[] {
    const bad: string[] = [];
    const list = field === 'to' ? to : field === 'cc' ? cc : bcc;
    for (const p of splitRecipients(drafts[field])) {
      const a = parseAddress(p);
      if (a && !list.some((x) => same(x, a))) {
        list.push(a);
        recipientAdded();
      } else if (!a) bad.push(p);
    }
    drafts[field] = bad.join(', ');
    return bad;
  }

  /** Alıcı eklenince "En az bir alıcı ekle." uyarısı kalkar (başka hata kalır). */
  function recipientAdded() {
    if (error === t('compose.needRecipient')) error = null;
  }

  function onRecipientKey(field: Field, e: KeyboardEvent) {
    const list = field === 'to' ? to : field === 'cc' ? cc : bcc;
    if (suggest?.field === field) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const n = suggest.items.length;
        suggest.active = (suggest.active + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        pick(field, suggest.items[suggest.active]);
        return;
      }
      if (e.key === 'Escape') {
        e.stopPropagation();
        suggest = null;
        return;
      }
    }
    if (e.key === 'Enter' || e.key === ',' || e.key === ';' || e.key === 'Tab') {
      if (drafts[field].trim()) {
        if (e.key !== 'Tab') e.preventDefault();
        commit(field);
      }
    } else if (e.key === 'Backspace' && !drafts[field] && list.length) {
      list.pop();
    }
  }

  function removeAt(field: Field, i: number) {
    (field === 'to' ? to : field === 'cc' ? cc : bcc).splice(i, 1);
  }

  /** Yazılan parçaya uyan adresleri geçmiş postalardan önerir. */
  function lookup(field: Field) {
    // Geçersiz parça düzeltiliyor: uyarı kalkar, gönderimde yeniden bakılır.
    if (badField === field) {
      badField = null;
      error = null;
    }
    if (suggestTimer) clearTimeout(suggestTimer);
    const q = drafts[field].trim();
    if (q.length < 2) {
      suggest = null;
      return;
    }
    suggestTimer = setTimeout(async () => {
      const needle = q.toLocaleLowerCase('tr');
      const taken = new Set([...to, ...cc, ...bcc].map((a) => a.email.toLowerCase()));
      const seen = new Map<string, Address>();
      for (const m of await call('search', { q, limit: 30 })) {
        for (const a of [m.from, ...m.to]) {
          const key = a.email.toLowerCase();
          const hay = `${a.name ?? ''} ${a.email}`.toLocaleLowerCase('tr');
          if (!taken.has(key) && !seen.has(key) && hay.includes(needle)) seen.set(key, a);
        }
      }
      const items = [...seen.values()].slice(0, 5);
      suggest = drafts[field].trim() === q && items.length ? { field, items, active: 0 } : null;
    }, 120);
  }

  /**
   * Son yazışılan 5 kişi: gönderilmiş postaların alıcıları (yeniden eskiye),
   * eksik kalırsa Kişiler kategorisindeki göndericiler. Hesabın kendi adresleri
   * atılır. Yalnız mevcut liste komutu kullanılır (yerel veritabanı).
   */
  async function loadRecentPeople() {
    const mine = new Set(accounts.map((a) => a.email.toLowerCase()));
    const seen = new Map<string, Address>();
    const add = (list: Address[]) => {
      for (const a of list) {
        const k = a.email.toLowerCase();
        if (seen.size < 5 && !mine.has(k) && !seen.has(k)) seen.set(k, a);
      }
    };
    try {
      const sent = await call('messages_list', { query: { view: { kind: 'sent' }, accountId: null, category: null, cursor: null, limit: 50 } });
      for (const m of sent.items) add(m.to);
      if (seen.size < 5) {
        const people = await call('messages_list', { query: { view: { kind: 'inbox' }, accountId: null, category: 'people', cursor: null, limit: 50 } });
        for (const m of people.items) add([m.from]);
      }
    } catch {
      /* öneri yoksa alan yine çalışır */
    }
    recentPeople = [...seen.values()];
  }

  function pick(field: Field, a: Address) {
    const list = field === 'to' ? to : field === 'cc' ? cc : bcc;
    if (!list.some((x) => same(x, a))) {
      list.push(a);
      recipientAdded();
    }
    drafts[field] = '';
    suggest = null;
    document.getElementById(`compose-${field}`)?.focus();
  }

  /** Seçili metni sarar (düz metin, okunur işaretlerle). */
  function wrap(before: string, after = before) {
    const el = bodyEl;
    if (!el || locked) return;
    const { selectionStart: s, selectionEnd: e } = el;
    const sel = body.slice(s, e);
    body = body.slice(0, s) + before + sel + after + body.slice(e);
    queueMicrotask(() => {
      el.focus();
      el.setSelectionRange(s + before.length, s + before.length + sel.length);
    });
  }

  function bullet() {
    const el = bodyEl;
    if (!el || locked) return;
    const s = el.selectionStart;
    const lineStart = body.lastIndexOf('\n', s - 1) + 1;
    body = body.slice(0, lineStart) + '- ' + body.slice(lineStart);
    queueMicrotask(() => {
      el.focus();
      el.setSelectionRange(s + 2, s + 2);
    });
  }

  async function attach() {
    const files = await pickFiles();
    attachments = [...attachments, ...files.filter((f) => !attachments.includes(f))];
  }

  function fileName(path: string) {
    return path.split(/[\\/]/).pop() ?? path;
  }

  /** "Sonra gönder" seçeneklerini açar (mobilde Gönder'e uzun basış). */
  export function openLater() {
    if (!locked) laterOpen = true;
  }

  const FIELD_LABELS: Record<Field, MessageKey> = { to: 'compose.to', cc: 'compose.cc', bcc: 'compose.bcc' };

  /** Gönderir (kapsayıcı da çağırır: mobilde düğme uygulama çubuğunda). Zamanlanmış gönderim de buradan geçer. */
  export async function send() {
    await deliver(false);
  }

  /**
   * Gönderimden önce: alıcı alanlarında adres olmayan parça kalmışsa durur ve
   * hangisi olduğunu söyler; ek unutulmuş görünüyorsa ya da konu boşsa onay
   * sorar (`confirmed`: onay verildi).
   */
  async function deliver(confirmed: boolean) {
    if (locked || confirm) return;
    error = null;
    badField = null;
    for (const field of ['to', 'cc', 'bcc'] as const) {
      const bad = commit(field);
      if (!bad.length) continue;
      badField = field;
      error = t('compose.badRecipient', { field: t(FIELD_LABELS[field]), part: bad[0] });
      document.getElementById(`compose-${field}`)?.focus();
      return;
    }
    if (!to.length && !cc.length && !bcc.length) {
      error = t('compose.needRecipient');
      return;
    }
    // Seçilen an bu arada geçmiş olabilir (pencere açık bekledi): sessizce hemen gönderilmez.
    if (sendAt != null && sendAt <= Date.now()) {
      sendAt = null;
      error = t('compose.schedulePast');
      return;
    }
    const msg = outgoing();
    if (!from || !msg) return;
    if (!confirmed) {
      const own = prefilledSubject ? subject.trim() !== prefilledSubject : baseSubject(subject) === subject.trim();
      confirm = sendIssues({ subject, body, hasAttachments: attachments.length + carried.length > 0, subjectIsOwn: own });
      if (confirm) return;
    }
    if (draftTimer) clearTimeout(draftTimer);
    sending = true;
    try {
      if (saving) await saving;
      const sig = signature?.body.trimEnd();
      const text = sig ? `${body.trimEnd()}\n\n${sig}` : body;
      const item = await call('send', { msg: { ...msg, bodyText: text, sendAt, draftId } });
      queued = { id: item.id, at: item.sendAt };
      laterOpen = false;
      // Gömülü: ekran kapanır; geri alınırsa içerik (imzasız gövde) taslak olarak yeniden açılır.
      if (embedded) onsent?.({ id: item.id, sendAt: item.sendAt, msg: { ...msg, sendAt, draftId } });
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      sending = false;
    }
  }

  /** Onay verildi: denetimler yeniden sorulmadan gönderilir. */
  function confirmSend() {
    confirm = null;
    void deliver(true);
  }

  /** Vazgeçildi: odak eksik olan yere gider (boş konu ya da ek düğmesi). */
  function cancelSend() {
    const target = confirm?.noSubject ? 'compose-subject' : 'compose-attach';
    confirm = null;
    queueMicrotask(() => document.getElementById(target)?.focus());
  }

  async function undo() {
    if (!queued) return;
    const ok = await call('send_cancel', { id: queued.id });
    if (ok) {
      queued = null;
      // Gönderimde taslak silinmiş olabilir; içerik kaybolmasın diye yeniden kaydet.
      baseline = '';
    }
  }

  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      if (!locked) void send();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      if (!locked && ready) void saveDraft();
    } else if (e.key === 'Escape' && laterOpen) {
      laterOpen = false;
      moreBtn?.focus();
    }
  }

  /**
   * "Sonra gönder" seçenekleri. Menü her açıldığında yeniden hesaplanır: uzun
   * süre açık kalan pencerede dünün "yarın sabah"ı (geçmiş bir an) seçilemez.
   */
  function laterOptions() {
    return snoozeOptions(Date.now())
      .filter((o) => o.key !== 'weekend')
      .map((o) => ({ ...o, label: `snooze.${o.key}` as MessageKey }));
  }

  function pickLater(at: number) {
    sendAt = at;
    laterOpen = false;
    if (!embedded) moreBtn?.focus();
  }

  // Açılır menü (masaüstü): odak ilk seçeneğe gider, ok tuşlarıyla gezilir;
  // dışarı tıklayınca kapanır. Alt sayfada (gömülü) odağı Sheet yönetir.
  $effect(() => {
    const el = laterEl;
    if (!laterOpen || embedded || !el) return;
    el.querySelector<HTMLButtonElement>('button')?.focus();
    const onDoc = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!el.contains(target) && !moreBtn?.contains(target)) laterOpen = false;
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  });

  const seconds = $derived(queued ? Math.max(1, Math.round((queued.at - Date.now()) / 1000)) : 0);
</script>

<svelte:window onkeydown={onKey} />

{#snippet laterItems()}
  {@const options = laterOptions()}
  {#each options as o (o.key)}
    <button class="menu-item" role="menuitem" onclick={() => pickLater(o.at)}>
      <Icon name="clock" size={embedded ? 20 : 16} />
      <span class="grow">{t(o.label)}</span>
      <span class="meta num">{when(o.at)}</span>
    </button>
  {/each}
  <PickTime
    row={t('compose.pickTime')}
    label={t('time.pickSend')}
    action={t('act.schedule')}
    initial={sendAt ?? options[0].at}
    iconSize={embedded ? 20 : 16}
    onpick={pickLater}
  />
  <FollowUpMenu
    embedded
    current={followUpAt}
    onpick={(at) => ((followUpAt = at), (laterOpen = false))}
    onclear={followUpAt != null ? () => ((followUpAt = null), (laterOpen = false)) : undefined}
    onclose={() => (laterOpen = false)}
  />
{/snippet}

<div class="win" class:embedded class:veiled={lockShown}>
  {#if lockShown}
    <div class="lockveil">
      <header class="titlebar" data-tauri-drag-region>
        <span class="title trunc" data-tauri-drag-region>{t('compose.new')}</span>
        <span class="grow" data-tauri-drag-region></span>
        <WindowControls height={40} />
      </header>
      <LockScreen compact />
    </div>
  {/if}
  {#if !embedded}
    <header class="titlebar" data-tauri-drag-region inert={lockShown}>
      <span class="title trunc" data-tauri-drag-region>{title()}</span>
      <span class="grow" data-tauri-drag-region></span>
      <WindowControls height={40} onclose={closeWindow} />
    </header>
  {/if}

  <div class="fields">
    <div class="field">
      <label class="flabel" for="compose-from">{t('compose.from')}</label>
      <select id="compose-from" class="from" bind:value={fromId} disabled={locked || accounts.length < 2}>
        {#each accounts as a (a.id)}
          <option value={a.id}>{a.displayName} · {a.email}</option>
        {/each}
      </select>
      {#if from}<span class="acc-dot" style:background={from.color}></span>{/if}
    </div>

    {#each [{ key: 'to', label: 'compose.to', list: to, show: true }, { key: 'cc', label: 'compose.cc', list: cc, show: showCc }, { key: 'bcc', label: 'compose.bcc', list: bcc, show: showBcc }] as row (row.key)}
      {#if row.show}
        {@const field = row.key as Field}
        <div class="field wrap">
          <label class="flabel" for="compose-{field}">{t(row.label as MessageKey)}</label>
          {#each row.list as a, i (a.email)}
            <span class="chip" title={a.email}>
              <Avatar address={a} size={20} />
              <span>{displayName(a)}</span>
              <button aria-label={t('compose.removeRecipient', { name: displayName(a) })} disabled={locked} onclick={() => removeAt(field, i)}>
                <Icon name="x" size={12} />
              </button>
            </span>
          {/each}
          <input
            id="compose-{field}"
            class="rinput"
            placeholder={row.list.length ? '' : t('compose.addRecipient')}
            bind:value={drafts[field]}
            oninput={() => lookup(field)}
            onkeydown={(e) => onRecipientKey(field, e)}
            onfocus={() => field === 'to' && (toFocused = true)}
            onblur={() => {
              if (field === 'to') toFocused = false;
              setTimeout(() => {
                if (suggest?.field === field) suggest = null;
                commit(field);
              }, 120);
            }}
            role="combobox"
            aria-expanded={suggest?.field === field}
            aria-controls="suggest-{field}"
            aria-autocomplete="list"
            disabled={locked}
            autocomplete="email"
          />
          {#if suggest?.field === field}
            <div class="popover suggest" id="suggest-{field}" role="listbox">
              {#each suggest.items as a, i (a.email)}
                <button
                  class="menu-item"
                  role="option"
                  aria-selected={i === suggest.active}
                  class:active={i === suggest.active}
                  onmousedown={(e) => {
                    e.preventDefault();
                    pick(field, a);
                  }}
                >
                  <Avatar address={a} size={24} />
                  <span class="trunc">{displayName(a)}</span>
                  <span class="meta trunc">{a.email}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if field === 'to'}
            {#if isMobileShell}
              <!-- Mobilde tek ikon düğme: çip gelince ikinci satır açmaz. -->
              {#if !showCc || !showBcc}
                <button class="btn-icon ccbcc" aria-label={t('compose.ccBcc')} title={t('compose.ccBcc')} onclick={() => ((showCc = true), (showBcc = true))}>
                  <Icon name="chevronDown" />
                </button>
              {/if}
            {:else}
              {#if !showCc}<button class="btn-ghost" onclick={() => (showCc = true)}>{t('compose.cc')}</button>{/if}
              {#if !showBcc}<button class="btn-ghost" onclick={() => (showBcc = true)}>{t('compose.bcc')}</button>{/if}
            {/if}
          {/if}
        </div>
        {#if field === 'to' && showRecent}
          <!-- Basış alandan odağı almaz: çipler kaybolmadan dokunuş yerine ulaşır. -->
          <div class="recent" role="group" aria-label={t('mob.compose.recent')}>
            {#each recentPeople as a (a.email)}
              <button type="button" class="rchip" title={a.email} onmousedown={(e) => e.preventDefault()} onclick={() => pick('to', a)}>
                <Avatar address={a} size={24} />
                <span class="trunc">{displayName(a)}</span>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    {/each}

    <div class="field">
      <label class="flabel" for="compose-subject">{t('compose.subject')}</label>
      <input id="compose-subject" class="rinput" bind:value={subject} disabled={locked} />
    </div>
  </div>

  <div class="tools">
    <button class="btn-icon small" aria-label={t('compose.bold')} title={t('compose.bold')} onclick={() => wrap('**')}><Icon name="bold" size={16} /></button>
    <button class="btn-icon small" aria-label={t('compose.italic')} title={t('compose.italic')} onclick={() => wrap('_')}><Icon name="italic" size={16} /></button>
    <button class="btn-icon small" aria-label={t('compose.link')} title={t('compose.link')} onclick={() => wrap('[', '](https://)')}><Icon name="link" size={16} /></button>
    <button class="btn-icon small" aria-label={t('compose.bullet')} title={t('compose.bullet')} onclick={bullet}><Icon name="list" size={16} /></button>
    <span class="vsep"></span>
    <button id="compose-attach" class="btn-icon small" aria-label={t('act.attach')} title={t('act.attach')} onclick={attach} disabled={locked}><Icon name="paperclip" size={16} /></button>
    <button class="btn-icon small" aria-label={t('tpl.insert')} title={t('tpl.insert')} aria-haspopup="menu" onclick={templateMenu} disabled={locked}><Icon name="template" size={16} /></button>
    <span class="grow"></span>
    {#if from?.signatures.length}
      {@const sigLabel = t('compose.signature', { name: signature ? signature.name || t('sig.unnamed') : t('sig.noneOption') })}
      <button class="btn-ghost sig-pick" aria-haspopup="menu" aria-label={sigLabel} title={sigLabel} disabled={locked} onclick={signatureMenu}>
        <Icon name="pen" size={14} /><span class="sig-lbl">{sigLabel}</span><Icon name="chevronDown" size={14} />
      </button>
    {/if}
  </div>

  <div class="editor">
    <textarea bind:this={bodyEl} bind:value={body} placeholder={t('compose.bodyPlaceholder')} aria-label={t('compose.body')} disabled={locked}></textarea>
    {#if signature?.body.trim()}<div class="signature">{signature.body}</div>{/if}
    {#if attachments.length || carried.length}
      <div class="files">
        {#each carried as a (a.index)}
          <span class="file" title="{a.name} · {t('compose.original')}">
            <FileBadge name={a.name} />
            <span class="trunc">{a.name}</span>
            <span class="meta num size">{fileSize(a.size, locale())}</span>
          </span>
        {/each}
        {#each attachments as path (path)}
          <span class="file" title={path}>
            <FileBadge name={fileName(path)} />
            <span class="trunc">{fileName(path)}</span>
            <button class="btn-icon small" aria-label={t('compose.removeAttachment')} disabled={locked} onclick={() => (attachments = attachments.filter((p) => p !== path))}>
              <Icon name="x" size={12} />
            </button>
          </span>
        {/each}
      </div>
    {/if}
  </div>

  <footer class="foot">
    {#if queued}
      <span class="status"><Icon name="check" size={14} />{t('compose.queued', { n: seconds })}<button class="link" onclick={undo}>{t('act.undo')}</button></span>
    {:else if error}
      <span class="status err" role="alert">{error}</span>
    {:else if sendAt}
      <span class="status"><Icon name="clock" size={14} />{t('compose.scheduled', { when: when(sendAt) })}<button class="link" onclick={() => (sendAt = null)}>{t('act.cancel')}</button></span>
    {:else if draftState === 'saving'}
      <span class="meta" role="status">{t('compose.draftSaving')}</span>
    {:else if draftState === 'saved'}
      <span class="meta" role="status">{t('compose.draftSaved')}</span>
    {:else if draftState === 'error'}
      <span class="status err" role="alert">{t('compose.draftFailed', { message: draftError ?? '' })}</span>
    {/if}
    {#if followUpAt != null && !queued}
      <span class="status follow" role="status"><Icon name="followUp" size={14} />{t('follow.composeStatus', { when: when(followUpAt) })}<button class="link" onclick={() => (followUpAt = null)}>{t('act.cancel')}</button></span>
    {/if}
    <span class="grow"></span>
    <button class="btn-icon" aria-label={t('compose.discard')} title={t('compose.discard')} disabled={locked} onclick={discard}><Icon name="trash" /></button>
    {#if embedded}
      <!-- Gönder kapsayıcının uygulama çubuğunda; "sonra gönder" ve hatırlatma alt sayfada. -->
      <button class="btn-soft later-btn" class:on={sendAt != null} aria-haspopup="menu" aria-expanded={laterOpen} disabled={locked} onclick={() => (laterOpen = true)}>
        <Icon name="clock" size={18} />{t('compose.later')}
      </button>
      {#if laterOpen}
        <Sheet title={t('compose.later')} onclose={() => (laterOpen = false)}>
          <div class="sheet-later" role="menu" tabindex="-1" aria-label={t('compose.later')} bind:this={laterEl} onkeydown={(e) => menuKeys(e, laterEl)}>
            {@render laterItems()}
          </div>
        </Sheet>
      {/if}
    {:else}
      <div class="split">
        <button class="send" onclick={send} disabled={locked}><Icon name="send" size={16} />{sendAt ? t('act.schedule') : t('act.send')}<span class="kbd on-accent">Ctrl Enter</span></button>
        <button class="more" aria-label={t('compose.later')} title={t('compose.later')} aria-haspopup="menu" aria-expanded={laterOpen} disabled={locked} bind:this={moreBtn} onclick={() => (laterOpen = !laterOpen)}>
          <Icon name="chevronDown" size={16} />
        </button>
        {#if laterOpen}
          <div class="popover later" role="menu" tabindex="-1" aria-label={t('compose.later')} bind:this={laterEl} onkeydown={(e) => menuKeys(e, laterEl)}>
            <div class="menu-title">{t('compose.later')}</div>
            {@render laterItems()}
          </div>
        {/if}
      </div>
    {/if}
  </footer>
</div>

{#if menu}<Menu at={menu.at} title={menu.title} items={menu.items} onclose={() => (menu = null)} />{/if}
{#if confirm}<SendConfirm issues={confirm} scheduled={sendAt != null} onconfirm={confirmSend} oncancel={cancelSend} />{/if}

<style>
  .win {
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--paper);
    /* Dar kapsayıcıda (mobil ekran, dar pencere) alanlar tek sütuna yaklaşır. */
    container-type: inline-size;
  }

  /* Kilit örtüsü pencerenin tamamını kaplar; altındaki içerik görünmez ve etkileşimsiz. */
  .win.veiled {
    position: relative;
  }

  .win.veiled > :not(.lockveil) {
    visibility: hidden;
  }

  .lockveil {
    position: absolute;
    inset: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    background: var(--paper);
  }

  .win.embedded {
    flex: 1;
    min-height: 0;
    height: auto;
  }

  .titlebar {
    flex: none;
    height: 40px;
    display: flex;
    align-items: center;
    padding-left: 16px;
    background: var(--ground);
    border-bottom: 1px solid var(--line);
  }

  .title {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--ink-2);
  }

  .grow {
    flex: 1;
  }

  .fields {
    flex: none;
    padding: 0 20px;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    border-bottom: 1px solid var(--line);
  }

  .field.wrap {
    position: relative;
    flex-wrap: wrap;
    padding: 6px 0;
  }

  .suggest {
    top: calc(100% - 2px);
    left: 56px;
    width: 380px;
  }

  /* Son yazışılan kişiler (mobil): Kime satırının altında yatay kayan çipler. */
  .recent {
    display: flex;
    gap: 8px;
    padding: 8px 0 10px;
    overflow-x: auto;
    scrollbar-width: none;
    border-bottom: 1px solid var(--line);
  }

  .recent::-webkit-scrollbar {
    display: none;
  }

  .rchip {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 200px;
    min-height: 40px;
    padding: 0 14px 0 6px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: var(--paper);
    color: var(--ink);
    font-weight: 500;
    transition: background var(--t-fast);
  }

  .rchip:active {
    background: var(--hover);
  }

  .suggest .active {
    background: var(--sel);
  }

  .flabel {
    width: 56px;
    flex: none;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .from {
    flex: 1;
    min-width: 0;
    height: 32px;
    border: 0;
    background: transparent;
    color: var(--ink);
    outline: none;
    appearance: none;
  }

  .acc-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .rinput {
    flex: 1;
    min-width: 120px;
    height: 32px;
    border: 0;
    outline: none;
    background: transparent;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
  }

  .chip button {
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--ink-3);
  }

  .chip button:hover {
    background: var(--hover);
    color: var(--ink);
  }

  /* Mobil: Bilgi ve Gizli'yi birlikte açan düğme satırın sağ üstünde durur. */
  .ccbcc {
    margin-left: auto;
    align-self: flex-start;
  }

  /* Taşma güvenlik ağı: sığmayan araç çubuğu kırpılmaz, yatay kayar. */
  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 14px;
    border-bottom: 1px solid var(--line);
    overflow-x: auto;
    scrollbar-width: none;
  }

  .sig-pick {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
    white-space: nowrap;
  }

  .vsep {
    width: 1px;
    height: 18px;
    margin: 0 6px;
    background: var(--line-strong);
  }

  .editor {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 20px 8px;
    overflow-y: auto;
  }

  textarea {
    flex: 1;
    min-height: 160px;
    border: 0;
    outline: none;
    resize: none;
    background: transparent;
    line-height: 1.65;
  }

  .signature {
    color: var(--ink-3);
    white-space: pre-line;
  }

  .files {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .file {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    max-width: 320px;
    padding: 4px 4px 4px 4px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
  }

  .file .size {
    padding-right: 8px;
  }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px 14px 20px;
    border-top: 1px solid var(--line);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .status :global(.icon) {
    color: var(--ok);
  }

  .status.follow :global(.icon) {
    color: var(--accent-text);
  }

  .status .link {
    font-size: var(--fs-meta);
  }

  .err {
    color: var(--danger);
  }

  .split {
    position: relative;
    display: flex;
  }

  .send {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 12px 0 14px;
    border: 0;
    border-radius: var(--r-btn) 0 0 var(--r-btn);
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 500;
  }

  .more {
    width: 34px;
    height: 36px;
    display: grid;
    place-items: center;
    border: 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-ink) 25%, transparent);
    border-radius: 0 var(--r-btn) var(--r-btn) 0;
    background: var(--accent);
    color: var(--accent-ink);
  }

  .send:hover:not(:disabled),
  .more:hover:not(:disabled) {
    filter: brightness(1.07);
  }

  .send:disabled,
  .more:disabled {
    opacity: var(--dim-disabled);
  }

  .on-accent {
    background: transparent;
    border-color: color-mix(in srgb, var(--accent-ink) 35%, transparent);
    color: inherit;
  }

  .later {
    right: 0;
    bottom: 44px;
    width: 300px;
  }

  .later-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .later-btn.on {
    color: var(--accent-text);
  }

  .sheet-later .menu-item {
    padding: 0 12px;
    gap: 14px;
  }

  .sheet-later :global(.embedded) {
    margin-top: 6px;
    padding-top: 6px;
  }

  /* Dar kapsayıcı: etiketler kısa, öneri listesi tam genişlik, araç çubuğu kayar, alt çubuk sarar. */
  @container (max-width: 520px) {
    .fields {
      padding: 0 12px;
    }

    .flabel {
      width: 44px;
    }

    .suggest {
      left: 0;
      right: 0;
      width: auto;
    }

    /* Tek alıcı satırı ikiye bölmesin: çip, sabit öğelere (etiket 44, yazma
       alanı 72, Bilgi/Gizli düğmesi 44, üç aralık 24) yer bırakacak kadar
       kısalır; ad üç noktayla biter, tam adres `title`da. */
    .chip {
      height: 32px;
      min-width: 0;
      max-width: calc(100% - 184px);
    }

    .chip > span {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .chip button {
      flex: none;
      width: 28px;
      height: 28px;
    }

    .rinput {
      min-width: 72px;
    }

    .tools {
      padding: 6px 8px;
      overflow-x: auto;
      scrollbar-width: none;
    }

    .editor {
      padding: 14px 12px 8px;
    }

    .foot {
      flex-wrap: wrap;
      row-gap: 6px;
      padding: 10px 12px calc(12px + env(safe-area-inset-bottom, 0px));
    }

    .status {
      flex: 1 1 100%;
      min-width: 0;
    }
  }

  /* Telefon genişliğinde imza seçici yalnız ikon: etiketle çubuk 415 px ister,
     393 ve 360 px'te kırpılırdı. Tam ad title ve erişilebilir adda. */
  @media (max-width: 420px) {
    .tools .sig-lbl {
      display: none;
    }

    .sig-pick {
      width: 40px;
      justify-content: center;
      padding: 0;
    }
  }
</style>
