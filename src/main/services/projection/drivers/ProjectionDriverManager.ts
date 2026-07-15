import type { Config } from '@shared/types'
import { AaManager } from '../driver/aa/AaManager'
import type { AaSession } from '../driver/aa/AaSession'
import { CpManager } from '../driver/cp/CpManager'
import type { CpSession } from '../driver/cp/CpSession'
import type { IPhoneDriver } from '../driver/IPhoneDriver'
import { DongleDriver, MediaData, type Message, NavigationData } from '../messages'
import type { Transport } from '../transport/types'

export type DriverEventHandlers = {
  onMessage: (...args: unknown[]) => void
  onMetaMessage: (driver: IPhoneDriver, msg: Message) => void
  onFailure: (...args: unknown[]) => void
  onTargetedConnect: (...args: unknown[]) => void
  onVideoCodec: (codec: 'h264' | 'h265' | 'vp9' | 'av1') => void
  onClusterVideoCodec: (codec: 'h264' | 'h265' | 'vp9' | 'av1') => void
}

export type AaConfigSeed = {
  hevcSupported: boolean
  vp9Supported: boolean
  av1Supported: boolean
  initialNightMode: boolean | undefined
}

export type DriverManagerDeps = {
  handlers: DriverEventHandlers
  onAaConnected: (session: IPhoneDriver) => void
  onAaDisconnected: (session: IPhoneDriver) => void
  onAaPresence?: (session: IPhoneDriver, presence: Record<string, unknown>) => void
  onAaCreated?: (session: IPhoneDriver) => void
  onAaReleased?: (session: IPhoneDriver) => void
  getAaConfigSeed: () => AaConfigSeed
  onCpConnected: (session: IPhoneDriver) => void
  onCpDisconnected: (session: IPhoneDriver) => void
  onCpPresence?: (session: IPhoneDriver, presence: Record<string, unknown>) => void
  onCpHelperPresence?: (presence: Record<string, unknown>) => void
  onCpCreated?: (session: IPhoneDriver) => void
  onCpReleased?: (session: IPhoneDriver) => void
  getCpConfigSeed: () => AaConfigSeed
  onPhoneReenumerate: (ms: number) => void
  getConfig: () => Config
}

export class ProjectionDriverManager {
  readonly dongle = new DongleDriver()
  private aaManager: AaManager | null = null
  private cpManager: CpManager | null = null
  private routed: IPhoneDriver
  private readonly metaListeners = new Map<IPhoneDriver, (msg: Message) => void>()

  constructor(private readonly deps: DriverManagerDeps) {
    this.routed = this.dongle
    this.attachListeners(this.dongle)
    this.attachMetaListener(this.dongle)
  }

  getActive(): IPhoneDriver {
    return this.routed
  }

  getAaManager(): AaManager | null {
    return this.aaManager
  }

  getCpManager(): CpManager | null {
    return this.cpManager
  }

  getDongle(): DongleDriver {
    return this.dongle
  }

  selectFor(_transport: Transport): IPhoneDriver {
    // CarPlay + AA are session-routed via SessionManager; only the dongle is
    // selected directly (start() short-circuits before selecting a native driver).
    this.route(this.dongle)
    return this.dongle
  }

  route(target: IPhoneDriver): void {
    if (this.routed === target) return
    this.detachListeners(this.routed)
    this.attachListeners(target)
    this.routed = target
  }

  // ── Android Auto ────────────────────────────────────────────────────────────

  ensureAaManager(): AaManager {
    if (this.aaManager) return this.aaManager
    const mgr = new AaManager({
      getConfig: this.deps.getConfig,
      onWillReenumerate: (ms) => this.deps.onPhoneReenumerate(ms),
      onSpawn: (session) => this.onAaSpawn(session)
    })
    this.aaManager = mgr

    const seed = this.deps.getAaConfigSeed()
    mgr.setHevcSupported(seed.hevcSupported)
    mgr.setVp9Supported(seed.vp9Supported)
    mgr.setAv1Supported(seed.av1Supported)
    mgr.setInitialNightMode(seed.initialNightMode)
    return mgr
  }

  startAaWireless(): void {
    this.ensureAaManager().startWireless()
  }

  stopAaWireless(): void {
    this.aaManager?.stopWireless()
  }

  bringUpAaWired(device: USBDevice): Promise<boolean> {
    return this.ensureAaManager().bringUpWired(device)
  }

  setAaHevcSupported(supported: boolean): void {
    this.aaManager?.setHevcSupported(supported)
  }

