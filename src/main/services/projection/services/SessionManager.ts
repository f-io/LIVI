import type { IPhoneDriver } from '../driver/IPhoneDriver'
import { logSessions } from './sessionLog'
import type { PersistedMediaPayload, PersistedNavigationPayload } from './types'

export type SessionProtocol = 'carplay' | 'androidauto' | 'dongle'
export type SessionTransport = 'usb' | 'wifi' | 'bt'
export type SessionState = 'active' | 'held'
export type VideoCodec = 'h264' | 'h265' | 'vp9' | 'av1'

export interface SessionDeviceIds {
  btMac?: string
  wifiMac?: string
  usbUdid?: string
  usbSerial?: string
  instanceId?: string
  controllerId?: string
  ip?: string
}

export interface KeyframeCache {
  codec?: VideoCodec
  codecData?: Buffer
  width?: number
  height?: number
  keyframe?: Buffer
}

export interface ProjectionSession {
  index: number
  protocol: SessionProtocol
  transport: SessionTransport
  device: SessionDeviceIds
  driver: IPhoneDriver
  state: SessionState
  video: { main: KeyframeCache; cluster: KeyframeCache }
  media: PersistedMediaPayload | null
  nav: PersistedNavigationPayload | null
}

export interface SessionManagerDeps {
  route: (driver: IPhoneDriver) => void
  onChange?: () => void
  onActiveChanged?: (next: ProjectionSession | null, prev: ProjectionSession | null) => void
}

function normMac(v?: string): string | undefined {
  return v ? v.toLowerCase() : v
}

function idsOverlap(a: SessionDeviceIds, b: SessionDeviceIds): boolean {
  return (
    (!!a.btMac && normMac(a.btMac) === normMac(b.btMac)) ||
    (!!a.wifiMac && normMac(a.wifiMac) === normMac(b.wifiMac)) ||
    (!!a.usbUdid && a.usbUdid === b.usbUdid) ||
    (!!a.usbSerial && a.usbSerial === b.usbSerial) ||
    (!!a.instanceId && a.instanceId === b.instanceId) ||
    (!!a.controllerId && a.controllerId === b.controllerId) ||
    (!!a.ip && a.ip === b.ip)
  )
}

export class SessionManager {
  private sessions: ProjectionSession[] = []
  private nextIndex = 1

  constructor(private readonly deps: SessionManagerDeps) {}

  private emitChange(reason = 'change'): void {
    logSessions(reason, this.sessions)
    this.deps.onChange?.()
  }

  dump(reason: string): void {
    logSessions(reason, this.sessions)
  }

  active(): ProjectionSession | null {
    return this.sessions.find((s) => s.state === 'active') ?? null
  }

  held(): ProjectionSession[] {
    return this.sessions.filter((s) => s.state === 'held')
  }

  all(): ProjectionSession[] {
    return this.sessions.slice()
  }

  byIndex(index: number): ProjectionSession | null {
    return this.sessions.find((s) => s.index === index) ?? null
  }

  byDriver(driver: IPhoneDriver): ProjectionSession | null {
    return this.sessions.find((s) => s.driver === driver) ?? null
  }

  byDevice(ids: SessionDeviceIds): ProjectionSession | null {
    return this.sessions.find((s) => idsOverlap(s.device, ids)) ?? null
  }

  private hasAnyId(ids: SessionDeviceIds): boolean {
    return !!(
      ids.btMac ||
      ids.wifiMac ||
      ids.usbUdid ||
      ids.usbSerial ||
      ids.instanceId ||
      ids.controllerId ||
      ids.ip
    )
  }

  byIdentity(protocol: SessionProtocol, ids: SessionDeviceIds): ProjectionSession | null {
    if (!this.hasAnyId(ids)) return null
    return this.sessions.find((s) => s.protocol === protocol && idsOverlap(s.device, ids)) ?? null
  }

  stateForDevice(ids: SessionDeviceIds): SessionState | null {
    return this.byDevice(ids)?.state ?? null
  }

