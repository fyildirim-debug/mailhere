<script lang="ts">
  // Gönderim öncesi onay: ek unutulmuş olabilir ya da konu boş. Enter (ve Ctrl
  // Enter) yine de gönderir, Esc, dışarı tıklama ve Android geri tuşu vazgeçer
  // (mobil kabukta alt sayfa katmanına bağlıdır). Gönderimi başlatan Ctrl Enter
  // basılı tutulursa yinelenen tuş onay sayılmaz.
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n/index.svelte';
  import type { SendIssues } from '../../lib/mail/precheck';
  import Icon from '../../lib/ui/Icon.svelte';
  import { sheetClosed, sheetLayerActive, sheetOpened } from '../../lib/ui/sheet-layer';

  interface Props {
    issues: SendIssues;
    /** Zamanlanmış gönderim: onay düğmesi "Yine de zamanla" der. */
    scheduled?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { issues, scheduled = false, onconfirm, oncancel }: Props = $props();

  const word = $derived(issues.attachmentWord);
  const title = $derived(
    word && issues.noSubject ? t('compose.check.bothTitle') : word ? t('compose.check.noAttachTitle') : t('compose.check.noSubjectTitle'),
  );

  let confirm = $state<HTMLButtonElement>();
  let box = $state<HTMLDivElement>();

  sheetOpened();

  onMount(() => {
    confirm?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        oncancel();
      } else if (e.key === 'Enter') {
        // Pencerenin Ctrl Enter kısayolu arkada yeniden çalışmasın.
        e.stopPropagation();
        if (e.repeat) e.preventDefault();
        else if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          onconfirm();
        }
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      sheetClosed();
    };
  });

  $effect(() => {
    if (!sheetLayerActive()) oncancel();
  });

  /** Belgenin köküne taşınır: gömülü yazma ekranı dönüşümlü bir kapta durabilir. */
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
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="send-confirm-title" aria-describedby="send-confirm-lead" tabindex="-1" bind:this={box} onkeydown={onTab}>
    <div class="head">
      <span class="badge"><Icon name={word ? 'paperclip' : 'alert'} size={20} /></span>
      <div class="grow">
        <h2 id="send-confirm-title">{title}</h2>
        <div id="send-confirm-lead" class="lead">
          {#if word}<p>{t('compose.check.noAttachBody', { word })}</p>{/if}
          {#if issues.noSubject}<p>{t('compose.check.noSubjectBody')}</p>{/if}
        </div>
      </div>
    </div>
    <div class="actions">
      <button class="btn-soft" onclick={oncancel}>{t('act.cancel')}</button>
      <button class="btn-primary" bind:this={confirm} onclick={onconfirm}>
        <Icon name={scheduled ? 'clock' : 'send'} size={16} />{t(scheduled ? 'compose.check.scheduleAnyway' : 'compose.check.sendAnyway')}
      </button>
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

  h2 {
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.015em;
    text-wrap: balance;
  }

  .lead {
    display: flex;
    flex-direction: column;
    gap: 8px;
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

  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .dialog {
      animation: none;
    }
  }
</style>
