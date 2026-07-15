export function isPhoneLikeCod(cod: number | undefined): boolean {
  if (typeof cod !== 'number' || cod <= 0) return true
  // 0x04 = Audio/Video CoD major class
  return ((cod >> 8) & 0x1f) !== 0x04
}
