import { promises as fs } from 'node:fs'
import path from 'node:path'
import { app } from 'electron'

export type LinkTransport = 'bt' | 'wifi' | 'usb'

export interface DeviceEntry {
  btMac?: string
  wifiMac?: string
  usbUdid?: string
  instanceId?: string
  name?: string
  model?: string
  hostname?: string
  protocol?: 'carplay' | 'androidauto'
  lastTransport?: string
  lastSeen?: number
  presence: { bt?: boolean; wifi?: boolean; usb?: boolean }
  currentIp?: string
  batteryLevel?: number
  batteryCritical?: boolean
  batteryCharging?: boolean
  batteryTimeRemaining?: number
  signalStrength?: number
  carrierName?: string
}

type StoredDevice = {
  btMac?: string
  wifiMac?: string
  usbUdid?: string
  instanceId?: string
  name?: string
  model?: string
  hostname?: string
  protocol?: 'carplay' | 'androidauto'
  lastTransport?: string
  lastSeen?: number
}

export type { DeviceView } from '@shared/types'

type Ids = { btMac?: string; wifiMac?: string; usbUdid?: string; instanceId?: string; ip?: string }

const IDENTITY_KEYS = [
  'btMac',
  'wifiMac',
  'usbUdid',
  'instanceId',
  'name',
  'model',
  'hostname',
  'protocol',
  'lastTransport',
  'lastSeen'
] as const

export class DeviceRegistry {
  private entries: DeviceEntry[] = []
  private loadOk = false
  private readonly fileOverride?: string
  private resolvedFile: string | null = null
  private saveTimer: NodeJS.Timeout | null = null
  private changeCb: (() => void) | null = null
  private notifyTimer: NodeJS.Timeout | null = null

  onChange(cb: () => void): void {
    this.changeCb = cb
  }

  private notify(): void {
    if (this.notifyTimer) return
    this.notifyTimer = setTimeout(() => {
      this.notifyTimer = null
      this.changeCb?.()
    }, 200)
  }

  constructor(file?: string) {
    this.fileOverride = file
  }

  private get file(): string {
    if (!this.resolvedFile) {
      this.resolvedFile = this.fileOverride ?? path.join(app.getPath('userData'), 'devices.json')
    }
    return this.resolvedFile
  }

  async load(): Promise<void> {
    try {
      const stored = JSON.parse(await fs.readFile(this.file, 'utf8')) as StoredDevice[]
      this.entries = stored.map((s) => ({ ...s, presence: {} }))
      this.loadOk = true
    } catch (e) {
      this.entries = []
      const code = (e as NodeJS.ErrnoException).code
      if (code === 'ENOENT') {
        this.loadOk = true
      } else {
        this.loadOk = false
        console.warn(
          `[DeviceRegistry] load failed (${code ?? 'parse error'}); keeping ${this.file} untouched`
        )
      }
    }
    this.notify()
  }

  private stableKey(e: DeviceEntry): string | null {
    return e.btMac ?? e.usbUdid ?? e.wifiMac ?? null
  }

