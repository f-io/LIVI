import type { DongleFwApiRaw } from '@shared/types'

export type { BoxInfoPayload, DevListEntry } from '@shared/types'

export type LocalFwStatus =
  | { ok: true; ready: true; path: string; bytes: number; model: string; latestVer?: string }
  | { ok: true; ready: false; reason: string }
  | { ok: false; error: string }

export type DongleFwCheckResponse = {
  ok: boolean
  hasUpdate: boolean
  size: string | number
  token?: string
  request?: Record<string, unknown> & { local?: LocalFwStatus }
  raw: DongleFwApiRaw
  error?: string
}
