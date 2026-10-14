<script lang="ts">
  import { route } from './lib/stores/route.svelte';
  import { isMobileShell } from './lib/shell';
  import MainWindow from './windows/main/MainWindow.svelte';
  import NotifyWindow from './windows/notify/NotifyWindow.svelte';
  import QuickWindow from './windows/quick/QuickWindow.svelte';
  import ComposeWindow from './windows/compose/ComposeWindow.svelte';

  // Mobil kabuk tembel yüklenir: masaüstü paketi onu taşımaz.
  const mobile = isMobileShell ? import('./windows/mobile/MobileApp.svelte') : null;
</script>

{#if mobile}
  {#await mobile then { default: MobileApp }}
    <MobileApp />
  {/await}
{:else if route.current.name === 'notify'}
  <NotifyWindow />
{:else if route.current.name === 'quick'}
  <QuickWindow id={route.current.id} />
{:else if route.current.name === 'compose'}
  <!-- Karma değişince (tarayıcı, testler) yazma penceresi baştan kurulur. -->
  {#key route.current}
    <ComposeWindow mode={route.current.mode} messageId={route.current.id} initialTo={route.current.to} initialFrom={route.current.from} />
  {/key}
{:else}
  <MainWindow />
{/if}
