import { DongleUpload } from '@main/services/link/dongleUpload'
import type { Config } from '@shared/types'
import type { AaMediaSinkDeps } from '../driver/aa/AaEventBridge'
import { AaManager, type HelperSessionSource } from '../driver/aa/AaManager'
import type { AaSession } from '../driver/aa/AaSession'
import { CpManager } from '../driver/cp/CpManager'
import type { CpSession } from '../driver/cp/CpSession'
import type { IPhoneDriver } from '../driver/IPhoneDriver'
import { DuckAudio, MediaData, type Message, NavigationData } from '../messages'

export type DriverEventHandlers = {
  onMessage: (...args: unknown[]) => void
  onMetaMessage: (driver: IPhoneDriver, msg: Message) => void
  onFailure: (...args: unknown[]) => void
  onTargetedConnect: (...args: unknown[]) => void
  onVideoCodec: (codec: 'h264' | 'h265' | 'vp9' | 'av1') => void
  onClusterVideoCodec: (codec: 'h264' | 'h265' | 'vp9' | 'av1') => void
  onVideoConfig: (codecData: Buffer) => void
  onClusterVideoConfig: (codecData: Buffer) => void
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
  onCpHelperConnect?: () => void
  onCpCreated?: (session: IPhoneDriver) => void
  onCpReleased?: (session: IPhoneDriver) => void
  getCpConfigSeed: () => AaConfigSeed
  getConfig: () => Config
  mediaSink?: AaMediaSinkDeps
}

export class ProjectionDriverManager {
  readonly dongleUpload = new DongleUpload()
  private aaManager: AaManager | null = null
  private cpManager: CpManager | null = null
  private routed: IPhoneDriver | null = null
  private readonly metaListeners = new Map<IPhoneDriver, (msg: Message) => void>()

  constructor(private readonly deps: DriverManagerDeps) {}

  getActive(): IPhoneDriver | null {
    return this.routed
  }

  getAaManager(): AaManager | null {
    return this.aaManager
  }

  getCpManager(): CpManager | null {
    return this.cpManager
  }

  getDongleUpload(): DongleUpload {
    return this.dongleUpload
  }

  route(target: IPhoneDriver | null): void {
    if (this.routed === target) return
    if (this.routed) this.detachListeners(this.routed)
    if (target) this.attachListeners(target)
    this.routed = target
  }

  // ── Android Auto ────────────────────────────────────────────────────────────

  ensureAaManager(): AaManager {
    if (this.aaManager) return this.aaManager
    const mgr = new AaManager({
      getConfig: this.deps.getConfig,
      onSpawn: (session) => this.onAaSpawn(session),
      mediaSink: this.deps.mediaSink
    })
    this.aaManager = mgr

    const seed = this.deps.getAaConfigSeed()
    mgr.setHevcSupported(seed.hevcSupported)
    mgr.setVp9Supported(seed.vp9Supported)
    mgr.setAv1Supported(seed.av1Supported)
    mgr.setInitialNightMode(seed.initialNightMode)
    return mgr
  }

  attachHelper(helper: HelperSessionSource | undefined): void {
    this.ensureAaManager().attachHelper(helper)
    this.dongleUpload.attachHelper(helper)
  }

  detachHelper(): void {
    this.aaManager?.detachHelper()
    this.dongleUpload.detachHelper()
  }

  stopAaWireless(): void {
    this.aaManager?.stopWireless()
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
    session.on('connected', () => {
      this.attachMetaListener(session)
      this.deps.onAaConnected(session)
    })
    session.on('device-presence', (p: Record<string, unknown>) =>
      this.deps.onAaPresence?.(session, p)
    )
    session.on('disconnected', () => {
      this.deps.onAaDisconnected(session)
      this.detachMetaListener(session)
      if (this.routed === session) this.route(null)
      this.deps.onAaReleased?.(session)
    })
  }

  // ── CarPlay ──────────────────────────────────────────────────────────────────

  ensureCpManager(): CpManager {
    if (this.cpManager) return this.cpManager
    const mgr = new CpManager({
      getConfig: this.deps.getConfig,
      onSpawn: (session) => this.onCpSpawn(session),
      onHelperPresence: (p) => this.deps.onCpHelperPresence?.(p),
      onHelperConnect: () => this.deps.onCpHelperConnect?.()
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

  async releaseCp(): Promise<void> {
    if (!this.cpManager) return
    const mgr = this.cpManager
    this.cpManager = null
    try {
      await mgr.close()
    } catch (e) {
      console.warn('[ProjectionDriverManager] cpManager.close threw on release', e)
    }
  }

  async releaseAa(): Promise<void> {
    if (!this.aaManager) return
    const mgr = this.aaManager
    this.aaManager = null
    try {
      await mgr.close()
    } catch (e) {
      console.warn('[ProjectionDriverManager] aaManager.close threw on release', e)
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
      if (this.routed === session) this.route(null)
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
    d.on('video-config', handlers.onVideoConfig)
    d.on('cluster-video-config', handlers.onClusterVideoConfig)
  }

  private detachListeners(d: IPhoneDriver): void {
    const { handlers } = this.deps
    d.off('message', handlers.onMessage)
    d.off('failure', handlers.onFailure)
    d.off('targeted-connect-dispatched', handlers.onTargetedConnect)
    d.off('video-codec', handlers.onVideoCodec)
    d.off('cluster-video-codec', handlers.onClusterVideoCodec)
    d.off('video-config', handlers.onVideoConfig)
    d.off('cluster-video-config', handlers.onClusterVideoConfig)
  }

  private attachMetaListener(d: IPhoneDriver): void {
    if (this.metaListeners.has(d)) return
    const fn = (msg: Message): void => {
      if (msg instanceof MediaData || msg instanceof NavigationData || msg instanceof DuckAudio) {
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
