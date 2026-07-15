/**
 * CpManager — shared Apple CarPlay infrastructure (singleton).
 *
 * Owns the single :7000 RTSP control listener, the shared MFi signer + BlueZ
 * control socket (CpHelperSock), and the one helper event subscription. Every
 * accepted control connection spawns ONE CpSession handed off via onSpawn. Holds
 * the codec / night-mode / cluster seed applied to each new CpSession, drives the
 * cross-connection transport handover (supersede), and routes the helper's
 * per-phone iAP2 metadata to the CpSession it belongs to.
 */

import * as net from 'node:net'
import type { Config } from '@shared/types'
import { CpHelperSock } from './CpHelperSock'
import { CpSession, type CpSessionSeed } from './CpSession'

const CP_CONTROL_PORT = 7000

/** A registry-level identity seen on a helper wifi/device event, awaiting its session. */
interface PendingDevice {
  btMac?: string
  wifiMac?: string
  ip?: string
  usbUdid?: string
  name?: string
}

export interface CpManagerOptions {
  getConfig: () => Config
  onSpawn: (session: CpSession) => void
  /** Registry-level (session-independent) helper presence: hostapd wifi + Bonjour/carkit device. */
  onHelperPresence: (presence: Record<string, unknown>) => void
}

function str(v: unknown): string {
  return typeof v === 'string' ? v : ''
}

export class CpManager {
  private _server: net.Server | null = null
  private readonly _helper = new CpHelperSock()
  private _eventSub: { close: () => void } | null = null
  private readonly _sessions = new Set<CpSession>()
  /** The CarPlay session that owns the helper's single iAP2 metadata feed. */
  private _liveSession: CpSession | null = null
  /** Recent identity-bearing helper events buffered until their session connects. */
  private readonly _pendingDevices: PendingDevice[] = []

  private _hevcSupported = false
  private _vp9Supported = false
  private _av1Supported = false
  private _initialNightMode: boolean | undefined = undefined
  private _clusterStreamActive = true

  private readonly _getConfig: () => Config
  private readonly _onSpawn: (session: CpSession) => void
  private readonly _onHelperPresence: (presence: Record<string, unknown>) => void

  constructor(opts: CpManagerOptions) {
    this._getConfig = opts.getConfig
    this._onSpawn = opts.onSpawn
    this._onHelperPresence = opts.onHelperPresence
  }

  /** The shared MFi signer + BlueZ control socket. */
  get helper(): CpHelperSock {
    return this._helper
  }

  // ── Codec / night / cluster seed (fans out to every live session) ──────────

  setHevcSupported(supported: boolean): void {
    this._hevcSupported = supported
    for (const s of this._sessions) s.setHevcSupported(supported)
  }

  setVp9Supported(supported: boolean): void {
    this._vp9Supported = supported
    for (const s of this._sessions) s.setVp9Supported(supported)
  }

  setAv1Supported(supported: boolean): void {
    this._av1Supported = supported
    for (const s of this._sessions) s.setAv1Supported(supported)
  }

  setInitialNightMode(value: boolean | undefined): void {
    this._initialNightMode = value
    for (const s of this._sessions) s.setInitialNightMode(value)
  }

  setClusterStreamActive(active: boolean): void {
    this._clusterStreamActive = active
    for (const s of this._sessions) s.setClusterStreamActive(active)
  }

  // ── Telemetry push (manager-level: shared hardware / whole subsystem) ───────

  sendNightMode(night: boolean): void {
    for (const s of this._sessions) s.sendNightMode(night)
  }

  sendLocation(nmea: string): void {
    this._helper.sendLocation(nmea).catch(() => {})
  }

  sendVehicleStatus(status: {
    range?: number
    outsideTemperature?: number
    rangeWarning?: boolean
  }): void {
    this._helper.sendVehicleStatus(status).catch(() => {})
  }

  /** Toggle the wireless AA BT profile in the running helper. */
  setAaWireless(enabled: boolean): void {
    this._helper.setAaWireless(enabled).catch((e: Error) => {
      console.warn(`[CpManager] setAaWireless failed: ${e.message}`)
    })
  }

