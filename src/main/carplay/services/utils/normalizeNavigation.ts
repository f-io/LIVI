import type { PersistedNavigationPayload, NaviBag } from '../types'

type NavMsg = {
  metaType?: unknown
  navi?: unknown
  rawUtf8?: unknown
}

function safeParseJsonObject(input: unknown): Record<string, unknown> {
  if (typeof input !== 'string' || input.length === 0) return {}
  try {
    const parsed = JSON.parse(input) as unknown
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>
    }
    return {}
  } catch {
    return {}
  }
}

function asNaviBag(input: unknown): NaviBag {
  if (!input || typeof input !== 'object' || Array.isArray(input)) return {}
  return input as NaviBag
}

function readNumber(o: unknown, key: string): number | null {
  if (!o || typeof o !== 'object' || Array.isArray(o)) return null
  const v = (o as Record<string, unknown>)[key]
  return typeof v === 'number' && Number.isFinite(v) ? v : null
}

export function normalizeNavigationPayload(
  existing: PersistedNavigationPayload,
  navMsg: NavMsg
): PersistedNavigationPayload {
  const metaType = typeof navMsg.metaType === 'number' ? navMsg.metaType : existing.metaType

  const rawObj = safeParseJsonObject(navMsg.rawUtf8)
  const msgNavi = asNaviBag(navMsg.navi)

  // Patch
  const patch: NaviBag = { ...rawObj, ...msgNavi }

  // Flush signal
  const isFlush = metaType === 200 && readNumber(patch, 'NaviStatus') === 0

  // Flush only on explicit signal (metaType 200 + NaviStatus 0), otherwise always merge
  const nextNavi: NaviBag | null = isFlush ? { ...patch } : { ...(existing.navi ?? {}), ...patch }

  return {
    metaType,
    navi: nextNavi,
    rawUtf8: '',
    error: false
  }
}
