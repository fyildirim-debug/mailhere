/** Yerel kimlik (imza, şablon): rastgele 128 bit, onaltılık. */
export function newLocalId(): string {
  const b = new Uint8Array(16);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}
