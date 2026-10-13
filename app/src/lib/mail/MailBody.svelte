<script lang="ts">
  // Posta gövdesi. HTML betiksiz, CSP'li bir çerçevede; düz metin bağlantıları güvenle ayrılmış halde.
  import { openExternal } from '../api';
  import { linkify } from '../util/format';

  interface Props {
    text: string | null;
    html: string | null;
    remoteAllowed: boolean;
  }

  let { text, html, remoteAllowed }: Props = $props();

  let frame = $state<HTMLIFrameElement>();
  let height = $state(120);

  const srcdoc = $derived(
    html == null
      ? ''
      : `<!doctype html><html><head><meta charset="utf-8">` +
          `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: cid:${remoteAllowed ? ' https: http:' : ''}; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'">` +
          // İzin verilen uzak görsellerde bile uygulamayı ele veren Referer gitmesin.
          `<meta name="referrer" content="no-referrer">` +
          `<style>html,body{margin:0;background:#fff;color:#15171c}body{padding:20px 22px;font:15px/1.6 'Geist Variable',ui-sans-serif,system-ui,sans-serif;word-wrap:break-word;overflow-wrap:anywhere}` +
          `img{max-width:100%;height:auto}img[data-mh-src]:not([src]){background:#eef0f4;border-radius:6px}table{max-width:100%!important}a{color:#3a47b8}pre{white-space:pre-wrap}blockquote{margin:0 0 0 4px;padding-left:12px;border-left:1px solid #d2d6de;color:#4f5562}</style>` +
          `</head><body>${html}</body></html>`,
  );

  function measure() {
    const doc = frame?.contentDocument;
    if (!doc?.documentElement) return;
    height = Math.max(80, doc.documentElement.scrollHeight);
  }

  function onLoad() {
    const doc = frame?.contentDocument;
    if (!doc) return;
    measure();
    // Görseller geç yüklenebilir; birkaç kez yeniden ölç.
    doc.querySelectorAll('img').forEach((img) => img.addEventListener('load', measure));
    for (const ms of [120, 400, 1200]) setTimeout(measure, ms);
    doc.addEventListener('click', (e) => {
      const a = (e.target as Element | null)?.closest?.('a[href]');
      if (!a) return;
      e.preventDefault();
      void openExternal(a.getAttribute('href') ?? '');
    });
  }

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
  <div class="paper">
    <iframe
      bind:this={frame}
      title="posta"
      sandbox="allow-same-origin"
      referrerpolicy="no-referrer"
      {srcdoc}
      style:height="{height}px"
      onload={onLoad}
    ></iframe>
  </div>
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
  }

  iframe {
    display: block;
    width: 100%;
    border: 0;
    background: var(--mail-paper);
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