  /** Toggle the wireless CarPlay iAP2 BT profile in the running helper. */
  setCpWireless(enabled: boolean): void {
    this._helper.setCpWireless(enabled).catch((e: Error) => {
      console.warn(`[CpManager] setCpWireless failed: ${e.message}`)
    })
  }

  private _seed(): CpSessionSeed {
    return {
      hevcSupported: this._hevcSupported,
      initialNightMode: this._initialNightMode,
      clusterStreamActive: this._clusterStreamActive
    }
  }

  // ── :7000 control listener ─────────────────────────────────────────────────

  start(): void {
    if (this._server) return
    const server = net.createServer((sock) => this._spawn(sock))
    server.on('error', (err) => console.warn(`[CpManager] server error: ${err.message}`))
    // CarPlay wireless is IPv6-first (link-local); listen dual-stack so the
    // phone can reach :7000 over IPv6 or IPv4.
    server.listen({ port: CP_CONTROL_PORT, host: '::', ipv6Only: false }, () =>
      console.log(`[CpManager] listening on :${CP_CONTROL_PORT} (dual-stack)`)
    )
    this._server = server
    this._eventSub = this._helper.subscribeEvents((ev) => this._onHelperEvent(ev))
  }

  close(): void {
    this._eventSub?.close()
    this._eventSub = null
    if (this._server) {
      try {
        this._server.close()
      } catch {
        /* already closed */
      }
      this._server = null
    }
    for (const s of [...this._sessions]) void s.close()
    this._sessions.clear()
    this._liveSession = null
    this._pendingDevices.length = 0
  }

  private _spawn(sock: net.Socket): void {
    const peer = `${sock.remoteAddress}:${sock.remotePort}`
    console.log(`[CpManager] control connection from ${peer}`)
    sock.setKeepAlive(true, 3000)
    this._register(
      new CpSession({
        socket: sock,
        getConfig: this._getConfig,
        helper: this._helper,
        seed: this._seed()
      })
    )
  }

  /** Wire a CpSession into the shared infra (identity adoption, supersede, teardown) and
   *  hand it to the driver layer. Shared by the AirPlay-socket spawn and the metadata-only
   *  session born at iAP2 identification. */
  private _register(session: CpSession): void {
    this._sessions.add(session)
    session.on('connected', () => {
      this._liveSession = session
    })
    session.on('device-presence', (p: Record<string, unknown>) => {
      // A session reaching RECORD (kind 'active') supersedes an earlier connection
      // of the SAME phone (BT → Wi-Fi handover), keyed by pair-verify controllerId.
      if (p?.kind === 'active') this._supersede(session)
    })
    session.on('identity', () => this._adoptPending(session))
    session.once('disconnected', () => {
      this._sessions.delete(session)
      if (this._liveSession === session) {
        this._liveSession = [...this._sessions].at(-1) ?? null
      }
    })
    this._onSpawn(session)
  }

  /** Born at iAP2 identification: a socket-less CpSession so the phone's metadata has a
   *  session target from event #1. The AirPlay transport adopts it at pair-verify. */
  private _createMetaSession(phoneId: string): CpSession {
    const session = new CpSession({
      getConfig: this._getConfig,
      helper: this._helper,
      seed: this._seed()
    })
    this._register(session)
    session.adoptHelperDevice({ btMac: phoneId })
    // Drain a carkit usbUdid buffered before this session existed, so a later unplug can target it.
    this._adoptPending(session)
    return session
  }

  private _supersede(keep: CpSession): void {
    const id = keep.getControllerId()
    if (!id) return
    for (const other of [...this._sessions]) {
      if (other === keep) continue
      if (other.getControllerId() === id) {
        console.log('[CpManager] transport handover: dropping the superseded connection')
        void other.close()
      }
    }
  }

  // ── Helper event routing ────────────────────────────────────────────────────
  //
  //   - wifi / device  → registry-level presence (onHelperPresence) + adopt onto a
  //                       matching session so it gains the phone's Wi-Fi IP.
  //   - metadata (nowplaying / navigation / call / power / cellular / albumart)
  //                       carries the phone's transport-independent iAP2 identity
  //                       (phoneId, from DeviceTransportIdentifierNotification) plus its
  //                       controllerId, and routes to the session matching either.
  //                       The phoneId is the same over BT, the Wi-Fi tunnel and the
  //                       USB tunnel, so it works even when metadata rides the BT
  //                       bootstrap peer. An untagged event falls back to the live
  //                       session, else the sole session.

