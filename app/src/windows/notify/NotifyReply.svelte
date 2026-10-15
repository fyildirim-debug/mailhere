<script lang="ts">
  // Kartın içinde yerinde açılan yanıt alanı: "Kime" satırı, 1–4 satır büyüyen
  // kutu, Gönder (Ctrl/Cmd Enter) ve Vazgeç (Esc). Açılınca kutu odaklanır.
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';

  interface Props {
    /** Yanıtın gideceği kişi(ler), görünen adla. */
    to: string;
    value: string;
    busy: boolean;
    error: string | null;
    onsend: () => void;
    oncancel: () => void;
  }

  let { to, value = $bindable(), busy, error, onsend, oncancel }: Props = $props();

  const errId = $props.id();
  let box = $state<HTMLTextAreaElement>();

  /** En çok dört satır; fazlası kutunun içinde kayar. */
  const MAX_LINES = 4;

  function grow() {
    if (!box) return;
    const s = getComputedStyle(box);
    const line = parseFloat(s.lineHeight) || 20;
    const pad = parseFloat(s.paddingTop) + parseFloat(s.paddingBottom);
    const border = parseFloat(s.borderTopWidth) + parseFloat(s.borderBottomWidth);
    const max = line * MAX_LINES + pad + border;
    box.style.height = 'auto';
    const want = box.scrollHeight + border;
    box.style.height = `${Math.min(want, max)}px`;
    box.style.overflowY = want > max ? 'auto' : 'hidden';
  }

  $effect(() => {
    void value;
    grow();
  });

  onMount(() => {
    box?.focus();
    // İmleç metnin sonunda (geri alınan yanıt metniyle döner).
    box?.setSelectionRange(value.length, value.length);
  });

  function onkey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      onsend();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      oncancel();
    }
  }
</script>

<div class="reply">
  <div class="to meta trunc" title={to}>{t('read.replyTo', { list: to })}</div>
  <textarea
    bind:this={box}
    bind:value
    class="input box"
    rows="1"
    aria-label={t('notify.replyBox')}
    aria-invalid={error ? 'true' : undefined}
    aria-describedby={error ? errId : undefined}
    placeholder={t('notify.replyPlaceholder')}
    readonly={busy}
    onkeydown={onkey}
  ></textarea>
  {#if error}
    <p class="err" id={errId} role="alert"><Icon name="alert" size={16} />{error}</p>
  {/if}
  <div class="foot">
    <span class="meta">{t('read.sendHint')}</span>
    <span class="grow"></span>
    <button class="btn-soft small" onclick={oncancel}>{t('act.cancel')}</button>
    <button class="btn-primary small" disabled={!value.trim() || busy} onclick={onsend}><Icon name="send" size={16} />{t('act.send')}</button>
  </div>
</div>

<style>
  .reply {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
    animation: open var(--t-state) var(--ease) both;
  }

  .to {
    color: var(--ink-2);
  }

  .box {
    height: auto;
    min-height: 0;
    padding: 8px 10px;
    line-height: 20px;
    resize: none;
    overflow-y: hidden;
  }

  .err {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 0;
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--ink-2);
  }

  .err :global(.icon) {
    flex: none;
    color: var(--danger);
  }

  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .grow {
    flex: 1;
  }

  @keyframes open {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  /* Azaltılmış harekette kayma kalkar, saydamlık kalır. */
  @media (prefers-reduced-motion: reduce) {
    .reply {
      animation-name: fade;
    }
  }
</style>
