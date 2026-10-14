<script lang="ts">
  // Kişi kartı: gönderenle yazışmanın özeti ve kişiye yönelik eylemler.
  // Sabit konumlu açılır pencere; Esc ya da dışarı tıklama kapatır.
  import { call, toAppError, type Address, type ContactInfo } from '../api';
  import { t, when } from '../i18n/index.svelte';
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import { showToast } from '../ui/toast.svelte';
  import { displayName } from '../util/format';
  import { blockSender, newRuleFrom, searchAll, select } from '../stores/mail.svelte';
  import { settings, updateSettings } from '../stores/settings.svelte';

  interface Props {
    address: Address;
    /** Postanın hesabı (engelleme kuralı bu hesaba yazılır). */
    accountId: string;
    /** Ekran koordinatı (kartın sol üst köşesi). */
    at: { x: number; y: number };
    onclose: () => void;
  }

  let { address, accountId, at, onclose }: Props = $props();

  let info = $state<ContactInfo | null>(null);
  let error = $state<string | null>(null);
  let el = $state<HTMLDivElement>();
  let pos = $state<{ left: number; top: number } | null>(null);

  const email = $derived(address.email.trim().toLowerCase());
  const isVip = $derived(settings.value.vip.includes(email));
  const name = $derived(info?.name ?? displayName(address));

  $effect(() => {
    let cancelled = false;
    void call('contact_get', { email: address.email })
      .then((i) => {
        if (!cancelled) info = i;
      })
      .catch((e) => {
        if (!cancelled) error = toAppError(e).message;
      });
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    pos = {
      left: Math.max(8, Math.min(at.x, window.innerWidth - r.width - 8)),
      top: Math.max(8, Math.min(at.y, window.innerHeight - r.height - 8)),
    };
    el.focus();
    const onDoc = (e: MouseEvent) => {
      if (el && !el.contains(e.target as Node)) onclose();
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

  async function toggleVip() {
    const was = isVip;
    const vip = was ? settings.value.vip.filter((v) => v !== email) : [...settings.value.vip, email];
    try {
      await updateSettings({ vip });
      showToast({ text: t(was ? 'toast.vipRemoved' : 'toast.vipAdded', { email }) });
    } catch (e) {
      showToast({ text: t('toast.error', { message: toAppError(e).message }) });
    }
  }

  function showMail() {
    onclose();
    searchAll(`from:${email}`);
  }

  function compose() {
    onclose();
    void call('open_compose', {
      prefill: { mode: 'new', messageId: null, to: [{ name: info?.name ?? address.name, email: address.email }], accountId },
    });
  }

  function rule() {
    onclose();
    newRuleFrom({
      id: '',
      name: t('read.ruleName', { name }),
      enabled: true,
      accountId,
      match: 'all',
      conditions: [{ kind: 'from', op: 'is', value: email }],
      actions: [{ kind: 'archive' }],
      stop: false,
    });
  }

  function block() {
    onclose();
    void blockSender(accountId, email);
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(address.email);
      showToast({ text: t('contact.copied') });
    } catch {
      /* pano kapalıysa adres kartta seçilebilir durur */
    }
  }

  function open(id: number) {
    onclose();
    void select(id, { markRead: false });
  }
</script>

<div
  class="popover card"
  role="dialog"
  aria-label={t('contact.title')}
  tabindex="-1"
  bind:this={el}
  style="left:{pos?.left ?? at.x}px;top:{pos?.top ?? at.y}px"
>
  <div class="head">
    <Avatar {address} size={40} />
    <div class="id">
      <div class="name-row"><span class="name trunc">{name}</span>{#if isVip}<span class="tag">{t('contact.vip')}</span>{/if}</div>
      <div class="meta trunc selectable">{address.email}</div>
    </div>
    <button class="btn-icon small" aria-label={t('contact.copy')} title={t('contact.copy')} onclick={copy}><Icon name="copy" size={16} /></button>
  </div>

  {#if error}
    <p class="meta note">{t('contact.loadError', { message: error })}</p>
  {:else if !info}
    <p class="meta note">{t('contact.loading')}</p>
  {:else}
    <p class="meta note num">{t('contact.total', { n: info.total })}{#if info.lastDate != null} · {t('contact.last', { when: when(info.lastDate) })}{/if}</p>
    {#if info.recent.length}
      <div class="recent-h">{t('contact.recent')}</div>
      <ul class="recent">
        {#each info.recent as m (m.id)}
          <li>
            <button class="recent-btn" onclick={() => open(m.id)}>
              <span class="trunc">{m.subject || '—'}</span>
              <span class="meta num">{when(m.date)}</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="meta note">{t('contact.none')}</p>
    {/if}
  {/if}

  <div class="actions">
    <button class="menu-item" onclick={showMail}><Icon name="search" size={16} />{t('contact.showMail')}</button>
    <button class="menu-item" onclick={compose}><Icon name="pen" size={16} />{t('contact.compose')}</button>
    <button class="menu-item" onclick={toggleVip}><span class:star={isVip}><Icon name="star" size={16} filled={isVip} /></span>{isVip ? t('contact.vipRemove') : t('contact.vipAdd')}</button>
    <button class="menu-item" onclick={rule}><Icon name="rules" size={16} />{t('contact.rule')}</button>
    <button class="menu-item danger" onclick={block}><Icon name="block" size={16} />{t('contact.block')}</button>
  </div>
</div>

<style>
  .card {
    position: fixed;
    z-index: 60;
    width: 332px;
    padding: 12px 6px 6px;
    border-radius: var(--r-float);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 6px 10px;
  }

  .id {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .name {
    font-weight: 600;
  }

  .note {
    margin: 0;
    padding: 0 12px 8px;
  }

  .recent-h {
    padding: 6px 12px 4px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-3);
  }

  .recent {
    margin: 0 0 6px;
    padding: 0;
    list-style: none;
  }

  .recent-btn {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-icon);
    background: transparent;
    color: var(--ink);
    text-align: left;
  }

  .recent-btn:hover,
  .recent-btn:focus-visible {
    background: var(--hover);
  }

  .recent-btn .trunc {
    flex: 1;
  }

  .actions {
    padding-top: 6px;
    border-top: 1px solid var(--line);
  }

  .danger {
    color: var(--danger);
  }

  .star {
    display: inline-flex;
    color: var(--amber);
  }
</style>
