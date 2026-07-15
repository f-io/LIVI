/**
 * AaManager — shared Android Auto infrastructure (singleton).
 *
 * Owns the :5277 wireless TCP listener and the wired AOAP bring-up (one
 * UsbAoapBridge per device). Every accepted wireless socket, and every wired
 * loopback socket after the AOAP handshake, spawns ONE AaSession handed off via
 * onSpawn. Holds the codec-capability seed applied to each new AaSession.
 */

import * as net from 'node:net'
import type { Config } from '@shared/types'
import { AaSession, type AaSessionSeed } from './AaSession'
import { AOAP_LOOPBACK_PORT } from './stack/aoap/constants'
import { TCP_PORT } from './stack/index'
import { UsbAoapBridge } from './stack/transport/UsbAoapBridge'

type Device = USBDevice

export interface AaManagerOptions {
  getConfig: () => Config
  onWillReenumerate?: (durationMs: number) => void
  onSpawn: (session: AaSession) => void
}

function deviceKey(device: Device): string {
  const vid = device.vendorId ?? 0
  const pid = device.productId ?? 0
  const serial = device.serialNumber ?? ''
  return `${vid}:${pid}:${serial}`
}

export class AaManager {
  private _server: net.Server | null = null
  private readonly _wiredBridges = new Map<string, UsbAoapBridge>()
  private readonly _sessions = new Set<AaSession>()

  private _hevcSupported = false
  private _vp9Supported = false
  private _av1Supported = false
  private _initialNightMode: boolean | undefined = undefined
  private _clusterStreamActive = true

  private readonly _getConfig: () => Config
  private readonly _onWillReenumerate: ((durationMs: number) => void) | undefined
  private readonly _onSpawn: (session: AaSession) => void

  constructor(opts: AaManagerOptions) {
    this._getConfig = opts.getConfig
    this._onWillReenumerate = opts.onWillReenumerate
    this._onSpawn = opts.onSpawn
  }

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

  private _seed(): AaSessionSeed {
    return {
      hevcSupported: this._hevcSupported,
      vp9Supported: this._vp9Supported,
      av1Supported: this._av1Supported,
      initialNightMode: this._initialNightMode,
      clusterStreamActive: this._clusterStreamActive
    }
  }

  // ── Wireless :5277 listener ────────────────────────────────────────────────

  startWireless(): void {
    if (this._server) return
    const server = net.createServer({ allowHalfOpen: true }, (sock) => {
      const remote = `${sock.remoteAddress}:${sock.remotePort}`
      console.log(`[AaManager] wireless connection from ${remote}`)
      sock.setNoDelay(true)
      sock.setTimeout(30_000)
      this._spawn(sock, false, null)
    })
    server.on('error', (err) => console.warn(`[AaManager] wireless server error: ${err.message}`))
    this._server = server
    server.listen(TCP_PORT, '0.0.0.0', () => {
      console.log(`[AaManager] wireless AA listening on TCP ${TCP_PORT}`)
    })
  }

  stopWireless(): void {
    if (this._server) {
      try {
        this._server.close()
      } catch {
        /* already closed */
      }
      this._server = null
    }
    for (const s of [...this._sessions]) {
      if (!s.isWiredMode()) void s.close()
    }
  }

  // ── Wired AOAP bring-up ────────────────────────────────────────────────────

  async bringUpWired(device: Device): Promise<boolean> {
    const key = deviceKey(device)
    if (this._wiredBridges.has(key)) {
      console.log('[AaManager] bringUpWired: bridge already in-flight/up for device, skipping')
      return true
    }
    console.log('[AaManager] _startWiredBridge: bringing up wired AOAP bridge')

    const bridge = new UsbAoapBridge(device, this._onWillReenumerate)
    this._wiredBridges.set(key, bridge)

    bridge.on('error', (err: Error) => {
      console.warn(`[AaManager] wired bridge error: ${err.message}`)
    })
    bridge.on('closed', () => {
      console.log('[AaManager] wired bridge closed')
    })
    bridge.once('ready', ({ host, port }: { host: string; port: number }) => {
      if (this._wiredBridges.get(key) !== bridge) return
      console.log(`[AaManager] wired bridge ready on ${host}:${port}, dialling loopback`)
      const sock = net.createConnection({ host, port, allowHalfOpen: true })
      sock.once('connect', () => {
        if (this._wiredBridges.get(key) !== bridge) {
          try {
            sock.destroy()
          } catch {
            /* ignore */
          }
          return
        }
        console.log('[AaManager] wired loopback connected → spawning AaSession')
        this._spawn(sock, true, bridge, key)
      })
      sock.on('error', (err: Error) => {
        console.warn(`[AaManager] wired loopback socket error: ${err.message}`)
      })
    })

    try {
      await bridge.start(AOAP_LOOPBACK_PORT)
      console.log('[AaManager] wired AA bridge started on loopback')
      return true
    } catch (err) {
      console.error(`[AaManager] wired bridge start failed: ${(err as Error).message}`)
      try {
        await bridge.stop()
      } catch {
        /* ignore */
      }
      this._wiredBridges.delete(key)
      return false
    }
  }

  private _spawn(
    socket: net.Socket,
    wired: boolean,
    wiredBridge: UsbAoapBridge | null,
    key?: string
  ): void {
    const session = new AaSession({
      socket,
      getConfig: this._getConfig,
      wired,
      wiredBridge,
      seed: this._seed()
    })
    this._sessions.add(session)
    session.once('disconnected', () => {
      this._sessions.delete(session)
      if (wired && key && this._wiredBridges.get(key) === wiredBridge) {
        this._wiredBridges.delete(key)
      }
    })
    this._onSpawn(session)
  }
}

export default AaManager
