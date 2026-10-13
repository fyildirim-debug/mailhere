<script module lang="ts">
  import type { Security } from '../api';

  export interface CertTarget {
    host: string;
    port: number;
    security: Security;
  }
</script>

<script lang="ts">
  // Güvenilmeyen sunucu sertifikası için onay penceresi.
  // Kullanıcı parmak izini görmeden güven verilmez. Odak pencerenin kendisindedir:
  // açılışı tetikleyen Enter tuşu bir düğmeye düşüp yanlışlıkla onay vermesin.
  import { call, toAppError, type CertInfo } from '../api';
  import { locale, t } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';

  interface Props {
    /** Sırayla denenir; sorunu olan ilk sertifika gösterilir. */
    targets: CertTarget[];
    onclose: (trusted: boolean) => void;
  }

  let { targets, onclose }: Props = $props();

  let info = $state<CertInfo | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let dialogEl = $state<HTMLDivElement>();

  $effect(() => {
    let alive = true;
    (async () => {
      let last: CertInfo | null = null;
      try {
        for (const target of targets) {
          last = await call('cert_inspect', target);
          if (last.problem) break;
        }
        if (alive) info = last;
      } catch (e) {
        if (alive) error = toAppError(e).message;
      }
    })();
    return () => {
      alive = false;
    };
  });

  $effect(() => {
    dialogEl?.focus();
  });

  function date(ms: number | null): string {
    return ms == null ? '—' : new Date(ms).toLocaleDateString(locale(), { day: 'numeric', month: 'long', year: 'numeric' });
  }

  async function trust() {
    if (!info || busy) return;
    busy = true;
    try {
      await call('cert_trust', { host: info.host, port: info.port, sha256: info.sha256 });
      onclose(true);
    } catch (e) {
      error = toAppError(e).message;
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose(false);
    }
  }
</script>

<div class="scrim" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose(false)}>
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="cert-title" tabindex="-1" bind:this={dialogEl} onkeydown={onKey}>
    <div class="head">
      <span class="badge"><Icon name="shield" size={20} /></span>
      <div class="grow">
        <h2 id="cert-title">{t('cert.title')}</h2>
        {#if info}
          <p class="lead">{t('cert.lead', { host: `${info.host}:${info.port}`, problem: info.problem || t('cert.noProblem') })}</p>
        {/if}
      </div>
    </div>

    {#if error}
      <p class="error" role="alert">{t('cert.failed', { message: error })}</p>
    {:else if !info}
      <p class="meta loading">{t('cert.loading')}</p>
    {:else}
      <dl>
        <dt>{t('cert.subject')}</dt><dd class="trunc" title={info.subject}>{info.subject}</dd>
        <dt>{t('cert.issuer')}</dt><dd class="trunc" title={info.issuer}>{info.issuer}</dd>
        <dt>{t('cert.valid')}</dt><dd>{date(info.notBefore)} – {date(info.notAfter)}</dd>
        {#if info.names.length}<dt>{t('cert.names')}</dt><dd class="trunc" title={info.names.join(', ')}>{info.names.join(', ')}</dd>{/if}
      </dl>
      <div class="fp">
        <span class="label">{t('cert.fingerprint')}</span>
        <!-- İki nokta sonrasında kırılır; kopyalanan metne fazladan karakter girmez. -->
        <code>{#each info.sha256.split(':') as part, i (i)}{#if i}:<wbr />{/if}{part}{/each}</code>
      </div>
      <p class="hint">{t('cert.compare')}</p>
    {/if}

    <div class="actions">
      <button class="btn-soft" onclick={() => onclose(false)}>{t('act.cancel')}</button>
      <button class="btn-primary" disabled={!info || busy || !!error} onclick={trust}>{t('cert.trust')}</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--ground) 60%, transparent);
    animation: fade-up var(--t-fast) ease-out;
  }

  .dialog {
    width: 520px;
    max-width: 100%;
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 24px;
    border-radius: var(--r-palette);
    background: var(--paper);
    border: 1px solid var(--line);
    box-shadow: var(--shadow-lg);
    animation: pop-in var(--t-state) var(--ease);
  }

  .dialog:focus {
    outline: none;
  }

  .head {
    display: flex;
    gap: 14px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: var(--r-row);
    background: color-mix(in srgb, var(--amber) 16%, transparent);
    color: var(--amber);
  }

  h2 {
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .lead {
    margin-top: 4px;
    color: var(--ink-2);
  }

  dl {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    column-gap: 16px;
    row-gap: 8px;
    margin: 0;
    padding: 14px 16px;
    border-radius: var(--r-card);
    background: var(--surface);
    border: 1px solid var(--line);
  }

  dt {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
    line-height: 22px;
  }

  dd {
    margin: 0;
    line-height: 22px;
  }

  .fp {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  code {
    padding: 10px 12px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    line-height: 1.6;
    user-select: all;
  }

  .hint {
    font-size: var(--fs-meta);
    color: var(--ink-2);
  }

  .loading {
    padding: 8px 0;
  }

  .error {
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
