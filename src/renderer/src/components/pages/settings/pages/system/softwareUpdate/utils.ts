export const parseSemver = (v?: string): number[] | null => {
  if (!v) return null
  const m = v.trim().match(/^(\d+)\.(\d+)\.(\d+)$/)
  if (!m) return null
  return [parseInt(m[1], 10), parseInt(m[2], 10), parseInt(m[3], 10)]
}

export const cmpSemver = (a: number[], b: number[]) => {
  for (let i = 0; i < 3; i++) {
    if ((a[i] || 0) < (b[i] || 0)) return -1
    if ((a[i] || 0) > (b[i] || 0)) return 1
  }
  return 0
}
