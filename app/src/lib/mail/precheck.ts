// Gönderim öncesi denetimler: geçersiz alıcı parçası, unutulmuş ek, boş konu.
// Saf işlevler; yazma penceresi sonucu kullanıcıya gösterir.

import { parseAddress } from '../util/recipients';

/** Alıcı alanına yazılan metni parçalarına ayırır (virgül, noktalı virgül, satır). */
export function splitRecipients(text: string): string[] {
  return text
    .split(/[,;\n]+/)
    .map((x) => x.trim())
    .filter(Boolean);
}

/** Alıcı alanında adres olarak çözülemeyen parçalar. */
export function invalidRecipients(text: string): string[] {
  return splitRecipients(text).filter((p) => !parseAddress(p));
}

// Ek anlamına gelen sözcükler. Bu depoda yazılmış kısa bir listedir; bilerek
// dardır: "ek" (ek olarak), "ekle" (listeye ekle) ya da "attach" (lütfen
// ekleyin) gibi başka anlama da gelen sözcükler yanlış uyarı verdiği için yok.
const WORDS_TR = new Set([
  'ekte',
  'ekteki',
  'ektedir',
  'ekli',
  'eklidir',
  'ekledim',
  'ekledik',
  'ekliyorum',
  'ekliyoruz',
  'ilişikte',
  'ilişikteki',
  'iliştirdim',
  'iliştiriyorum',
  // Türkçe harfsiz yazım.
  'ilisikte',
  'ilisikteki',
]);

const WORDS_EN = new Set(['attached', 'attaching', 'attachment', 'attachments', 'enclosed', 'enclosing']);

/** İletilen posta ayracı: iki yanı tirelerle çevrili satır ("---------- İletilen posta ----------"). */
const DIVIDER = /^-{5,}.*-{5,}$/;

/**
 * Yazarın kendi metni: alıntılanan önceki ileti sayılmaz. Atılanlar: `>` ile
 * başlayan satırlar, alıntı bloğundan hemen önceki "… şunu yazdı:" satırı ve
 * iletilen postanın ayracından sonraki her şey.
 */
export function ownText(body: string): string {
  const lines = body.replace(/\r\n?/g, '\n').split('\n');
  const out: string[] = [];
  for (const line of lines) {
    const s = line.trim();
    if (DIVIDER.test(s)) break;
    if (s.startsWith('>')) {
      // Alıntının başlığı: bloktan önceki son dolu satır iki noktayla bitiyorsa o da alıntıdır.
      let i = out.length - 1;
      while (i >= 0 && !out[i].trim()) i--;
      if (i >= 0 && out[i].trim().endsWith(':')) out.length = i;
      continue;
    }
    out.push(line);
  }
  return out.join('\n');
}

/**
 * Konuda ya da yazarın kendi metninde ek anlamına gelen ilk sözcük (yazıldığı
 * haliyle); yoksa null. Yanıt ve iletmede konu özgün postadan geldiği için
 * çağıran, değiştirilmemiş konuyu boş verir.
 */
export function attachmentWord(subject: string, body: string): string | null {
  const text = `${subject}\n${ownText(body)}`;
  for (const word of text.split(/[^\p{L}\p{N}]+/u)) {
    if (!word) continue;
    // Türkçe küçültme İ → i, I → ı yapar; İngilizce sözcük için ayrıca yalın küçültme gerekir.
    if (WORDS_TR.has(word.toLocaleLowerCase('tr')) || WORDS_EN.has(word.toLowerCase())) return word;
  }
  return null;
}

export interface SendIssues {
  /** Konu boş. */
  noSubject: boolean;
  /** Ek yokken metinde geçen ek sözcüğü; yoksa null. */
  attachmentWord: string | null;
}

/**
 * Göndermeden önce sorulacak durumlar; sorulacak bir şey yoksa null.
 * `subjectIsOwn`: konuyu kullanıcı yazdı (yanıtta özgün konu olduğu gibi duruyorsa false).
 */
export function sendIssues(msg: { subject: string; body: string; hasAttachments: boolean; subjectIsOwn: boolean }): SendIssues | null {
  const noSubject = !msg.subject.trim();
  const word = msg.hasAttachments ? null : attachmentWord(msg.subjectIsOwn ? msg.subject : '', msg.body);
  return noSubject || word ? { noSubject, attachmentWord: word } : null;
}
