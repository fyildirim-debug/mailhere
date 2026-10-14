<script lang="ts">
  import { route } from './lib/stores/route.svelte';
  import MainWindow from './windows/main/MainWindow.svelte';
  import NotifyWindow from './windows/notify/NotifyWindow.svelte';
  import QuickWindow from './windows/quick/QuickWindow.svelte';
  import ComposeWindow from './windows/compose/ComposeWindow.svelte';
</script>

{#if route.current.name === 'notify'}
  <NotifyWindow />
{:else if route.current.name === 'quick'}
  <QuickWindow id={route.current.id} />
{:else if route.current.name === 'compose'}
  <!-- Karma değişince (tarayıcı, testler) yazma penceresi baştan kurulur. -->
  {#key route.current}
    <ComposeWindow mode={route.current.mode} messageId={route.current.id} initialTo={route.current.to} />
  {/key}
{:else}
  <MainWindow />
{/if}
