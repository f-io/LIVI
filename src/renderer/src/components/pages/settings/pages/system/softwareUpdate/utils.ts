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

export const sameNightlyBuild = (installedSha?: string, releaseCommit?: string): boolean => {
  const a = (installedSha || '').trim().toLowerCase()
  const b = (releaseCommit || '').trim().toLowerCase()
  if (!a || !b || a === 'dev') return false
  const len = Math.min(a.length, b.length)
  return len > 0 && a.slice(0, len) === b.slice(0, len)
}

/** " (#123 · 0f404b2)" for a nightly, dropping whichever part is missing. */
export const buildTag = (run?: string, sha?: string): string => {
  const parts = [run ? `#${run}` : '', (sha || '').trim()].filter(Boolean)
  return parts.length ? ` (${parts.join(' · ')})` : ''
}

export const human = (n: number) =>
  n >= 1024 * 1024 ? `${(n / (1024 * 1024)).toFixed(1)} MB` : `${Math.round(n / 1024)} KB`
