<script lang="ts">
  // Liste satırındaki doğrulama kodu çipi (masaüstü ve mobil): tıklayınca kodu
  // kopyalar, postayı açmaz. Yalnız taze postada gösterilir (`otpFresh`).
  import type { MessageId } from '../api';
  import { t } from '../i18n/index.svelte';
  import { copyCode } from '../stores/guard.svelte';
  import Icon from '../ui/Icon.svelte';
  import { groupCode } from './otp';

  interface Props {
    id: MessageId;
    code: string;
    unread: boolean;
  }
  let { id, code, unread }: Props = $props();

  function copy(e: Event) {
    e.stopPropagation();
    e.preventDefault();
    void copyCode(id, code, unread);
  }
</script>

<button
  type="button"
  class="otp-chip num"
  aria-label={t('otp.chipLabel', { code: groupCode(code) })}
  title={t('otp.chipTitle')}
  onclick={copy}
  onkeydown={(e) => e.stopPropagation()}
  ondblclick={(e) => e.stopPropagation()}
>
  <span>{groupCode(code)}</span>
  <Icon name="copy" size={14} />
</button>

<style>
  .otp-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: var(--paper);
    color: var(--ink);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    cursor: pointer;
    transition: background 120ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  .otp-chip:hover {
    background: var(--hover);
  }
  .otp-chip:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .otp-chip :global(svg) {
    color: var(--ink-2);
  }
  @media (prefers-reduced-motion: reduce) {
    .otp-chip {
      transition: none;
    }
  }
</style>