  private _onHelperEvent(ev: Record<string, unknown>): void {
    if (ev.type === 'wifi') {
      this._onHelperPresence({
        kind: 'wifi',
        wifiMac: str(ev.mac),
        ip: str(ev.ip),
        connected: ev.event === 'joined'
      })
      return
    }
    if (ev.type === 'device') {
      const ids: PendingDevice = {
        btMac: str(ev.btMac) || undefined,
        ip: str(ev.ip) || undefined,
        usbUdid: str(ev.usbUdid) || undefined,
        name: str(ev.name) || undefined
      }
      this._onHelperPresence({ kind: 'device', ...ids })
      const match = this._matchSession(ids)
      if (match) match.adoptHelperDevice(ids)
      else this._bufferPending(ids)
      return
    }
    if (ev.type === 'device-gone') {
      const usbUdid = str(ev.usbUdid)
      if (!usbUdid) return
      // A wired phone physically left the bus (carkit): close its session, not the live one.
      this._onHelperPresence({ kind: 'device-gone', usbUdid })
      for (const s of [...this._sessions]) {
        if (s.matchesIdentity({ usbUdid })) void s.close()
      }
      for (let i = this._pendingDevices.length - 1; i >= 0; i--) {
        if (this._pendingDevices[i]!.usbUdid === usbUdid) this._pendingDevices.splice(i, 1)
      }
      return
    }
    const phoneId = typeof ev.phoneId === 'string' ? ev.phoneId : ''
    const cid = typeof ev.cid === 'string' ? ev.cid : ''
    const byPhoneId = phoneId
      ? [...this._sessions].find((s) => s.matchesIdentity({ btMac: phoneId }))
      : undefined
    const byCid = cid
      ? [...this._sessions].find((s) => s.matchesIdentity({ controllerId: cid }))
      : undefined
    let target = byPhoneId ?? byCid
    if (!target) {
      // A phoneId-tagged event whose phone has no session yet BIRTHS one, so its metadata has
      // a target from event #1; the AirPlay transport adopts that session at pair-verify
      // (ProjectionService reassigns the driver, drops the placeholder). An untagged event
      // falls back to the live/sole session, unless a phoneId contradicts it.
      const fallback = this._metadataTarget()
      const contradicts =
        Boolean(phoneId) &&
        Boolean(fallback?.getBtMac()) &&
        fallback!.getBtMac().toLowerCase() !== phoneId.toLowerCase()
      if (fallback && !contradicts) target = fallback
      else if (phoneId) target = this._createMetaSession(phoneId)
    }
    if (target) target.ingestHelperEvent(ev)
  }

  private _matchSession(ids: PendingDevice): CpSession | null {
    for (const s of this._sessions) if (s.matchesIdentity(ids)) return s
    return null
  }

  /** The session an untagged (cid="") event belongs to: the current live session,
   *  else the sole session. Untagged metadata rides a BT bootstrap peer that lingers
   *  when disableBluetooth cannot drop it, so dropping here would lose CP media. */
  private _metadataTarget(): CpSession | null {
    if (this._liveSession && this._sessions.has(this._liveSession)) return this._liveSession
    if (this._sessions.size === 1) return [...this._sessions][0] ?? null
    return null
  }

  private _bufferPending(ids: PendingDevice): void {
    if (!ids.btMac && !ids.wifiMac && !ids.usbUdid) return
    const i = this._pendingDevices.findIndex(
      (d) => (ids.btMac && d.btMac === ids.btMac) || (ids.usbUdid && d.usbUdid === ids.usbUdid)
    )
    if (i >= 0) this._pendingDevices[i] = { ...this._pendingDevices[i], ...ids }
    else this._pendingDevices.push(ids)
    while (this._pendingDevices.length > 8) this._pendingDevices.shift()
  }

  private _adoptPending(session: CpSession): void {
    for (let i = this._pendingDevices.length - 1; i >= 0; i--) {
      const ids = this._pendingDevices[i]!
      if (session.matchesIdentity(ids)) {
        this._pendingDevices.splice(i, 1)
        session.adoptHelperDevice(ids)
      }
    }
  }
}

export default CpManager
