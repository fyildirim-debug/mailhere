<script lang="ts">
  import { runToastAction, toasts } from './toast.svelte';
</script>

{#if toasts.current}
  {#key toasts.current.id}
    <div class="toast" role="status">
      <span>{toasts.current.text}</span>
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

  .toast span {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
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
    background: rgba(127, 127, 127, 0.16);
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