  setAaVp9Supported(supported: boolean): void {
    this.aaManager?.setVp9Supported(supported)
  }

  setAaAv1Supported(supported: boolean): void {
    this.aaManager?.setAv1Supported(supported)
  }

  setAaInitialNightMode(value: boolean | undefined): void {
    this.aaManager?.setInitialNightMode(value)
  }

  setAaClusterStreamActive(active: boolean): void {
    this.aaManager?.setClusterStreamActive(active)
  }

  private onAaSpawn(session: AaSession): void {
    this.deps.onAaCreated?.(session)
    this.attachMetaListener(session)
    session.on('connected', () => this.deps.onAaConnected(session))
    session.on('device-presence', (p: Record<string, unknown>) =>
      this.deps.onAaPresence?.(session, p)
    )
    session.once('disconnected', () => {
      this.deps.onAaDisconnected(session)
      this.detachMetaListener(session)
      if (this.routed === session) this.route(this.dongle)
      this.deps.onAaReleased?.(session)
    })
  }

  // ── CarPlay ──────────────────────────────────────────────────────────────────

  ensureCpManager(): CpManager {
    if (this.cpManager) return this.cpManager
    const mgr = new CpManager({
      getConfig: this.deps.getConfig,
      onSpawn: (session) => this.onCpSpawn(session),
      onHelperPresence: (p) => this.deps.onCpHelperPresence?.(p)
    })
    this.cpManager = mgr

    const seed = this.deps.getCpConfigSeed()
    mgr.setHevcSupported(seed.hevcSupported)
    mgr.setVp9Supported(seed.vp9Supported)
    mgr.setAv1Supported(seed.av1Supported)
    mgr.setInitialNightMode(seed.initialNightMode)
    return mgr
  }

  startCp(): void {
    this.ensureCpManager().start()
  }

  setCpHevcSupported(supported: boolean): void {
    this.cpManager?.setHevcSupported(supported)
  }

  setCpVp9Supported(supported: boolean): void {
    this.cpManager?.setVp9Supported(supported)
  }

  setCpAv1Supported(supported: boolean): void {
    this.cpManager?.setAv1Supported(supported)
  }

  setCpInitialNightMode(value: boolean | undefined): void {
    this.cpManager?.setInitialNightMode(value)
  }

  setCpClusterStreamActive(active: boolean): void {
    this.cpManager?.setClusterStreamActive(active)
  }

  releaseCp(): void {
    if (!this.cpManager) return
    const mgr = this.cpManager
    this.cpManager = null
    try {
      mgr.close()
    } catch (e) {
      console.warn('[ProjectionDriverManager] cpManager.close threw on release', e)
    }
  }

  private onCpSpawn(session: CpSession): void {
    this.deps.onCpCreated?.(session)
    this.attachMetaListener(session)
    session.on('connected', () => this.deps.onCpConnected(session))
    session.on('device-presence', (p: Record<string, unknown>) =>
      this.deps.onCpPresence?.(session, p)
    )
    session.once('disconnected', () => {
      this.deps.onCpDisconnected(session)
      this.detachMetaListener(session)
      if (this.routed === session) this.route(this.dongle)
      this.deps.onCpReleased?.(session)
    })
  }

  private attachListeners(d: IPhoneDriver): void {
    const { handlers } = this.deps
    d.on('message', handlers.onMessage)
    d.on('failure', handlers.onFailure)
    d.on('targeted-connect-dispatched', handlers.onTargetedConnect)
    d.on('video-codec', handlers.onVideoCodec)
    d.on('cluster-video-codec', handlers.onClusterVideoCodec)
  }

  private detachListeners(d: IPhoneDriver): void {
    const { handlers } = this.deps
    d.off('message', handlers.onMessage)
    d.off('failure', handlers.onFailure)
    d.off('targeted-connect-dispatched', handlers.onTargetedConnect)
    d.off('video-codec', handlers.onVideoCodec)
    d.off('cluster-video-codec', handlers.onClusterVideoCodec)
  }

  private attachMetaListener(d: IPhoneDriver): void {
    if (this.metaListeners.has(d)) return
    const fn = (msg: Message): void => {
      if (msg instanceof MediaData || msg instanceof NavigationData) {
        this.deps.handlers.onMetaMessage(d, msg)
      }
    }
    this.metaListeners.set(d, fn)
    d.on('message', fn)
  }

  private detachMetaListener(d: IPhoneDriver): void {
    const fn = this.metaListeners.get(d)
    if (!fn) return
    d.off('message', fn)
    this.metaListeners.delete(d)
  }
}
