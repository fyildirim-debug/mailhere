<script lang="ts">
  // Telefon tarafı: QR okut ya da kodu yapıştır → bilgisayardan paket →
  // önizleme (seçimler) → uygula → rapor. Hesap ekleme akışında (ilk kurulum)
  // ve Ayarlar › Cihazlar'da kullanılır. `ondone` verilmezse gezinme yığını
  // köke (listeye) döner.
  import {
    call,
    toAppError,
    type TransferPreview,
    type TransferPreviewAccount,
    type TransferReport,
  } from '../../lib/api';
  import { t, when, type MessageKey } from '../../lib/i18n/index.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { mail, openAccountPassword, reloadAccounts } from '../../lib/stores/mail.svelte';
  import { loadTags } from '../../lib/stores/tags.svelte';
  import { nav } from './nav.svelte';

  interface Props {
    /** Hesap ekleme akışında: önceki adıma dönüş (başlık da h1 olur). */
    onback?: () => void;
    /** Bitince; verilmezse gezinme yığını köke döner. */
    ondone?: () => void;
  }

  let { onback, ondone }: Props = $props();

  type Step = 'code' | 'preview' | 'report';
  let step = $state<Step>('code');
  let code = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let preview = $state<TransferPreview | null>(null);
  let report = $state<TransferReport | null>(null);
  let picked = $state<Record<string, boolean>>({});
  let takeRules = $state(true);
  let takeTags = $state(true);
  let takeSettings = $state(true);

  const platformName: Record<string, string> = { windows: 'Windows', linux: 'Linux', macos: 'macOS', android: 'Android' };
  const pickedIds = $derived(preview ? preview.accounts.filter((a) => picked[a.id]).map((a) => a.id) : []);
  const anything = $derived(
    pickedIds.length > 0 || (takeRules && !!preview?.rules) || (takeTags && !!preview?.tags) || (takeSettings && !!preview?.settings),
  );
  // Kendi ekranında (karşılama) h1; Ayarlar › Cihazlar içinde bölüm başlığının altında h3.
  const heading = $derived(onback ? 'h1' : 'h3');

  function authLabel(a: TransferPreviewAccount): string {
    return a.kind === 'jmap' ? 'JMAP' : t(`acc.via.${a.auth}` as MessageKey);
  }

  async function receive(text: string) {
    const trimmed = text.trim();
    if (!trimmed || busy) return;
    busy = true;
    error = null;
    try {
      const p = await call('transfer_receive', { code: trimmed });
      preview = p;
      picked = Object.fromEntries(p.accounts.map((a) => [a.id, true]));
      takeRules = p.rules > 0;
      takeTags = p.tags > 0;
      takeSettings = p.settings;
      step = 'preview';
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      busy = false;
    }
  }

  /** Kamera (Android kabuğundaki `qr_scan`); vazgeçilirse sessizce döner. */
  async function scan() {
    if (busy) return;
    busy = true;
    error = null;
    let text: string | null = null;
    try {
      text = await call('qr_scan');
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      busy = false;
    }
    if (text) {
      code = text;
      await receive(text);
    }
  }

  async function fromClipboard() {
    let text = '';
    try {
      text = await navigator.clipboard.readText();
    } catch {
      // Pano okunamadı (izin ya da destek yok): boş demek yanıltır.
      error = t('aktar.clipboardDenied');
      return;
    }
    if (!/mailhere:\/\/aktar\?/i.test(text)) {
      error = t('aktar.clipboardEmpty');
      return;
    }
    code = text.trim();
    error = null;
    await receive(code);
  }

  async function apply() {
    if (!preview || !anything || busy) return;
    busy = true;
    error = null;
    try {
      report = await call('transfer_apply', {
        previewId: preview.previewId,
        choice: {
          accounts: pickedIds,
          rules: takeRules && preview.rules > 0,
          tags: takeTags && preview.tags > 0,
          settings: takeSettings && preview.settings,
        },
      });
      await Promise.all([reloadAccounts(), loadTags()]);
      step = 'report';
    } catch (e) {
      error = toAppError(e).message;
      busy = false;
      return;
    }
    busy = false;
    // Bildirim izni burada sorulmaz: hesaplar gelince MobileApp bir kez ister.
  }

  function accountFor(email: string) {
    const e = email.toLowerCase();
    return mail.accounts.find((a) => a.email.toLowerCase() === e) ?? null;
  }

  function enterPassword(email: string) {
    const a = accountFor(email);
    if (a) openAccountPassword(a.id);
  }

  function done() {
    if (!mail.accounts.length) {
      // Hesap gelmedi: hesap ekleme akışına dönülür.
      if (onback) onback();
      else again();
      return;
    }
    if (ondone) ondone();
    else nav.home();
  }

  function again() {
    step = 'code';
    code = '';
    preview = null;
    report = null;
    error = null;
  }
</script>

