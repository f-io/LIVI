import type { DeviceView, NaviBag } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import type { AudioCommand } from '@shared/types/ProjectionEnums'
import type { NavLocale } from '@shared/utils'
import type { DongleFwResponse } from '../ipc/types'
import type { Command, NavigationData, PhoneType } from '../messages'
import { MediaType, NavigationMetaType } from '../messages'
import type { TransportSnapshot } from '../transport/types'
import type { SessionProtocol, VideoCodec } from './SessionManager'

export type PendingStartupConnectTarget = {
  btMac: string
  phoneWorkMode: PhoneWorkMode
}

export type MediaBag = Record<string, unknown>

export interface PersistedMediaPayload {
  type: MediaType
  media?: MediaBag
  base64Image?: string
  error?: boolean
}

export type PersistedMediaFile = {
  timestamp: string
  payload: PersistedMediaPayload
}

export interface PersistedNavigationPayload {
  metaType: NavigationMetaType | number
  navi: NaviBag | null
  rawUtf8?: string
  error?: boolean
  display?: {
    locale: NavLocale
    appName?: string
    destinationName?: string
    roadName?: string
    maneuverText?: string
    timeToDestinationText?: string
    distanceToDestinationText?: string
    remainDistanceText?: string
  }
}

export type AudioInfo = {
  codec: string | null
  sampleRate: number | null
  channels: number | null
  bitDepth: number | null
}

export type PersistedNavigationFile = {
  timestamp: string
  payload: PersistedNavigationPayload
}

export type ProjectionEventAudioInfo = {
  codec: string | number
  sampleRate: number
  channels: number
  bitDepth: number
}

export type ProjectionEvent =
  | { type: 'dongleInfo'; payload: { dongleFwVersion: string | undefined; boxInfo: unknown } }
  | { type: 'gnss'; payload: { text: string } }
  | {
      type: 'fwUpdate'
      stage:
        | 'check:start'
        | 'check:done'
        | 'download:start'
        | 'download:progress'
        | 'download:done'
        | 'download:error'
        | 'upload:start'
        | 'upload:progress'
        | 'upload:state'
        | 'upload:file-sent'
        | 'upload:done'
        | 'upload:error'
      message?: string
      status?: number
      statusText?: string
      isOta?: boolean
      isTerminal?: boolean
      ok?: boolean
      progress?: number
      result?: DongleFwResponse
      path?: string | null
      bytes?: number
      received?: number
      total?: number
      percent?: number
    }
  | { type: 'plugged'; phoneType: PhoneType }
  | { type: 'unplugged' }
  | { type: 'resolution'; payload: { width: number; height: number } }
  | {
      type: 'audio'
      payload: { command: AudioCommand; audioType: number; decodeType: number; volume: number }
    }
  | { type: 'audioInfo'; payload: ProjectionEventAudioInfo }
  | { type: 'command'; message: Command }
  | { type: 'projection'; shown: boolean }
  | { type: 'audioDevicesChanged' }
  | { type: 'transportState'; payload: TransportSnapshot }
  | { type: 'bluetoothPairedList'; payload: string }
  | { type: 'session'; protocol: SessionProtocol | null; position: number; total: number }
  | { type: 'devices'; payload: DeviceView[] }
  | { type: 'media'; payload: { payload: PersistedMediaPayload } }
  | { type: 'media-reset'; reason: string }
  | { type: 'navigation'; payload: NavigationData }
  | { type: 'navigation-reset'; reason: string }
  | {
      type: 'attention'
      payload: {
        kind: 'call' | 'voiceAssistant' | 'nav'
        active: boolean
        phase?: 'incoming' | 'ended'
      }
    }
  | { type: 'failure' }
  | { type: 'video-codec'; payload: { codec: VideoCodec } }
  | { type: 'cluster-video-codec'; payload: { codec: VideoCodec } }
