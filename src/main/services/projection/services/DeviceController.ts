import type { DevListEntry } from '@shared/types'
import type { AaBtSockClient } from '../driver/aa/AaBtSockClient'
import type { DeviceRegistry, DeviceView } from './DeviceRegistry'
import type { ProjectionSession, SessionManager } from './SessionManager'
import type { ProjectionEvent } from './types'
import { isPhoneLikeCod } from './utils/isPhoneLikeCod'

export type DeviceControllerDeps = {
  deviceRegistry: DeviceRegistry
  sessions: () => SessionManager
  getDongleSession: () => ProjectionSession | null
  aaBtSock: AaBtSockClient
  getAaBtName: (macUpper: string) => string | undefined
  getAaBtMac: () => string
  getDongleConnectedMac: () => string
  getDongleDevList: () => DevListEntry[]
  emit: (payload: ProjectionEvent) => void
}

// Builds the unified device-picker view (native registry + dongle list) and
// serves the picker commands: select routes to a session, forget unpairs BT.
export class DeviceController {
  private lastDeviceViewsSig = ''

  constructor(private readonly deps: DeviceControllerDeps) {}

  getDevices(): DeviceView[] {
    return this.buildDeviceViews()
  }

  forgetDevice(id: string): { ok: boolean } {
    const e = this.deps.deviceRegistry.forget(id)
    if (!e) return { ok: false }
    const mac = e.btMac
    if (mac) {
      void this.deps.aaBtSock
        .disconnect(mac)
        .catch(() => {})
        .then(() => this.deps.aaBtSock.remove(mac))
        .catch((err) =>
          console.warn(`[DeviceController] forget ${mac} unpair failed: ${(err as Error).message}`)
        )
    }
    return { ok: true }
  }

  selectDevice(id: string): { ok: boolean } {
    if (this.deps.getDongleDevList().some((d) => d.id === id)) {
      const ds = this.deps.getDongleSession()
      if (!ds) return { ok: false }
      this.deps.sessions().activate(ds.index)
      return { ok: true }
    }
    const reg = this.deps.deviceRegistry
    const e = reg.list().find((x) => reg.deviceId(x) === id)
    const ids = e
      ? {
          btMac: e.btMac,
          wifiMac: e.wifiMac,
          usbUdid: e.usbUdid,
          instanceId: e.instanceId,
          ip: e.currentIp
        }
      : { btMac: id, wifiMac: id, usbUdid: id, instanceId: id }
    const s = this.deps.sessions().byDevice(ids)
    console.log(`[DeviceController] selectDevice ${id} -> session #${s?.index ?? 'none'}`)
    if (!s) return { ok: false }
    this.deps.sessions().activate(s.index)
    return { ok: true }
  }

  emitDevices(): void {
    const views = this.buildDeviceViews()
    const sig = JSON.stringify(views)
    if (sig === this.lastDeviceViewsSig) return
    this.lastDeviceViewsSig = sig
    this.deps.emit({ type: 'devices', payload: views })
  }

  private buildDeviceViews(): DeviceView[] {
    const out: DeviceView[] = []
    const lastSeenOf = new Map<DeviceView, number>()
    const reg = this.deps.deviceRegistry
    const ordered = this.deps
      .sessions()
      .all()
      .slice()
      .sort((a, b) => a.index - b.index)
    const cpBtMacs = new Set(
      ordered
        .filter((s) => s.protocol === 'carplay' && s.device.btMac)
        .map((s) => (s.device.btMac as string).toUpperCase())
    )
    for (const e of reg.list()) {
      const id = reg.deviceId(e)
      if (!id || !(e.protocol || e.name)) continue
      const ids = {
        btMac: e.btMac,
        wifiMac: e.wifiMac,
        usbUdid: e.usbUdid,
        instanceId: e.instanceId,
        ip: e.currentIp
      }
      const st = this.deps.sessions().stateForDevice(ids)
      const sess = this.deps.sessions().byDevice(ids)
      const status: DeviceView['status'] =
        st === 'active' ? 'active' : st === 'held' || e.presence.wifi ? 'available' : 'offline'
      let nameBt = e.btMac
      if (e.protocol === 'androidauto' && nameBt && cpBtMacs.has(nameBt.toUpperCase())) {
        const aaBt = this.deps.getAaBtMac()
        nameBt = aaBt && !cpBtMacs.has(aaBt.toUpperCase()) ? aaBt : undefined
      }
      const view: DeviceView = {
        id,
        name: (nameBt ? this.deps.getAaBtName(nameBt.toUpperCase()) : undefined) || e.name,
        model: e.model,
        protocol: e.protocol,
        lastTransport: e.lastTransport,
        status,
        source: 'native',
        batteryLevel: e.batteryLevel,
        batteryCharging: e.batteryCharging,
        signalStrength: e.signalStrength,
        carrierName: e.carrierName,
        session: sess ? ordered.indexOf(sess) + 1 || undefined : undefined
      }
      out.push(view)
      lastSeenOf.set(view, e.lastSeen ?? 0)
    }
    const connectedDongleMac = this.deps.getDongleConnectedMac().trim().toUpperCase()
    const dongleSession = this.deps.getDongleSession()
    const dongleActive = dongleSession?.state === 'active'
    const phoneLikeDongle = this.deps
      .getDongleDevList()
      .filter((d): d is DevListEntry & { id: string } => !!d.id && isPhoneLikeCod(d.class))
    for (const d of phoneLikeDongle) {
      const isConnected =
        (!!connectedDongleMac && d.id.trim().toUpperCase() === connectedDongleMac) ||
        !!d.connected ||
        (dongleActive && phoneLikeDongle.length === 1)
      const view: DeviceView = {
        id: d.id,
        name: d.name || d.id,
        protocol: d.type === 'AndroidAuto' ? 'androidauto' : 'carplay',
        status: !dongleSession ? 'offline' : dongleActive && isConnected ? 'active' : 'available',
        source: 'dongle'
      }
      out.push(view)
      lastSeenOf.set(view, 0)
    }
    out.sort((a, b) => {
      const as = a.session
      const bs = b.session
      if (as !== undefined || bs !== undefined) {
        if (as === undefined) return 1
        if (bs === undefined) return -1
        return as - bs
      }
      const rank = (v: DeviceView): number =>
        v.status === 'active' ? 0 : v.status === 'available' ? 1 : 2
      const ra = rank(a)
      const rb = rank(b)
      if (ra !== rb) return ra - rb
      return (lastSeenOf.get(b) ?? 0) - (lastSeenOf.get(a) ?? 0)
    })
    return out
  }
}
