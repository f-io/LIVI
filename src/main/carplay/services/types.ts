import { MediaType, NavigationMetaType } from '../messages'
import type { NavLocale } from './utils/translateNavigation'

export type MediaBag = Record<string, unknown>
export type NaviBag = Record<string, unknown>

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

export type PersistedNavigationFile = {
  timestamp: string
  payload: PersistedNavigationPayload
}