  upsert(
    driver: IPhoneDriver,
    protocol: SessionProtocol,
    transport: SessionTransport,
    device: SessionDeviceIds
  ): ProjectionSession {
    const byId = this.byIdentity(protocol, device)
    const stealsWired =
      byId !== null && byId.driver !== driver && byId.transport === 'usb' && transport !== 'usb'
    let s = byId && !stealsWired ? byId : this.byDriver(driver)
    const created = !s
    // A wired driver adopting a non-wired session of the same phone is a transport
    // handover: wireless -> wired. Take over the entry and retire the wireless driver,
    // so the session keeps its index, media and nav instead of being torn down.
    let superseded: IPhoneDriver | null = null
    if (s && s.driver !== driver && transport === 'usb' && s.transport !== 'usb') {
      superseded = s.driver
      s.driver = driver
      if (s.state === 'active') this.deps.route(driver)
    }
    if (!s) {
      s = {
        index: this.nextIndex++,
        protocol,
        transport,
        device: {},
        driver,
        state: 'held',
        video: { main: {}, cluster: {} },
        media: null,
        nav: null
      }
      this.sessions.push(s)
    } else {
      s.protocol = protocol
      s.transport = transport
    }
    // Sticky identity merge: a partial update must never erase a known id with an empty
    // or undefined value (a wifi/Bonjour presence carries usbUdid:"" and must not clear it).
    for (const key of Object.keys(device) as (keyof SessionDeviceIds)[]) {
      const value = device[key]
      if (value === undefined || value === '') continue
      s.device[key] = key === 'btMac' || key === 'wifiMac' ? value.toLowerCase() : value
    }
    // CarPlay wiredness follows the accumulated udid, not the caller-passed transport.
    if (protocol === 'carplay') s.transport = s.device.usbUdid ? 'usb' : 'wifi'
    this.emitChange(
      `${created ? 'create' : 'upsert'} #${s.index} ${protocol} in=${JSON.stringify(device)}`
    )
    if (superseded) void superseded.close()
    return s
  }

  /** Hand a session to a new driver, keeping its identity, media, nav and state: the
   *  AirPlay transport adopts a session that was born at iAP2 identification. */
  reassignDriver(from: IPhoneDriver, to: IPhoneDriver): ProjectionSession | null {
    const s = this.byDriver(from)
    if (!s || from === to) return s
    s.driver = to
    if (s.state === 'active') this.deps.route(to)
    this.emitChange(`reassign #${s.index} ${s.protocol} → new driver`)
    return s
  }

  activate(index: number): ProjectionSession | null {
    const target = this.byIndex(index)
    if (!target) return null
    if (target.state === 'active') return target
    const prev = this.active()
    if (prev) prev.state = 'held'
    target.state = 'active'
    this.deps.route(target.driver)
    this.deps.onActiveChanged?.(target, prev ?? null)
    this.emitChange(
      `activate #${index} ${target.protocol} (prev ${prev ? `#${prev.index}` : 'none'})`
    )
    return target
  }

  activateNext(): void {
    const sorted = this.sessions.slice().sort((a, b) => a.index - b.index)
    if (sorted.length <= 1) return
    const current = this.active()
    const pos = current ? sorted.findIndex((s) => s === current) : -1
    const next = sorted[(pos + 1) % sorted.length]
    this.activate(next.index)
  }

  private removeAt(i: number): ProjectionSession {
    return this.sessions.splice(i, 1)[0]
  }

  private closeSession(s: ProjectionSession): void {
    const wasActive = s.state === 'active'
    const i = this.sessions.indexOf(s)
    if (i < 0) return
    this.removeAt(i)
    let reason = `close #${s.index}/${s.protocol}`
    if (wasActive) {
      const next = this.held()[0] ?? null
      reason += next ? ` → promote #${next.index}/${next.protocol}` : ' → IDLE'
      if (next) {
        next.state = 'active'
        this.deps.route(next.driver)
        this.deps.onActiveChanged?.(next, s)
      } else {
        this.deps.onActiveChanged?.(null, s)
      }
    } else {
      reason += ' (was held)'
    }
    this.emitChange(reason)
  }

  close(index: number): void {
    const s = this.byIndex(index)
    if (s) this.closeSession(s)
  }

  closeByDriver(driver: IPhoneDriver): void {
    const s = this.byDriver(driver)
    if (!s) console.log(`[SESSIONS] closeByDriver → NO matching session`)
    if (s) this.closeSession(s)
  }

  closeByDevice(ids: SessionDeviceIds): void {
    const s = this.byDevice(ids)
    if (s) this.closeSession(s)
  }

  closeByDeviceOnTransport(ids: SessionDeviceIds, transport: SessionTransport): void {
    const s = this.byDevice(ids)
    console.log(
      `[SESSIONS] closeByDeviceOnTransport in=${JSON.stringify(ids)} t=${transport} → match=${s ? `#${s.index}/${s.protocol}/t=${s.transport}` : 'NONE'}`
    )
    if (s && s.transport === transport) void s.driver.close()
  }

  clear(): void {
    this.sessions = []
    this.emitChange('clear')
  }
}
