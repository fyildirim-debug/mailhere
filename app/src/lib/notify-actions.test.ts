import { describe, expect, it } from 'vitest';
import { defaultSettings, NOTIFY_ACTIONS_MAX } from './api';
import { NOTIFY_ACTION_INFO, NOTIFY_ACTION_ORDER, shownNotifyActions, toggleNotifyAction } from './notify-actions';
import { en } from './i18n/en';
import { tr } from './i18n/tr';

describe('bildirim düğmeleri', () => {
  it('varsayılan Yanıtla, Sil, İstenmeyen; en çok üç', () => {
    expect(defaultSettings.notifyActions).toEqual(['reply', 'trash', 'spam']);
    expect(NOTIFY_ACTIONS_MAX).toBe(3);
  });

  it('seçim kanonik sırada dizilir, üçten fazlası eklenmez', () => {
    let list = toggleNotifyAction(['spam', 'reply'], 'markRead', true);
    expect(list).toEqual(['reply', 'markRead', 'spam']);
    // Üç seçiliyken dördüncü eklenmez.
    expect(toggleNotifyAction(list, 'archive', true)).toEqual(['reply', 'markRead', 'spam']);
    list = toggleNotifyAction(list, 'reply', false);
    expect(list).toEqual(['markRead', 'spam']);
    list = toggleNotifyAction(list, 'archive', true);
    expect(list).toEqual(['markRead', 'archive', 'spam']);
    // Var olanı yeniden açmak ya da olmayanı kapatmak listeyi değiştirmez.
    expect(toggleNotifyAction(list, 'archive', true)).toEqual(list);
    expect(toggleNotifyAction(list, 'trash', false)).toEqual(list);
    // Hepsi kapanabilir: kartta yalnız "Aç" kalır.
    expect(['markRead', 'archive', 'spam'].reduce((l, a) => toggleNotifyAction(l, a as never, false), list)).toEqual([]);
  });

  it('kartta bilinmeyen ve yinelenen düğme gösterilmez, sıra korunur', () => {
    expect(shownNotifyActions(['spam', 'bilinmeyen', 'spam', 'reply', 'trash', 'archive'])).toEqual(['spam', 'reply', 'trash']);
    expect(shownNotifyActions(undefined)).toEqual([]);
  });

  it('her düğmenin iki dilde metni var; erişilebilir ad görünen etiketi içerir', () => {
    for (const a of NOTIFY_ACTION_ORDER) {
      const info = NOTIFY_ACTION_INFO[a];
      for (const dict of [tr, en] as Record<string, string>[]) {
        const label = dict[info.label];
        const aria = dict[info.aria];
        expect(label, `${a} etiketi`).toBeTruthy();
        expect(dict[info.hint], `${a} açıklaması`).toBeTruthy();
        expect(aria.toLocaleLowerCase('tr')).toContain(label.toLocaleLowerCase('tr'));
        if (info.done) expect(dict[info.done]).toBeTruthy();
      }
    }
  });
});
