<script lang="ts">
  // `askConfirm` ile sorulan onayı çizer; pencere başına bir kez yerleştirilir.
  import ConfirmDialog from './ConfirmDialog.svelte';
  import { answerConfirm, confirmState } from './confirm.svelte';

  const c = $derived(confirmState.current);
</script>

{#if c}
  {#key c.key}
    <ConfirmDialog
      title={c.title}
      body={c.body}
      confirmLabel={c.confirmLabel}
      cancelLabel={c.cancelLabel}
      icon={c.icon}
      danger={c.danger}
      onconfirm={() => answerConfirm(true)}
      oncancel={() => answerConfirm(false)}
    />
  {/key}
{/if}
