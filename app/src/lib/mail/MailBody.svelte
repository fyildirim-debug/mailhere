<script lang="ts">
  // Posta gövdesi. HTML betiksiz, CSP'li bir çerçevede; düz metin bağlantıları güvenle ayrılmış halde.
  // Koyu temada HTML posta koyu temaya eşlenir (darkmail.ts): açık zeminler
  // koyulaşır, koyu yazı açılır, görseller ve koyu tasarlanmış alanlar kalır.
  // `original` ile posta kendi renklerine döner. Çerçevenin yükleyemediği
  // izinli uzak görseller (CORP başlığı) arka uçtan `data:` olarak alınır.
  import { onDestroy } from 'svelte';
  import { call, openExternal, type MessageId } from '../api';
  import { t } from '../i18n/index.svelte';
  import { darkMailWanted, settings } from '../stores/settings.svelte';
  import { linkify } from '../util/format';
  import { adaptDark } from './darkmail';
  import LinkConfirm from './LinkConfirm.svelte';

  interface Props {
    id: MessageId;
    text: string | null;
    html: string | null;
    remoteAllowed: boolean;
    /** Koyu temada postayı kendi renkleriyle göster ("Özgün renkler"). */
    original?: boolean;
  }

  let { id, text, html, remoteAllowed, original = $bindable(false) }: Props = $props();

  /** Bir çağrıda arka uca verilen en çok adres (arka uçla aynı). */
  const MAX_RESCUE = 40;

  let frame = $state<HTMLIFrameElement>();
  let height = $state(120);
  /** Uyarlama bitene kadar çerçeve görünmez (beyaz–koyu yanıp sönmesin). */
  let shown = $state(false);

  const wantDark = $derived(darkMailWanted(settings.value));
  /** Çerçeve koyu kağıtta mı? Ayardan bilinir; analiz beklenmez, yerleşim oynamaz. */
  const darkPaper = $derived(wantDark && !original);

  /** Çerçeve kendi belgesi olduğundan jetonların o anki değerleri aktarılır. */
  function token(name: string): string {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  }

  /** Açık kağıdın renkleri: posta kendi renkleriyle tasarlandığı için temadan bağımsız. */
  const LIGHT = { ink: '#15171c', ink2: '#4f5562', link: '#3a47b8', rule: '#d2d6de' };

  const srcdoc = $derived.by(() => {
    if (html == null) return '';
    const paper = token('--mail-paper');
    // Koyu kurallar yalnız koyu temada üretilir; `mh-dark` sınıfı uyarlamadan sonra eklenir.
    const dark = wantDark
      ? `html.mh-dark{color-scheme:dark}html.mh-dark,html.mh-dark body{background:${token('--paper')};color:${token('--ink')}}` +
        `html.mh-dark a{color:${token('--accent-text')}}html.mh-dark blockquote{border-left-color:${token('--line-strong')};color:${token('--ink-2')}}`
      : '';
    return (
      `<!doctype html><html><head><meta charset="utf-8">` +
      `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: cid:${remoteAllowed ? ' https: http:' : ''}; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'">` +
      // İzin verilen uzak görsellerde bile uygulamayı ele veren Referer gitmesin.
      `<meta name="referrer" content="no-referrer">` +
      `<style>:root{color-scheme:light}html,body{margin:0;background:${paper};color:${LIGHT.ink}}body{padding:20px 22px;font:15px/1.6 'Geist Variable',ui-sans-serif,system-ui,sans-serif;word-wrap:break-word;overflow-wrap:anywhere}` +
      // Engellenmiş görsel (adresi `data-mh-src`'de) yerini korur ama çizilmez (alt
      // metni ekran okuyucuda kalır);
      // Chromium kaynaksız görseli kırık simge, çerçeve ve alt metinle gösterirdi.
      `img{max-width:100%;height:auto}img[data-mh-src]:not([src]){opacity:0}table{max-width:100%!important}a{color:${LIGHT.link}}pre{white-space:pre-wrap}blockquote{margin:0 0 0 4px;padding-left:12px;border-left:1px solid ${LIGHT.rule};color:${LIGHT.ink2}}` +
      `${dark}</style>` +
      `</head><body>${html}</body></html>`
    );
  });

  // Yeni posta ya da tema: uyarlama yeniden yapılana kadar gizle.
  $effect(() => {
    void srcdoc;
    shown = false;
  });

  /** Son uyarlamanın geri alma işlevi (özgün renklere dönüş). */
  let undo: (() => void) | null = null;
  /** Çerçevedeki belgeye en son uygulanan kip (yüklemede ve geçişte bir kez yazılsın). */
  let appliedDark: boolean | null = null;

  /** Belgeyi ayara göre koyuya eşler ya da özgün haline döndürür. */
  function applyTheme(doc: Document) {
    undo?.();
    undo = null;
    appliedDark = darkPaper;
    const root = doc.documentElement;
    root.classList.remove('mh-dark');
    if (!darkPaper) return;
    // Okuma açık varsayılanlarla yapılır, sonra koyu sınıf eklenir.
    const back = adaptDark(doc, {
      ink: token('--ink'),
      ink2: token('--ink-2'),
      paper: token('--paper'),
      line: token('--line'),
      lineStrong: token('--line-strong'),
      lightLink: LIGHT.link,
    });
    // Eşlenemeyen (çok büyük) posta özgün renklerinde kalır.
    if (!back) return;
    undo = back;
    root.classList.add('mh-dark');
  }

  // "Özgün renkler" geçişi çerçeveyi yeniden yüklemez; yalnız renkler değişir.
  $effect(() => {
    const want = darkPaper;
    const doc = frame?.contentDocument;
    if (shown && doc?.body && appliedDark !== want) applyTheme(doc);
  });

  function measure() {
    const doc = frame?.contentDocument;
    if (!doc?.documentElement) return;
    height = Math.max(80, doc.documentElement.scrollHeight);
  }

  function onLoad() {
    const doc = frame?.contentDocument;
    if (!doc) return;
    appliedDark = null;
    applyTheme(doc);
    shown = true;
    measure();
    // Görseller geç yüklenebilir; birkaç kez yeniden ölç.
    doc.querySelectorAll('img').forEach((img) => img.addEventListener('load', measure));
    for (const ms of [120, 400, 1200]) setTimeout(measure, ms);
    doc.addEventListener('click', (e) => {
      const a = (e.target as Element | null)?.closest?.('a[href]');
      if (!a) return;
      e.preventDefault();
      const url = a.getAttribute('href') ?? '';
      // Metni başka bir adres gösteren bağlantı (arka uç işaretler): önce onay.
      const host = a.getAttribute('data-mh-host');
      if (host) confirmLink = { url, host };
      else void openExternal(url);
    });
    if (remoteAllowed) rescueImages(doc);
  }

  // ------------------------------------------------------ yüklenemeyen görseller

  /** Bu posta için indirilmiş görseller (adres → data:); tema geçişinde yeniden istenmez. */
  let rescued: { id: MessageId | null; map: Map<string, string> } = { id: null, map: new Map() };
  let rescueTimer: ReturnType<typeof setTimeout> | null = null;
  /** Yüklenemeyen görseller, adrese göre (aynı adres birden çok yerde olabilir). */
  let failed = new Map<string, HTMLImageElement[]>();

  function isHttp(url: string | null): url is string {
    return !!url && /^https?:\/\//i.test(url);
  }

  /**
   * Çerçevenin yükleyemediği izinli uzak görselleri toplar ve arka uçtan
   * ister. Yükleme bitmişse (`complete`) ve boyut yoksa görsel gelmemiştir;
   * sonradan hata verenler de aynı kuyruğa girer.
   */
  function rescueImages(doc: Document) {
    if (rescued.id !== id) rescued = { id, map: new Map() };
    failed = new Map();
    for (const img of doc.querySelectorAll('img')) {
      const url = img.getAttribute('src');
      if (!isHttp(url)) continue;
      const data = rescued.map.get(url);
      if (data) {
        img.src = data;
        continue;
      }
      if (img.complete) {
        if (img.naturalWidth === 0) note(url, img);
      } else {
        img.addEventListener(
          'error',
          () => {
            note(url, img);
            schedule();
          },
          { once: true },
        );
      }
    }
    schedule();
  }

  function note(url: string, img: HTMLImageElement) {
    const list = failed.get(url);
    if (list) list.push(img);
    else failed.set(url, [img]);
  }

  /** Kısa süre bekleyip tüm başarısız görselleri tek çağrıda ister. */
  function schedule() {
    if (rescueTimer) clearTimeout(rescueTimer);
    rescueTimer = setTimeout(() => void flush(), 150);
  }

  async function flush() {
    rescueTimer = null;
    if (!failed.size) return;
    const batch = failed;
    failed = new Map();
    const all = [...batch.keys()];
    const forId = id;
    // Arka uç çağrı başına en çok MAX_RESCUE adres alır; kalanlar sıradaki partide.
    for (let i = 0; i < all.length; i += MAX_RESCUE) {
      let got: { url: string; data: string }[];
      try {
        got = await call('remote_images', { id: forId, urls: all.slice(i, i + MAX_RESCUE) });
      } catch {
        // İnmeyen görsel alt metniyle kalır; posta yeniden açılınca yeniden denenir.
        return;
      }
      // Bu arada başka posta açıldıysa ya da okuma kapandıysa geç gelen yanıt yok sayılır.
      if (closed || rescued.id !== forId) return;
      for (const { url, data } of got) {
        rescued.map.set(url, data);
        for (const img of batch.get(url) ?? []) img.src = data;
      }
      measure();
    }
  }

  // ------------------------------------------------------ aldatıcı bağlantı

  let confirmLink = $state<{ url: string; host: string } | null>(null);

  function closeConfirm() {
    confirmLink = null;
    frame?.focus();
  }

  function openConfirmed() {
    const url = confirmLink?.url;
    closeConfirm();
    if (url) void openExternal(url);
  }

  let closed = false;
  onDestroy(() => {
    closed = true;
    if (rescueTimer) clearTimeout(rescueTimer);
  });

  const paragraphs = $derived(
    (text ?? '')
      .replace(/\r\n/g, '\n')
      .split(/\n{2,}/)
      .map((p) => p.replace(/\s+$/, ''))
      .filter((p) => p.trim().length > 0)
      .map((p) => ({ quote: p.split('\n').every((l) => l.startsWith('>')), parts: linkify(p) })),
  );
