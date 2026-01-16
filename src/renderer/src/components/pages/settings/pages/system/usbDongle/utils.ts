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

export function isDongleFwCheckResponse(v: unknown): v is DongleFwCheckResponse {
  if (!v || typeof v !== 'object') return false
  const o = v as any
  return typeof o.ok === 'boolean' && o.raw && typeof o.raw === 'object' && 'err' in o.raw
}
