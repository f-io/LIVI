export type Row = {
  label: string
  value: string | number | null | undefined
  mono?: boolean
  tooltip?: string
}

export type DevListEntry = {
  id?: string
  type?: string
  name?: string
  index?: string | number
  time?: string
  rfcomm?: string | number
}

export type BoxInfoPayload = {
  uuid?: string
  MFD?: string
  boxType?: string
  OemName?: string
  productType?: string
  HiCar?: number
  supportLinkType?: string
  supportFeatures?: string
  hwVersion?: string
  WiFiChannel?: number
  CusCode?: string
  DevList?: DevListEntry[]
  ChannelList?: string
}

export type DongleFwApiRaw = {
  err: number
  token?: string
  ver?: string
  size?: string | number
  id?: string
  notes?: string
  msg?: string
  error?: string
}

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

export type FwPhase = 'start' | 'download' | 'ready' | 'error' | 'upload'

export type FwProgress = {
  percent?: number
  received?: number
  total?: number
}

export type FwDialogState = {
  open: boolean
  phase: FwPhase
  progress: FwProgress
  error: string
  message: string
  inFlight: boolean
}