</script>

{#if html != null}
  <div class="paper" class:dark={darkPaper} class:pending={!shown}>
    <iframe
      bind:this={frame}
      title={t('read.frame')}
      sandbox="allow-same-origin"
      referrerpolicy="no-referrer"
      {srcdoc}
      style:height="{height}px"
      onload={onLoad}
    ></iframe>
  </div>
  {#if confirmLink}
    <LinkConfirm url={confirmLink.url} host={confirmLink.host} onopen={openConfirmed} onclose={closeConfirm} />
  {/if}
{:else}
  <div class="text selectable">
    {#each paragraphs as p, i (i)}
      <p class:quote={p.quote}>
        {#each p.parts as part, j (j)}{#if part.href}<a
              href={part.href}
              onclick={(e) => {
                e.preventDefault();
                void openExternal(part.href!);
              }}>{part.text}</a
            >{:else}{part.text}{/if}{/each}
      </p>
    {/each}
  </div>
{/if}

<style>
  .paper {
    border-radius: var(--r-card);
    background: var(--mail-paper);
    overflow: hidden;
    box-shadow: 0 0 0 1px var(--line);
    transition: opacity var(--t-fast);
  }

  .paper.dark {
    background: var(--paper);
  }

  .paper.pending {
    opacity: 0;
  }

  iframe {
    display: block;
    width: 100%;
    border: 0;
    background: transparent;
  }

  .text {
    max-width: 68ch;
    display: flex;
    flex-direction: column;
    gap: 14px;
    line-height: 1.65;
  }

  p {
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .quote {
    padding-left: 12px;
    border-left: 1px solid var(--line-strong);
    color: var(--ink-3);
  }

  a {
    color: var(--accent-text);
    text-underline-offset: 3px;
  }
</style>
