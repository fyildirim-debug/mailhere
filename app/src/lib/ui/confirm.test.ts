import { describe, expect, it } from 'vitest';
import { answerConfirm, askConfirm, confirmState } from './confirm.svelte';

const question = { title: 'Silinsin mi?', body: 'Bu işlem geri alınamaz.', confirmLabel: 'Kalıcı sil', danger: true };

describe('onay penceresi durumu', () => {
  it('soru açılır; yanıt sözü çözer ve pencereyi kapatır', async () => {
    const asked = askConfirm(question);
    expect(confirmState.current).toMatchObject(question);
    answerConfirm(true);
    expect(confirmState.current).toBeNull();
    expect(await asked).toBe(true);
  });

  it('vazgeçmek false döndürür; açık soru yokken yanıt etkisizdir', async () => {
    const asked = askConfirm(question);
    answerConfirm(false);
    expect(await asked).toBe(false);
    answerConfirm(true);
    expect(confirmState.current).toBeNull();
  });

  it('yeni soru eskisini vazgeçilmiş sayar; her soru kendi anahtarını alır', async () => {
    const first = askConfirm(question);
    const firstKey = confirmState.current!.key;
    const second = askConfirm({ ...question, title: 'Çöp boşaltılsın mı?' });
    expect(await first).toBe(false);
    expect(confirmState.current!.title).toBe('Çöp boşaltılsın mı?');
    expect(confirmState.current!.key).not.toBe(firstKey);
    answerConfirm(true);
    expect(await second).toBe(true);
  });
});
