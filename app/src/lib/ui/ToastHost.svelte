<script lang="ts">
  import { runToastAction, toasts } from './toast.svelte';
</script>

{#if toasts.current}
  {#key toasts.current.id}
    <div class="toast" role="status">
      <span class="text">{toasts.current.text}</span>
      {#if toasts.current.hint}
        <span class="hint">{toasts.current.hint.label}<kbd>{toasts.current.hint.keys}</kbd></span>
      {/if}
      {#if toasts.current.action}
        <button onclick={() => runToastAction()}>{toasts.current.action.label}</button>
      {/if}
    </div>
  {/key}
{/if}

<style>
  .toast {
    position: fixed;
    left: 50%;
    bottom: 24px;
    z-index: 50;
    display: flex;
    align-items: center;
    gap: 16px;
    max-width: calc(100vw - 32px);
    padding: 10px 10px 10px 16px;
    border-radius: var(--r-card);
    background: var(--toast-bg);
    color: var(--toast-ink);
    box-shadow: var(--shadow);
    transform: translateX(-50%);
    animation: toast-in var(--t-panel) var(--ease);
  }

  .text {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* Kısayol ipucu: metnin yanında sakin, eylem düğmesinden önce. */
  .hint {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-meta);
    color: color-mix(in srgb, var(--toast-ink) 72%, transparent);
    animation: hint-in var(--t-state) var(--ease);
  }

  kbd {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: var(--r-kbd);
    border: 1px solid color-mix(in srgb, var(--toast-ink) 34%, transparent);
    color: var(--toast-ink);
    font: inherit;
    font-weight: 500;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  @keyframes hint-in {
    from {
      opacity: 0;
    }
  }

  button {
    flex: none;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--toast-act);
    font-weight: 600;
  }

  button:hover {
    background: color-mix(in srgb, var(--toast-ink) 12%, transparent);
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translate(-50%, 12px);
    }
    to {
      opacity: 1;
      transform: translate(-50%, 0);
    }
  }
</style>
