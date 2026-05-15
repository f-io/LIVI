import { AaDriver } from '../driver/aa/aaDriver'
import type { IPhoneDriver } from '../driver/IPhoneDriver'
import { DongleDriver } from '../messages'
import type { Transport } from '../transport/types'

export type DriverEventHandlers = {
  onMessage: (...args: unknown[]) => void
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
  onAaConnected: () => void
  onAaDisconnected: () => void
  onAaCreated?: () => void
  onAaReleased?: () => void
  getAaConfigSeed: () => AaConfigSeed
  onPhoneReenumerate: (ms: number) => void
}

export class ProjectionDriverManager {
  readonly dongle = new DongleDriver()
  private aa: AaDriver | null = null

  constructor(private readonly deps: DriverManagerDeps) {
    this.attachListeners(this.dongle)
  }

  getActive(): IPhoneDriver {
    return this.aa ?? this.dongle
  }

  getAa(): AaDriver | null {
    return this.aa
  }

  getDongle(): DongleDriver {
    return this.dongle
  }

  selectFor(transport: Transport): IPhoneDriver {
    return transport === 'aa' ? this.ensureAa() : this.releaseAaToDongle()
  }

  ensureAa(): AaDriver {
    if (this.aa) return this.aa
    const aa = new AaDriver({
      onWillReenumerate: (ms) => this.deps.onPhoneReenumerate(ms)
    })
    this.aa = aa

    const seed = this.deps.getAaConfigSeed()
    aa.setHevcSupported(seed.hevcSupported)
    aa.setVp9Supported(seed.vp9Supported)
    aa.setAv1Supported(seed.av1Supported)
    aa.setInitialNightMode(seed.initialNightMode)

    this.detachListeners(this.dongle)
    this.attachListeners(aa)
    aa.on('connected', this.deps.onAaConnected)
    aa.on('disconnected', this.deps.onAaDisconnected)

    this.deps.onAaCreated?.()
    return aa
  }

  releaseAa(): void {
    if (!this.aa) return
    const aa = this.aa
    this.deps.onAaReleased?.()

    aa.off('connected', this.deps.onAaConnected)
    aa.off('disconnected', this.deps.onAaDisconnected)
    this.detachListeners(aa)
    try {
      aa.close()
    } catch (e) {
      console.warn('[ProjectionDriverManager] aa.close threw on release', e)
    }
    this.aa = null
    this.attachListeners(this.dongle)
  }

  private releaseAaToDongle(): DongleDriver {
    this.releaseAa()
    return this.dongle
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
}
