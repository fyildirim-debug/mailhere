<script lang="ts">
  // Onay penceresi: geri alınamayan ya da toplu bir eylemden önce sorar.
  // Odak onay düğmesinde açılır ve pencerenin içinde döner; Enter onaylar
  // (odak başka bir düğmedeyse o düğme çalışır), Esc, dışarı tıklama ve Android
  // geri tuşu vazgeçer (mobil kabukta alt sayfa katmanına bağlıdır). Kapanınca
  // odak pencereyi açan öğeye döner. Basılı tutulan Enter onay sayılmaz.
  import { onMount } from 'svelte';
  import { t } from '../i18n/index.svelte';
  import Icon from './Icon.svelte';
  import type { IconName } from './icons';
  import { sheetClosed, sheetLayerActive, sheetOpened } from './sheet-layer';

  interface Props {
    title: string;
    /** Ne olacağı ve geri alınıp alınamayacağı. */
    body: string;
    /** Onay düğmesi yaptığı işi söyler ("Kalıcı sil"). */
    confirmLabel: string;
    cancelLabel?: string;
    icon?: IconName;
    /** Geri alınamayan eylem: rozet ve onay düğmesi `--danger`. */
    danger?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { title, body, confirmLabel, cancelLabel, icon = 'alert', danger = false, onconfirm, oncancel }: Props = $props();

  const uid = $props.id();
  let confirm = $state<HTMLButtonElement>();
  let box = $state<HTMLDivElement>();

  sheetOpened();

  onMount(() => {
    const returnTo = document.activeElement as HTMLElement | null;
    confirm?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        oncancel();
      } else if (e.key === 'Enter') {
        // Arkadaki pencerenin Enter kısayolu çalışmasın.
        e.stopPropagation();
        if (e.repeat) e.preventDefault();
        else if (!(e.target as Element | null)?.closest?.('button')) {
          e.preventDefault();
          onconfirm();
        }
      } else if (e.key !== 'Tab') {
        // Tek tuşlu kısayollar (arşivle, sil…) pencere açıkken arkada çalışmaz.
        e.stopPropagation();
      }
    };
    document.addEventListener('keydown', onKey, true);
    /** Odak pencereyi açan öğeye döner; bu arada başka bir öğe odak aldıysa dokunulmaz. */
    const restore = () => {
      const at = document.activeElement;
      if (returnTo?.isConnected && (at == null || at === document.body)) returnTo.focus({ preventScroll: true });
    };
    return () => {
      document.removeEventListener('keydown', onKey, true);
      sheetClosed();
      restore();
      // Dışarı tıklamada tarayıcı odağı tıklamadan sonra gövdeye taşır; bir tur sonra yeniden bakılır.
      setTimeout(restore);
    };
  });

  $effect(() => {
    if (!sheetLayerActive()) oncancel();
  });

  /** Belgenin köküne taşınır: çağıran bileşen dönüşümlü bir kapta durabilir. */
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

<div class="scrim" role="presentation" use:portal onmousedown={(e) => e.target === e.currentTarget && oncancel()}>
  <div
    class="dialog"
    class:danger
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    aria-describedby="{uid}-body"
    tabindex="-1"
    bind:this={box}
    onkeydown={onTab}
  >
    <div class="head">
      <span class="badge"><Icon name={icon} size={20} /></span>
      <div class="grow">
        <h2 id="{uid}-title">{title}</h2>
        <p id="{uid}-body" class="lead">{body}</p>
      </div>
    </div>
    <div class="actions">
      <button class="btn-soft" onclick={oncancel}>{cancelLabel ?? t('act.cancel')}</button>
      <button class="btn-primary go" bind:this={confirm} onclick={onconfirm}>{confirmLabel}</button>
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
    gap: 20px;
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

  .danger .badge {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
  }

  h2 {
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
    text-wrap: balance;
  }

  .lead {
    margin-top: 6px;
    color: var(--ink-2);
    overflow-wrap: anywhere;
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

  /* Geri alınamayan eylemin düğmesi: --danger zemin, üstünde kâğıt rengi (iki temada ≥ 4.5:1). */
  .danger .go {
    background: var(--danger);
    color: var(--paper);
  }

  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .dialog {
      animation: none;
    }
  }
</style>
