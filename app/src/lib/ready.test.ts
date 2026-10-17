import { afterEach, describe, expect, it, vi } from 'vitest';
import { resetFirstScreenForTest, signalFirstScreen } from './ready';

// Hazır sinyali: kanal varsa bir kez 'hazir' gider; kanal yoksa sessizdir.

afterEach(() => {
  delete (globalThis as { mhAcilis?: unknown }).mhAcilis;
  resetFirstScreenForTest();
  vi.useRealTimers();
});

describe('signalFirstScreen', () => {
  it('kanal varsa görev sonunda bir kez "hazir" yollar', () => {
    vi.useFakeTimers();
    const postMessage = vi.fn();
    (globalThis as { mhAcilis?: unknown }).mhAcilis = { postMessage };
    signalFirstScreen();
    signalFirstScreen();
    expect(postMessage).not.toHaveBeenCalled();
    vi.runAllTimers();
    expect(postMessage).toHaveBeenCalledTimes(1);
    expect(postMessage).toHaveBeenCalledWith('hazir');
  });

  it('kanal yoksa (masaüstü, tarayıcı) hata vermez', () => {
    expect(() => signalFirstScreen()).not.toThrow();
  });
});
