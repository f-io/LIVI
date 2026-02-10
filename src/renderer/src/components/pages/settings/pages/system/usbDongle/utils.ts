import { BoxInfoPayload, DongleFwCheckResponse } from './types'

export function normalizeBoxInfo(input: unknown): BoxInfoPayload | null {
  if (!input) return null

  if (typeof input === 'object') {
    return input as BoxInfoPayload
  }

  if (typeof input === 'string') {
    const s = input.trim()
    if (!s) return null
    try {
      const parsed = JSON.parse(s)
      if (parsed && typeof parsed === 'object') return parsed as BoxInfoPayload
    } catch {
      // ignore
    }
  }

  return null
}

export function fmt(v: unknown): string | null {
  if (v == null) return null
  const s = String(v).trim()
  return s.length ? s : null
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export function isDongleFwCheckResponse(v: unknown): v is DongleFwCheckResponse {
  if (!isRecord(v)) return false

  const ok = v.ok
  const raw = v.raw

  if (typeof ok !== 'boolean') return false
  if (!isRecord(raw)) return false

  return 'err' in raw
}
