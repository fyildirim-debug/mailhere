<script lang="ts">
  // Geri alınamayan işlemin onayı (kalıcı silme, kutuyu boşaltma): alt sayfada
  // soru, sonucu söyleyen kısa metin ve iki düğme. Onay `--danger` metinli
  // yumuşak düğmedir (ekranın birincil eylemi değildir); Vazgeç ilk odaktır.
  // Örtü, Esc, aşağı kaydırma ve geri tuşu vazgeçer (Sheet).
  import { t } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import type { IconName } from '../../lib/ui/icons';
  import Sheet from '../../lib/ui/Sheet.svelte';

  interface Props {
    /** Soru: neyin silineceği ("3 posta kalıcı silinsin mi?"). */
    title: string;
    /** Sonuç: geri alınamayacağı. */
    body: string;
    /** Onay düğmesinin adı; yaptığı eylemi söyler. */
    confirm: string;
    icon?: IconName;
    onconfirm: () => void;
    onclose: () => void;
  }

  let { title, body, confirm, icon = 'trash', onconfirm, onclose }: Props = $props();

  /** Önce eylem, sonra kapanış: kapanış çağıranın durumunu (neyin silineceği) sıfırlar. */
  function go() {
    onconfirm();
    onclose();
  }
</script>

<Sheet {title} {onclose}>
  <div class="confirm">
    <p class="body">{body}</p>
    <div class="acts">
      <button class="btn-soft" onclick={onclose}>{t('act.cancel')}</button>
      <button class="btn-soft go" onclick={go}><Icon name={icon} size={18} />{confirm}</button>
    </div>
  </div>
</Sheet>

<style>
  .confirm {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 0 12px 8px;
  }

  .body {
    margin: 0;
    color: var(--ink-2);
  }

  /* İki düğme yan yana; uzun ad (dar ekran, büyük yazı) sığmazsa alt alta iner, kırpılmaz. */
  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .acts .btn-soft {
    flex: 1 1 auto;
    min-height: var(--m-touch-lg);
  }

  .go {
    color: var(--danger);
  }
</style>
