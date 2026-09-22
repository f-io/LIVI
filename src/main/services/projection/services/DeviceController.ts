import { SendDisconnectPhone } from '@projection/messages/sendable'
import type { DevListEntry } from '@shared/types'
import type { BluezDeviceClient } from '../bt/BluezDeviceClient'
import type { DeviceRegistry, DeviceView } from './DeviceRegistry'
import type { ProjectionSession, SessionManager } from './SessionManager'
import type { ProjectionEvent } from './types'
import { isPhoneLikeCod } from './utils/isPhoneLikeCod'

export type DeviceControllerDeps = {
  deviceRegistry: DeviceRegistry
  sessions: () => SessionManager
  bluez: BluezDeviceClient
  getBtName: (macUpper: string) => string | undefined
  getConnectedBtMac: () => string
  emit: (payload: ProjectionEvent) => void
  autoConnect: () => boolean
  pushReconnectTargets: (targets: Array<[string, string | null]>) => void
  pushWiredPhones: (ids: string[]) => void
}

// The phone's iAP service UUID, used as the CarPlay reconnect ConnectProfile target.
const IAP_PROFILE_UUID = '00000000-deca-fade-deca-deafdecacafe'
const HFP_AG_UUID = '0000111f-0000-1000-8000-00805f9b34fb'

/** The profile a phone of this protocol is woken on. */
function wakeUuid(protocol: string | undefined): string | null {
  if (protocol === 'carplay') return IAP_PROFILE_UUID
  if (protocol === 'androidauto') return HFP_AG_UUID
  return null
}

// Builds the unified device-picker view (native registry + dongle list) and
// serves the picker commands: select routes to a session, forget unpairs BT.
export class DeviceController {
  private lastDeviceViewsSig = ''
  private lastReconnectSig = ''
  private lastWiredPhonesSig = ''

  constructor(private readonly deps: DeviceControllerDeps) {}

  getDevices(): DeviceView[] {
    return this.buildDeviceViews()
  }

  forgetDevice(id: string): { ok: boolean } {
    const e = this.deps.deviceRegistry.forget(id)
    if (!e) return { ok: false }

    // A forgotten device with a running session gets the goodbye
    const s = this.deps.sessions().byDevice({
      btMac: e.btMac,
      wifiMac: e.wifiMac,
      usbUdid: e.usbUdid,
      instanceId: e.instanceId,
      ip: e.currentIp
    })
    if (s) {
      console.log(`[DeviceController] forget ${id} ends session #${s.index}`)
      void (s.driver.disconnectPhone?.() ?? s.driver.send(new SendDisconnectPhone())).catch((err) =>
        console.warn(`[DeviceController] forget ${id} goodbye failed: ${(err as Error).message}`)
      )
      // The goodbye is best-effort
      const index = s.index
      setTimeout(() => this.deps.sessions().close(index), 1500)
    }

    const mac = e.btMac
    if (mac) {
      void this.deps.bluez
        .disconnect(mac)
        .catch(() => {})
        .then(() => this.deps.bluez.remove(mac))
        .catch((err) =>
          console.warn(`[DeviceController] forget ${mac} unpair failed: ${(err as Error).message}`)
        )
    }
    return { ok: true }
  }

  selectDevice(id: string): { ok: boolean } {
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
    if (!s) return this.wakeDevice(e?.btMac ?? (id.includes(':') ? id : undefined), e?.protocol)
    this.deps.sessions().activate(s.index)
    return { ok: true }
  }

  /** Wake a known phone that has no session yet. This goes past the reconnect worker
   *  and its target list, so picking a device by hand works with autoconnect off. */
  private wakeDevice(btMac: string | undefined, protocol: string | undefined): { ok: boolean } {
    if (!btMac) return { ok: false }
    const uuid = wakeUuid(protocol)
    console.log(`[DeviceController] wake ${btMac} (${protocol ?? 'unknown protocol'})`)
    void this.deps.bluez
      .connect(btMac, undefined, uuid ?? undefined)
      .catch((err) =>
        console.warn(`[DeviceController] wake ${btMac} failed: ${(err as Error).message}`)
      )
    return { ok: true }
  }

  emitDevices(): void {
    this.reconcileReconnectTargets()
    this.reconcileWiredPhones()
    const views = this.buildDeviceViews()
    const sig = JSON.stringify(views)
    if (sig === this.lastDeviceViewsSig) return
    this.lastDeviceViewsSig = sig
    this.deps.emit({ type: 'devices', payload: views })
  }

  resendReconnectTargets(): void {
    this.reconcileReconnectTargets(true)
    this.reconcileWiredPhones(true)
  }

  /** Tell the helper which phones already project over USB, so it never offers them the
   *  AP: a wired session outranks a wireless one, the other direction stays open. */
  private reconcileWiredPhones(force = false): void {
    const ids = new Set<string>()
    for (const s of this.deps.sessions().all()) {
      if (s.protocol !== 'androidauto' || s.transport !== 'usb') continue
      if (s.device.instanceId) ids.add(s.device.instanceId.toUpperCase())
      if (s.device.btMac) ids.add(s.device.btMac.toUpperCase())
      if (s.device.usbSerial) ids.add(s.device.usbSerial.toUpperCase())
    }
    const list = [...ids].sort()
    const sig = list.join(',')
    if (!force && sig === this.lastWiredPhonesSig) return
    this.lastWiredPhonesSig = sig
    this.deps.pushWiredPhones(list)
  }

  // Ordered by recency: the helper pages one phone at a time, top of the list first.
  private reconcileReconnectTargets(force = false): void {
    const reg = this.deps.deviceRegistry
    const candidates = this.deps.autoConnect() ? reg.list() : []
    const targets: Array<[string, string | null]> = []
    for (const e of [...candidates].sort((a, b) => (b.lastSeen ?? 0) - (a.lastSeen ?? 0))) {
      if (!e.btMac || !(e.protocol || e.name)) continue
      const sess = this.deps.sessions().byDevice({
        btMac: e.btMac,
        wifiMac: e.wifiMac,
        usbUdid: e.usbUdid,
        instanceId: e.instanceId,
        ip: e.currentIp
      })
      if (sess) continue
      targets.push([e.btMac.toUpperCase(), wakeUuid(e.protocol)])
    }
    const sig = targets.map(([m, u]) => `${m}=${u ?? ''}`).join(',')
    if (!force && sig === this.lastReconnectSig) return
    this.lastReconnectSig = sig
    this.deps.pushReconnectTargets(targets)
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
        const aaBt = this.deps.getConnectedBtMac()
        nameBt = aaBt && !cpBtMacs.has(aaBt.toUpperCase()) ? aaBt : undefined
      }
      const view: DeviceView = {
        id,
        name: (nameBt ? this.deps.getBtName(nameBt.toUpperCase()) : undefined) || e.name,
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
      // every view in `out` was registered in lastSeenOf right after construction
      return (lastSeenOf.get(b) as number) - (lastSeenOf.get(a) as number)
    })
    return out
  }
}
