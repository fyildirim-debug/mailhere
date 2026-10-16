<script lang="ts">
  // Aldatıcı bağlantı onayı: bağlantının metni bir adres gösteriyor ama gerçek
  // hedef başka bir site (arka uç `data-mh-host` ile işaretler). Varsayılan ve
  // birincil eylem vazgeçmektir; Esc, dışarı tıklama ve Android geri tuşu
  // da vazgeçer (mobil kabukta alt sayfa katmanına bağlıdır).
  import { onMount } from 'svelte';
  import { t } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';
  import { sheetClosed, sheetLayerActive, sheetOpened } from '../ui/sheet-layer';

  interface Props {
    url: string;
    host: string;
    onopen: () => void;
    onclose: () => void;
  }

  let { url, host, onopen, onclose }: Props = $props();

  let cancel = $state<HTMLButtonElement>();
  let box = $state<HTMLDivElement>();

  sheetOpened();

  onMount(() => {
    cancel?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      onclose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      sheetClosed();
    };
  });

  $effect(() => {
    if (!sheetLayerActive()) onclose();
  });

  /**
   * Pencere belgenin köküne taşınır: okuma ekranı kaydırma için dönüşümlü
   * (transform) bir kapta durur ve `position: fixed` o kaba göre yerleşirdi.
   */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  /** Sekme tuşu iki düğme arasında döner. */
  function onTab(e: KeyboardEvent) {
    if (e.key !== 'Tab' || !box) return;
    const list = [...box.querySelectorAll<HTMLElement>('button')];
    const i = list.indexOf(document.activeElement as HTMLElement);
    const next = e.shiftKey ? (i <= 0 ? list.length - 1 : i - 1) : (i + 1) % list.length;
    e.preventDefault();
    list[next]?.focus();
  }
</script>

<div class="scrim" role="presentation" use:portal onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="link-confirm-title" aria-describedby="link-confirm-lead" tabindex="-1" bind:this={box} onkeydown={onTab}>
    <div class="head">
      <span class="badge"><Icon name="alert" size={20} /></span>
      <div class="grow">
        <h2 id="link-confirm-title">{t('read.linkTitle')}</h2>
        <p id="link-confirm-lead" class="lead">{t('read.linkLead', { host })}</p>
      </div>
    </div>
    <p class="target selectable">{url}</p>
    <div class="actions">
      <button class="btn-soft" onclick={onopen}><Icon name="external" size={16} />{t('read.linkOpen')}</button>
      <button class="btn-primary" bind:this={cancel} onclick={onclose}>{t('act.cancel')}</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px 16px;
    background: color-mix(in srgb, var(--ground) 60%, transparent);
    animation: fade-up var(--t-fast) var(--ease);
  }

  .dialog {
    width: min(460px, 100%);
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
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
    letter-spacing: -0.015em;
  }

  .lead {
    margin-top: 4px;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }

  .target {
    padding: 10px 12px;
    border-radius: var(--r-row);
    background: var(--surface);
    border: 1px solid var(--line);
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    line-height: 1.5;
    color: var(--ink-2);
    overflow-wrap: anywhere;
    max-height: 120px;
    overflow-y: auto;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }

  /* Telefonda dokunma hedefi 44 px (mobil ölçü jetonu yalnız orada tanımlı). */
  .actions button {
    min-height: var(--m-touch, 36px);
  }

  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .dialog {
      animation: none;
    }
  }
</style>
