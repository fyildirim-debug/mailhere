<script lang="ts">
  // Menülerde hazır seçeneklerin altındaki "kendi tarih-saatini seç" satırı:
  // basınca yerinde tarih-saat girdisi ve onay düğmesi açılır (ertele, sonra
  // gönder, yanıt hatırlatması). Geçmiş bir an seçilemez; "şimdi" girdi her
  // değiştiğinde ve onay anında yeniden okunur (menü açık beklemiş olabilir).
  import { tick } from 'svelte';
  import { t } from '../i18n/index.svelte';
  import Icon from '../ui/Icon.svelte';
  import { parseLocalDateTime, toLocalDateTime } from '../util/followup';

  interface Props {
    /** Satırın metni ("Tarih ve saat seç…"). */
    row: string;
    /** Girdinin erişilebilir adı. */
    label: string;
    /** Onay düğmesinin metni: yaptığı eylem ("Ertele", "Zamanla"). */
    action: string;
    /** Girdinin başlangıç değeri; geçmişteyse bir saat sonrası yazılır. */
    initial: number;
    /** Alt sayfada (mobil) 20, açılır menüde 16. */
    iconSize?: number;
    onpick: (at: number) => void;
  }

  let { row, label, action, initial, iconSize = 16, onpick }: Props = $props();
  /** Aynı menüde iki satır açık olabilir (sonra gönder + hatırlatma): uyarı kimliği bileşene özgü. */
  const uid = $props.id();

  let open = $state(false);
  let value = $state('');
  let now = $state(Date.now());
  let input = $state<HTMLInputElement>();

  const at = $derived(parseLocalDateTime(value, now));
  const past = $derived(value !== '' && at == null);

  async function show() {
    now = Date.now();
    value = toLocalDateTime(initial > now ? initial : now + 3_600_000);
    open = true;
    await tick();
    input?.focus();
  }

  function pick() {
    now = Date.now();
    const ms = parseLocalDateTime(value, now);
    if (ms != null) onpick(ms);
  }
</script>

{#if open}
  <div class="custom" class:sheet={iconSize > 16}>
    <input
      class="input small num"
      type="datetime-local"
      aria-label={label}
      aria-invalid={past}
      aria-describedby={past ? `${uid}-past` : undefined}
      bind:this={input}
      bind:value
      min={toLocalDateTime(now)}
      oninput={() => (now = Date.now())}
      onkeydown={(e) => {
        if (e.key !== 'Enter' || e.ctrlKey || e.metaKey) return;
        e.preventDefault();
        pick();
      }}
    />
    <button class="btn-primary small" disabled={at == null} onclick={pick}>{action}</button>
  </div>
  {#if past}<p id="{uid}-past" class="past" class:sheet={iconSize > 16} role="alert">{t('time.past')}</p>{/if}
{:else}
  <button class="menu-item" class:sheet={iconSize > 16} role="menuitem" onclick={show}>
    <Icon name="calendar" size={iconSize} />
    <span class="grow">{row}</span>
  </button>
{/if}

<style>
  .menu-item :global(.icon) {
    color: var(--ink-3);
  }

  .menu-item.sheet {
    padding: 0 12px;
    gap: 14px;
  }

  .grow {
    flex: 1;
  }

  /* Girdi içeriği kadar yer ister (12 saatlik biçim daha geniştir); düğmeyle
     yan yana sığmıyorsa düğme alt satıra, sağa geçer. Değer kırpılmaz. */
  .custom {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 6px 10px 8px;
  }

  .custom.sheet {
    padding: 8px 12px 10px;
  }

  .custom .input {
    flex: 1 1 auto;
    width: auto;
    min-width: 0;
  }

  .past {
    padding: 0 10px 8px;
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  .past.sheet {
    padding: 0 12px 10px;
  }
</style>
