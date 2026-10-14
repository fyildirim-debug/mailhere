<script lang="ts">
  // Tek alanlı küçük soru penceresi (ör. "Aramayı kaydet", "Yeniden adlandır"):
  // Enter kaydeder, Esc ya da dışarı tıklama kapatır.
  import { t } from '../i18n/index.svelte';

  interface Props {
    title: string;
    label: string;
    value?: string;
    placeholder?: string;
    maxlength?: number;
    /** Ekran koordinatı; verilmezse kapsayıcıya göre `style` ile konumlanır. */
    at?: { x: number; y: number } | null;
    style?: string;
    onsave: (value: string) => void;
    onclose: () => void;
  }

  let { title, label, value = '', placeholder = '', maxlength = 60, at = null, style = '', onsave, onclose }: Props = $props();

  // Başlangıç değeri bir kez alınır; sonrası kullanıcının yazdığıdır.
  // svelte-ignore state_referenced_locally
  let text = $state(value);
  let box = $state<HTMLDivElement>();
  let input = $state<HTMLInputElement>();
  let pos = $state<{ left: number; top: number } | null>(null);

  $effect(() => {
    if (!box) return;
    if (at) {
      const r = box.getBoundingClientRect();
      pos = {
        left: Math.max(8, Math.min(at.x, window.innerWidth - r.width - 8)),
        top: Math.max(8, Math.min(at.y, window.innerHeight - r.height - 8)),
      };
    }
    input?.focus();
    input?.select();
    const onDoc = (e: MouseEvent) => {
      if (box && !box.contains(e.target as Node)) onclose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onclose();
      }
    };
    const timer = setTimeout(() => document.addEventListener('mousedown', onDoc));
    document.addEventListener('keydown', onKey, true);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey, true);
    };
  });

  function save() {
    const v = text.trim();
    if (!v) return;
    onsave(v);
  }
</script>

<div
  class="popover prompt"
  class:fixed={!!at}
  role="dialog"
  aria-label={title}
  bind:this={box}
  style={at ? `left:${pos?.left ?? at.x}px;top:${pos?.top ?? at.y}px` : style}
>
  <div class="menu-title">{title}</div>
  <label class="field">
    <span class="label">{label}</span>
    <input
      class="input small"
      bind:this={input}
      bind:value={text}
      {placeholder}
      {maxlength}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          save();
        }
      }}
    />
  </label>
  <div class="set-actions">
    <button class="btn-ghost" onclick={onclose}>{t('act.cancel')}</button>
    <button class="btn-primary small" disabled={!text.trim()} onclick={save}>{t('set.save')}</button>
  </div>
</div>

<style>
  .prompt {
    width: 300px;
    padding: 6px 10px 10px;
  }

  .fixed {
    position: fixed;
    z-index: 60;
  }

  .field {
    display: block;
    padding: 4px 0 10px;
  }
</style>
