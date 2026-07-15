import {
  BluetoothPairedList,
  BoxInfo,
  BoxUpdateProgress,
  GnssData,
  type Message,
  type PhoneType,
  Plugged,
  SoftwareVersion,
  Unplugged
} from '@projection/messages/readable'
import type { DevListEntry } from '@shared/types'
import type { DongleDriver } from './dongleDriver.js'

export type DongleInfo = {
  dongleFwVersion: string | undefined
  boxInfo: unknown
}

export type DongleAdapterDeps = {
  emitMessage: (msg: Message) => void
  emitConnected: (phoneType: PhoneType) => void
  emitDisconnected: () => void
  emitDevicePresence: (info: { text: string }) => void
  emitDeviceStatus: (status: Record<string, unknown>) => void
  emitDongleInfo: (info: DongleInfo) => void
  emitDevices: () => void
  emitFwUpdateProgress: (progress: number) => void
  emitBluetoothPairedList: (raw: string) => void
}

export class DongleAdapter {
  private dongleFwVersion?: string
  private boxInfo?: unknown
  private dongleDevList: DevListEntry[] = []
  private dongleConnectedMac = ''
  private donglePairedRaw = ''
  private lastDongleInfoEmitKey = ''

  constructor(
    private readonly dongle: DongleDriver,
    private readonly deps: DongleAdapterDeps
  ) {}

  wire(): void {
    this.dongle.on('message', this.onMessage)
  }

  unwire(): void {
    this.dongle.off('message', this.onMessage)
  }

  getDongleDevList(): DevListEntry[] {
    return this.dongleDevList
  }

  getDongleConnectedMac(): string {
    return this.dongleConnectedMac
  }

  getDonglePairedRaw(): string {
    return this.donglePairedRaw
  }

  getDongleInfo(): DongleInfo {
    return { dongleFwVersion: this.dongleFwVersion, boxInfo: this.boxInfo }
  }

  private readonly onMessage = (msg: Message): void => {
    if (msg instanceof SoftwareVersion) {
      this.dongleFwVersion = msg.version
      this.emitDongleInfoIfChanged()
      return
    }

    if (msg instanceof BoxInfo) {
      const settings = msg.settings as { DevList?: Array<Record<string, unknown>> }
      if (Array.isArray(settings.DevList)) {
        this.dongleDevList = settings.DevList.map((entry) => ({
          ...(entry as DevListEntry),
          source: 'dongle' as const
        }))
        settings.DevList = this.dongleDevList as unknown as Array<Record<string, unknown>>
      }
      const rawBtMac = (msg.settings as { btMacAddr?: unknown }).btMacAddr
      if (typeof rawBtMac === 'string' && rawBtMac.trim()) {
        this.dongleConnectedMac = rawBtMac.trim()
      }
      this.boxInfo = mergePreferExisting(this.boxInfo, msg.settings)
      this.emitDongleInfoIfChanged()
      this.deps.emitDevices()
      return
    }

    if (msg instanceof GnssData) {
      this.deps.emitMessage(msg)
      return
    }

    if (msg instanceof BluetoothPairedList) {
      this.donglePairedRaw = msg.data
      this.deps.emitBluetoothPairedList(this.donglePairedRaw)
      return
    }

    if (msg instanceof Plugged) {
      this.deps.emitConnected(msg.phoneType)
      return
    }

    if (msg instanceof Unplugged) {
      this.dongleConnectedMac = ''
      this.dongleDevList = []
      if (isRecord(this.boxInfo)) {
        this.boxInfo = { ...this.boxInfo, btMacAddr: '' }
      }
      this.deps.emitDisconnected()
      this.deps.emitDongleInfo({ dongleFwVersion: this.dongleFwVersion, boxInfo: this.boxInfo })
      this.deps.emitDevices()
      return
    }

    if (msg instanceof BoxUpdateProgress) {
      this.deps.emitFwUpdateProgress(msg.progress)
      return
    }
  }

  private emitDongleInfoIfChanged(): void {
    let boxKey = ''
    if (this.boxInfo != null) {
      try {
        boxKey = JSON.stringify(this.boxInfo)
      } catch {
        boxKey = String(this.boxInfo)
      }
    }

    const key = `${this.dongleFwVersion ?? ''}||${boxKey}`
    if (key === this.lastDongleInfoEmitKey) return
    this.lastDongleInfoEmitKey = key

    this.deps.emitDongleInfo({ dongleFwVersion: this.dongleFwVersion, boxInfo: this.boxInfo })
  }
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}

function asObject(input: unknown): Record<string, unknown> | null {
  if (!input) return null

  if (typeof input === 'object' && input !== null) return input as Record<string, unknown>

  if (typeof input === 'string') {
    const s = input.trim()
    if (!s) return null
    try {
      const parsed = JSON.parse(s)
      if (parsed && typeof parsed === 'object') return parsed as Record<string, unknown>
    } catch {}
  }

  return null
}

function isMeaningful(v: unknown): boolean {
  if (v == null) return false
  if (typeof v === 'string') return v.trim().length > 0
  return true
}

function mergePreferExisting(prev: unknown, next: unknown): unknown {
  const p = asObject(prev)
  const n = asObject(next)

  if (!p && !n) return next ?? prev
  if (!p && n) return next
  if (p && !n) return prev

  const out: Record<string, unknown> = { ...p }

  for (const [k, v] of Object.entries(n as Record<string, unknown>)) {
    if (isMeaningful(v)) {
      out[k] = v
    } else {
      if (!(k in out)) out[k] = v
    }
  }

  return out
}
