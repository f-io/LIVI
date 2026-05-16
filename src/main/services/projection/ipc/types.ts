import type { Config, DongleFirmwareAction, DongleFwApiRaw } from '@shared/types'
import type { WebContents } from 'electron'
import type { SendableMessage } from '../messages/sendable'
import type { FirmwareUpdateService } from '../services/FirmwareUpdateService'
import type { LogicalStreamKey } from '../services/ProjectionAudio'
import type { PendingStartupConnectTarget } from '../services/types'
import type { Transport, TransportSnapshot } from '../transport/types'

export type AaBtSockResponse = { ok: boolean; error?: string }

export type DongleFwResponse = {
  ok: boolean
  hasUpdate: boolean
  size: string | number
  token?: string
  request?: Record<string, unknown>
  raw: DongleFwApiRaw
  error?: string
}

export type DongleFwRequest = { action: DongleFirmwareAction }

export type DevToolsUploadResult = {
  ok: boolean
  cgiOk: boolean
  webOk: boolean
  urls: string[]
  startedAt: string
  finishedAt: string
  durationMs: number
}

export interface ProjectionIpcHost {
  // Lifecycle / transport
  start(): Promise<void>
  stop(): Promise<void>
  pickPreferredTransport(): Transport | null
  flipTransport(): Promise<{ ok: boolean; active: Transport | null }>
  getTransportState(): TransportSnapshot
  applyCodecCapabilities(caps: unknown): void

  // Driver send
  send(msg: SendableMessage): Promise<boolean>
  isUsingDongle(): boolean
  isUsingAa(): boolean
  isStarted(): boolean
  hasWebUsbDevice(): boolean

  // Bluetooth
  sendBluetoothPairedList(text: string): Promise<boolean>
  connectAaBt(mac: string): Promise<AaBtSockResponse>
  removeAaBt(mac: string): Promise<AaBtSockResponse>
  refreshAaBtPaired(): void
  getBoxInfo(): unknown
  setPendingStartupConnectTarget(t: PendingStartupConnectTarget | null): void

  // Cluster
  getConfig(): Config
  setClusterRequested(v: boolean): void
  resetLastClusterVideoSize(): void
  getLastClusterCodec(): string | null
  getClusterTargetWebContents(): WebContents[]

  // Dongle ops
  uploadIcons(): void
  getDevToolsUrlCandidates(): string[]

  // Firmware
  reloadConfigFromDisk(): Promise<void>
  getFirmware(): FirmwareUpdateService
  getApkVer(): string
  getDongleFwVersion(): string | undefined
  emitProjectionEvent(payload: unknown): void

  // Audio
  setAudioStreamVolume(stream: LogicalStreamKey, volume: number): void
  setAudioVisualizerEnabled(enabled: boolean): void
}
