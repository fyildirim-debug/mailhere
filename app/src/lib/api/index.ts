// Arayüzün arka uca tek kapısı: Tauri içinde gerçek komutlar, tarayıcıda sahte arka uç.

import type { AppError, CommandArgs, CommandName, CommandResult, EventPayloads } from './types';
import type { MockBackend } from './mock';

export * from './types';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

interface Backend {
  call<K extends CommandName>(name: K, args?: CommandArgs<K>): Promise<CommandResult<K>>;
  on<E extends keyof EventPayloads>(event: E, handler: (p: EventPayloads[E]) => void): Promise<() => void> | (() => void);
}

let current: Promise<Backend> | null = null;

async function tauriBackend(): Promise<Backend> {
  const [{ invoke }, { listen }] = await Promise.all([import('@tauri-apps/api/core'), import('@tauri-apps/api/event')]);
  return {
    call: (name, args) => invoke(name, (args ?? {}) as Record<string, unknown>),
    on: (event, handler) => listen(event, (e) => handler(e.payload as never)),
  };
}

async function mockBackend(): Promise<Backend> {
  const { createMockBackend } = await import('./mock');
  const params = new URLSearchParams(window.location.search);
  const mock = createMockBackend({ empty: params.has('bos'), persist: true });
  // Testler ve geliştirme için sahte arka uca erişim.
  (window as unknown as { __mh?: MockBackend }).__mh = mock;
  return mock;
}

function backend(): Promise<Backend> {
  current ??= isTauri ? tauriBackend() : mockBackend();
  return current;
}

/** Testlerde belirli bir arka uç kullanmak için. */
export function useBackend(b: Backend) {
  current = Promise.resolve(b);
}

type NoArgs = Record<string, never>;

export async function call<K extends CommandName>(
  name: K,
  ...args: CommandArgs<K> extends NoArgs ? [] : [CommandArgs<K>]
): Promise<CommandResult<K>> {
  const b = await backend();
  return b.call(name, args[0]);
}

/** Olay dinler; dönen işlev aboneliği kaldırır. */
export function on<E extends keyof EventPayloads>(event: E, handler: (p: EventPayloads[E]) => void): () => void {
  let off: (() => void) | null = null;
  let cancelled = false;
  void backend().then(async (b) => {
    const unlisten = await b.on(event, handler);
    if (cancelled) unlisten();
    else off = unlisten;
  });
  return () => {
    cancelled = true;
    off?.();
  };
}

export function toAppError(e: unknown): AppError {
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) return e as AppError;
  return { code: 'internal', message: e instanceof Error ? e.message : String(e) };
}

// ------------------------------------------------------------ pencere

async function currentWindow() {
  if (!isTauri) return null;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  return getCurrentWindow();
}

export const appWindow = {
  async minimize() {
    await (await currentWindow())?.minimize();
  },
  async toggleMaximize() {
    await (await currentWindow())?.toggleMaximize();
  },
  /** Ana pencere tepsiye gizlenir; arka uç karar verir. */
  async close() {
    const w = await currentWindow();
    if (w) await w.close();
    else window.close();
  },
  async setAlwaysOnTop(on: boolean) {
    await (await currentWindow())?.setAlwaysOnTop(on);
  },
};

/** Bağlantıyı varsayılan tarayıcıda açar. */
export async function openExternal(url: string) {
  if (!/^(https?:|mailto:)/i.test(url)) return;
  if (isTauri) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  } else {
    window.open(url, '_blank', 'noopener,noreferrer');
  }
}

/** Dosya seçtirir (ek eklemek için). Tarayıcıda boş döner. */
export async function pickFiles(): Promise<string[]> {
  if (!isTauri) return [];
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ multiple: true, directory: false });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}
