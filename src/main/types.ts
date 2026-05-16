import { NULL_DELETES } from '@main/constants'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { TelemetrySocket } from '@main/services/Socket'
import { USBService } from '@main/services/usb/USBService'
import type { Config } from '@shared/types'

export type UpdateSessionState = 'idle' | 'downloading' | 'ready' | 'installing'

export type UpdateEventPayload =
  | { phase: 'start' }
  | { phase: 'download'; received: number; total: number; percent: number }
  | { phase: 'ready' }
  | { phase: 'mounting' | 'copying' | 'unmounting' | 'installing' | 'relaunching' }
  | { phase: 'error'; message: string }

// GitHub API
export interface GhAsset {
  name?: string
  browser_download_url?: string
}
export interface GhRelease {
  tag_name?: string
  name?: string
  assets?: GhAsset[]
}

export interface ServicesProps {
  projectionService: ProjectionService
  usbService: USBService
  telemetrySocket: TelemetrySocket
}

export interface runtimeStateProps {
  config: Config
  telemetrySocket: TelemetrySocket | null
  isQuitting: boolean
  suppressNextFsSync: boolean
  wmExitedKiosk: boolean
}

export type NullDeleteKey = (typeof NULL_DELETES)[number]

export interface Stream {
  speed: number
  rpm: number
  temperature: number
}

export interface ServerToClientEvents {
  settings: (config: Config) => void
  reverse: (reverse: boolean) => void
  lights: (lights: boolean) => void
}

export interface ClientToServerEvents {
  connection: () => void
  getSettings: () => void
  saveSettings: (settings: Config) => void
  stream: (stream: Stream) => void
}
