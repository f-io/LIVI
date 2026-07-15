// Prefixes every main-process console line with a wall-clock timestamp (HH:MM:SS.mmm).
// Imported first in index.ts so relayed [helper] lines get stamped too.
function stamp(): string {
  const d = new Date()
  const p = (n: number, w = 2): string => String(n).padStart(w, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`
}

const base = {
  log: console.log.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  debug: console.debug.bind(console)
}

console.log = (...a: unknown[]): void => base.log(`[${stamp()}]`, ...a)
console.info = (...a: unknown[]): void => base.info(`[${stamp()}]`, ...a)
console.warn = (...a: unknown[]): void => base.warn(`[${stamp()}]`, ...a)
console.error = (...a: unknown[]): void => base.error(`[${stamp()}]`, ...a)
console.debug = (...a: unknown[]): void => base.debug(`[${stamp()}]`, ...a)
