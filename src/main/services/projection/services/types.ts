import type { DeviceView, NaviBag } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import type { AudioCommand } from '@shared/types/ProjectionEnums'
import type { NavLocale } from '@shared/utils'
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
    afterRoadName?: string
    maneuverText?: string
    timeToDestinationText?: string
    distanceToDestinationText?: string
    remainDistanceText?: string
  }
}

export type PersistedNavigationFile = {
  timestamp: string
  payload: PersistedNavigationPayload
}

export type ProjectionEventAudioInfo = {
  sampleRate: number
}

export type ProjectionEvent =
  | { type: 'plugged'; phoneType: PhoneType }
  | { type: 'unplugged' }
  | { type: 'resolution'; payload: { width: number; height: number } }
  | {
      type: 'audio'
      payload: { command: AudioCommand; audioType: number; decodeType: number }
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