  private persist(): void {
    if (!this.loadOk) return
    if (this.saveTimer) return
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null
      const stored = this.entries
        .filter((e) => this.stableKey(e) && (e.protocol || e.name))
        .map((e) => {
          const out: StoredDevice = {}
          for (const k of IDENTITY_KEYS) {
            if (e[k] !== undefined) (out as Record<string, unknown>)[k] = e[k]
          }
          return out
        })
      fs.writeFile(this.file, JSON.stringify(stored, null, 2)).catch(() => {})
    }, 500)
  }

  private matchAll(ids: Ids): DeviceEntry[] {
    return this.entries.filter(
      (e) =>
        (!!ids.btMac && e.btMac === ids.btMac) ||
        (!!ids.wifiMac && e.wifiMac === ids.wifiMac) ||
        (!!ids.usbUdid && e.usbUdid === ids.usbUdid) ||
        (!!ids.instanceId && e.instanceId === ids.instanceId) ||
        (!!ids.ip && e.currentIp === ids.ip)
    )
  }

  private mergeEntries(list: DeviceEntry[]): DeviceEntry {
    const primary = list[0]
    for (let i = 1; i < list.length; i++) {
      const o = list[i]
      primary.btMac ??= o.btMac
      primary.wifiMac ??= o.wifiMac
      primary.usbUdid ??= o.usbUdid
      primary.instanceId ??= o.instanceId
      primary.name ??= o.name
      primary.model ??= o.model
      primary.hostname ??= o.hostname
      primary.protocol ??= o.protocol
      primary.lastTransport ??= o.lastTransport
      primary.currentIp ??= o.currentIp
      primary.presence = {
        bt: primary.presence.bt || o.presence.bt,
        wifi: primary.presence.wifi || o.presence.wifi,
        usb: primary.presence.usb || o.presence.usb
      }
      primary.lastSeen = Math.max(primary.lastSeen ?? 0, o.lastSeen ?? 0)
      const idx = this.entries.indexOf(o)
      if (idx >= 0) this.entries.splice(idx, 1)
    }
    return primary
  }

  private upsert(ids: Ids): DeviceEntry {
    const matches = this.matchAll(ids)
    if (!matches.length) {
      const e: DeviceEntry = { presence: {} }
      this.entries.push(e)
      return e
    }
    return matches.length === 1 ? matches[0] : this.mergeEntries(matches)
  }

  private match(ids: Ids): DeviceEntry | null {
    const matches = this.matchAll(ids)
    if (!matches.length) return null
    return matches.length === 1 ? matches[0] : this.mergeEntries(matches)
  }

  noteLink(ids: Ids, transport: LinkTransport, up: boolean): void {
    const e = this.match(ids)
    if (!e) return
    if (up) {
      e.presence[transport] = true
      if (ids.ip) e.currentIp = ids.ip
      e.lastSeen = Date.now()
    } else {
      e.presence[transport] = false
    }
    this.notify()
  }

  clearPresence(ids: Ids): void {
    const e = this.match(ids)
    if (!e) return
    e.presence = {}
    e.currentIp = undefined
    this.notify()
  }

  noteDevice(d: {
    btMac?: string
    wifiMac?: string
    ip?: string
    usbUdid?: string
    instanceId?: string
    name?: string
    model?: string
    protocol?: 'carplay' | 'androidauto'
    transport?: string
  }): void {
    const e = this.upsert({
      btMac: d.btMac,
      wifiMac: d.wifiMac,
      usbUdid: d.usbUdid,
      instanceId: d.instanceId,
      ip: d.ip
    })
    if (d.btMac) e.btMac = d.btMac
    if (d.wifiMac) e.wifiMac = d.wifiMac
    if (d.usbUdid) e.usbUdid = d.usbUdid
    if (d.instanceId) e.instanceId = d.instanceId
    if (d.name) e.name = d.name
    if (d.model) e.model = d.model
    if (d.transport) e.lastTransport = d.transport
    if (d.ip) e.currentIp = d.ip
    e.protocol = d.protocol ?? e.protocol ?? 'carplay'
    e.presence[d.transport === 'usb' ? 'usb' : 'wifi'] = true
    e.lastSeen = Date.now()
    this.persist()
    this.notify()
  }

  noteStatus(
    ids: Ids,
    s: {
      batteryLevel?: number
      batteryCritical?: boolean
      batteryCharging?: boolean
      batteryTimeRemaining?: number
      signalStrength?: number
      carrierName?: string
    }
  ): void {
    const e = this.match(ids)
    if (!e) return
    if (s.batteryLevel !== undefined) e.batteryLevel = s.batteryLevel
    if (s.batteryCritical !== undefined) e.batteryCritical = s.batteryCritical
    if (s.batteryCharging !== undefined) e.batteryCharging = s.batteryCharging
    if (s.batteryTimeRemaining !== undefined) e.batteryTimeRemaining = s.batteryTimeRemaining
    if (s.signalStrength !== undefined) e.signalStrength = s.signalStrength
    if (s.carrierName !== undefined) e.carrierName = s.carrierName
    this.notify()
  }

  deviceId(e: DeviceEntry): string {
    return e.btMac ?? e.usbUdid ?? e.wifiMac ?? e.instanceId ?? ''
  }

  list(): DeviceEntry[] {
    return this.entries.map((e) => ({ ...e }))
  }

  noteName(btMac: string, name: string): void {
    if (!btMac || !name) return
    const up = btMac.toUpperCase()
    const e = this.entries.find((x) => x.btMac?.toUpperCase() === up)
    if (!e || e.name === name) return
    e.name = name
    this.persist()
    this.notify()
  }

  forget(id: string): DeviceEntry | undefined {
    const idx = this.entries.findIndex(
      (e) => e.btMac === id || e.usbUdid === id || e.wifiMac === id
    )
    if (idx < 0) return undefined
    const [removed] = this.entries.splice(idx, 1)
    this.persist()
    this.notify()
    return removed
  }
}
