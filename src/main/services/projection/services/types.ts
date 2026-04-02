import type { NaviBag } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import type { NavLocale } from '@shared/utils'
import { MediaType, NavigationMetaType } from '../messages'

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
