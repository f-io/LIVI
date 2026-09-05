import type { Config } from '@shared/types'
import type { WebContents } from 'electron'
import type { SendableMessage } from '../messages/sendable'
import type { DeviceView } from '../services/DeviceRegistry'
import type { LogicalStreamKey } from '../services/ProjectionAudio'
import type {
  PersistedMediaFile,
  PersistedNavigationFile,
  ProjectionEvent
} from '../services/types'
import type { Transport, TransportSnapshot } from '../transport/types'

export type BtActionResponse = { ok: boolean; error?: string }

export interface ProjectionIpcHost {
  // Lifecycle / transport
  start(): Promise<void>
  stop(): Promise<void>
  restartSession(): Promise<void>
  setVideoVisible(visible: boolean): void
  pickPreferredTransport(): Transport | null
  switchTransport(): Promise<{ ok: boolean; active: Transport | null }>
  getTransportState(): TransportSnapshot
  getDevices(): DeviceView[]
  selectDevice(id: string): { ok: boolean }
  cycleSession(): void
  forgetDevice(id: string): { ok: boolean }
  applyCodecCapabilities(caps: unknown): void

  // Driver send
  send(msg: SendableMessage): Promise<boolean>
  isUsingAa(): boolean
  isStarted(): boolean

  // Bluetooth
  connectBt(mac: string): Promise<BtActionResponse>
  refreshBtPaired(): void

  // Cluster
  getConfig(): Config
  setClusterRequested(id: number, wanted: boolean): void
  isMainClusterWindow(id: number): boolean
  isClusterRequested(): boolean
  setClusterVisible(v: boolean): void
  resetLastClusterVideoSize(): void
  getLastClusterVideoSize(): { width: number; height: number } | null
  getClusterTargetWebContents(): WebContents[]

  reloadConfigFromDisk(): Promise<void>
  emitProjectionEvent(payload: ProjectionEvent): void
  readActiveMedia(): PersistedMediaFile
  readActiveNav(): PersistedNavigationFile

  // Audio
  setAudioStreamVolume(stream: LogicalStreamKey, volume: number): void
  setAudioVisualizerEnabled(enabled: boolean, sourceId?: number): void
}
