/** Absolute http address for `input`, or null when it is none.
 *  A value without a scheme is read as http, so `10.0.0.9` and `this.local`
 *  reach the same host as `http://10.0.0.9` and `http://this.local`. */
export function normalizeHttpUrl(input: string): string | null {
  const text = input.trim()
  if (!text) return null

  const hasScheme = /^[a-z][a-z0-9+.-]*:/i.test(text)
  let url: URL
  try {
    url = new URL(hasScheme ? text : `http://${text}`)
  } catch {
    return null
  }

  if (url.protocol !== 'http:' && url.protocol !== 'https:') return null
  // A single label without a scheme is still being typed, not an address
  if (!hasScheme && !url.hostname.includes('.') && url.hostname !== 'localhost') return null

  return url.toString()
}

/** True for an empty field or an address `normalizeHttpUrl` accepts. */
export function isHttpUrlInput(value: unknown): boolean {
  if (typeof value !== 'string') return false
  return value.trim() === '' || normalizeHttpUrl(value) !== null
}