<div class="recv">
  {#if step === 'code'}
    {#if onback}<button class="link back" onclick={onback}><Icon name="chevronLeft" size={14} />{t('onb.back')}</button>{/if}
    <svelte:element this={heading} class="title">{t('aktar.recvTitle')}</svelte:element>
    <p class="lead">{t('aktar.recvLead')}</p>
    <button class="btn-primary wide" disabled={busy} onclick={scan}><Icon name="qr" size={18} />{t('aktar.scan')}</button>
    <div class="or"><span></span>{t('aktar.or')}<span></span></div>
    <label class="label" for="aktar-code">{t('aktar.pasteLabel')}</label>
    <textarea
      id="aktar-code"
      class="input code"
      rows="3"
      placeholder={t('aktar.pastePlaceholder')}
      spellcheck="false"
      autocapitalize="off"
      autocomplete="off"
      bind:value={code}
    ></textarea>
    <div class="row">
      <button class="btn-soft" disabled={busy} onclick={fromClipboard}><Icon name="clipboard" size={16} />{t('aktar.fromClipboard')}</button>
      <button class="btn-soft" disabled={busy || !code.trim()} onclick={() => receive(code)}><Icon name="plug" size={16} />{t('aktar.connect')}</button>
    </div>
    {#if busy}<p class="wait" role="status"><Icon name="sync" size={16} class="spin" />{t('aktar.connecting')}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  {:else if step === 'preview' && preview}
    <button class="link back" onclick={() => (step = 'code')}><Icon name="chevronLeft" size={14} />{t('onb.back')}</button>
    <svelte:element this={heading} class="title">{t('aktar.previewTitle')}</svelte:element>
    <p class="lead">
      {t('aktar.previewLead', {
        version: preview.fromVersion,
        platform: platformName[preview.fromPlatform] ?? preview.fromPlatform,
        when: when(preview.createdAt),
      })}
    </p>

    <h3 class="group">{t('aktar.accounts')}</h3>
    <ul class="list">
      {#each preview.accounts as a (a.id)}
        <li>
          <label class="pick">
            <input
              type="checkbox"
              class="check"
              aria-label={t('aktar.pickAccount', { name: a.name })}
              checked={picked[a.id]}
              onchange={(e) => (picked[a.id] = (e.currentTarget as HTMLInputElement).checked)}
            />
            <span class="text">
              <span class="name"><span class="trunc">{a.name}</span><span class="tag">{a.exists ? t('aktar.willUpdate') : t('aktar.willAdd')}</span></span>
              <span class="trunc meta">{a.email} · {authLabel(a)}</span>
              {#if !a.hasSecret}<span class="warn"><Icon name="key" size={14} />{t('aktar.noSecret')}</span>{/if}
            </span>
          </label>
        </li>
      {:else}
        <li class="none meta">{t('aktar.none')}</li>
      {/each}
    </ul>

    <h3 class="group">{t('aktar.others')}</h3>
    <ul class="list">
      <li>
        <label class="pick">
          <input
            type="checkbox"
            class="check"
            aria-label={t('set.nav.rules')}
            disabled={!preview.rules}
            checked={takeRules}
            onchange={(e) => (takeRules = (e.currentTarget as HTMLInputElement).checked)}
          />
          <span class="text">
            <span class="name">{t('set.nav.rules')}</span>
            <span class="meta">{preview.rules ? t('aktar.rulesN', { n: preview.rules }) : t('aktar.none')}</span>
          </span>
        </label>
      </li>
      <li>
        <label class="pick">
          <input
            type="checkbox"
            class="check"
            aria-label={t('tag.title')}
            disabled={!preview.tags}
            checked={takeTags}
            onchange={(e) => (takeTags = (e.currentTarget as HTMLInputElement).checked)}
          />
          <span class="text">
            <span class="name">{t('tag.title')}</span>
            <span class="meta">{preview.tags ? t('aktar.tagsN', { n: preview.tags }) : t('aktar.none')}</span>
          </span>
        </label>
      </li>
      <li>
        <label class="pick">
          <input
            type="checkbox"
            class="check"
            aria-label={t('aktar.settingsPick')}
            disabled={!preview.settings}
            checked={takeSettings}
            onchange={(e) => (takeSettings = (e.currentTarget as HTMLInputElement).checked)}
          />
          <span class="text">
            <span class="name">{t('aktar.settingsPick')}</span>
            <span class="meta">{preview.settings ? t('aktar.settingsHint') : t('aktar.none')}</span>
          </span>
        </label>
      </li>
    </ul>

    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if !anything}<p class="note">{t('aktar.nothingPicked')}</p>{/if}
    <button class="btn-primary wide apply" disabled={busy || !anything} onclick={apply}>{busy ? t('aktar.applying') : t('aktar.apply')}</button>
  {:else if report}
    <span class="ok-icon"><Icon name="check" size={32} /></span>
    <svelte:element this={heading} class="title">{t('aktar.reportTitle')}</svelte:element>
    <p class="lead">{report.accountsAdded + report.accountsUpdated ? t('aktar.reportLead') : t('aktar.reportLeadNoAccount')}</p>
    <ul class="facts">
      {#if report.accountsAdded}<li><Icon name="user" size={18} />{t('aktar.added', { n: report.accountsAdded })}</li>{/if}
      {#if report.accountsUpdated}<li><Icon name="user" size={18} />{t('aktar.updated', { n: report.accountsUpdated })}</li>{/if}
      {#if report.rules}<li><Icon name="rules" size={18} />{t('aktar.rulesDone', { n: report.rules })}</li>{/if}
      {#if report.tags}<li><Icon name="tag" size={18} />{t('aktar.tagsDone', { n: report.tags })}</li>{/if}
      {#if report.settings}<li><Icon name="settings" size={18} />{t('aktar.settingsDone')}</li>{/if}
      {#if report.rulesSkipped}<li class="skipped"><Icon name="info" size={18} />{t('aktar.rulesSkipped', { n: report.rulesSkipped })}</li>{/if}
      {#if !report.accountsAdded && !report.accountsUpdated && !report.rules && !report.tags && !report.settings}
        <li class="skipped"><Icon name="info" size={18} />{t('aktar.nothingDone')}</li>
      {/if}
    </ul>

    {#if report.accountsWithoutSecret.length}
      <h3 class="group">{t('aktar.needPassword')}</h3>
      <p class="note">{t('aktar.needPasswordHint')}</p>
      <ul class="need">
        {#each report.accountsWithoutSecret as email (email)}
          {@const acc = accountFor(email)}
          <li>
            <span class="trunc">{email}</span>
            <button class="btn-soft small" disabled={!acc} onclick={() => enterPassword(email)}>
              <Icon name="key" size={16} />{acc && acc.auth !== 'password' ? t('aktar.signInAgain') : t('aktar.enterPassword')}
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    <button class="btn-primary wide" onclick={done}>{mail.accounts.length ? t('aktar.goInbox') : t('act.addAccount')}</button>
    <button class="link small again" onclick={again}>{t('aktar.again')}</button>
  {/if}
</div>

<style>
  .recv {
    display: flex;
    flex-direction: column;
    width: 100%;
    max-width: 480px;
    padding-bottom: 8px;
  }

  .title {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.02em;
    text-wrap: balance;
  }

  .lead {
    margin: 6px 0 20px;
    color: var(--ink-2);
  }

  .back {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
    margin-bottom: 4px;
    font-size: var(--fs-meta);
  }

  .wide {
    width: 100%;
    min-height: 48px;
    margin-top: 8px;
  }

  .apply {
    margin-top: 20px;
  }

  .or {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 22px 0 14px;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .or span {
    flex: 1;
    height: 1px;
    background: var(--line);
  }

  /* Kod metni veri olduğu için eş aralıklı; uzun olduğundan kırılır. */
  .code {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    word-break: break-all;
  }

  .row {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }

  .row .btn-soft {
    flex: 1;
    min-width: 0;
    min-height: 44px;
  }

  .wait {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    color: var(--ink-2);
  }

  .wait :global(.spin) {
    color: var(--accent-text);
    animation: spin 1.2s linear infinite;
  }

  .error {
    margin-top: 12px;
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  .note {
    margin: 4px 0 8px;
    font-size: var(--fs-meta);
    color: var(--ink-3);
  }

  .group {
    margin: 16px 0 2px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--line);
  }

  .list li {
    border-bottom: 1px solid var(--line);
  }

  .none {
    padding: 12px 0;
  }

  .pick {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    min-height: 56px;
    padding: 12px 0;
    cursor: pointer;
  }

  .check {
    appearance: none;
    flex: none;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin: 1px 0 0;
    border: 1.5px solid var(--line-strong);
    border-radius: var(--r-sm);
    background: var(--paper);
    cursor: pointer;
    transition:
      background var(--t-fast),
      border-color var(--t-fast);
  }

  .check::after {
    content: '';
    width: 6px;
    height: 10px;
    margin-top: -2px;
    border: solid var(--accent-ink);
    border-width: 0 2px 2px 0;
    transform: rotate(45deg) scale(0);
    transition: transform var(--t-fast) var(--ease);
  }

  .check:checked {
    background: var(--accent);
    border-color: var(--accent);
  }

  .check:checked::after {
    transform: rotate(45deg) scale(1);
  }

  .check:disabled {
    opacity: var(--dim-disabled);
    cursor: default;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-weight: 500;
  }

  .warn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--fs-meta);
    color: var(--amber-text);
  }

  .ok-icon {
    color: var(--accent);
    margin-bottom: 10px;
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0 0 8px;
    padding: 0;
    list-style: none;
  }

  .facts li {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .facts :global(.icon) {
    margin-top: 2px;
    color: var(--ok);
  }

  .facts .skipped {
    color: var(--ink-2);
  }

  .facts .skipped :global(.icon) {
    color: var(--ink-3);
  }

  .need {
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--line);
  }

  .need li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 52px;
    padding: 4px 0;
    border-bottom: 1px solid var(--line);
  }

  .need .trunc {
    flex: 1;
  }

  .again {
    align-self: center;
    margin-top: 10px;
    min-height: 44px;
    display: inline-flex;
    align-items: center;
    font-size: var(--fs-meta);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
