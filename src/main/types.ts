import { NULL_DELETES } from '@main/constants'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { USBService } from '@main/services/usb/USBService'
import type { Config } from '@shared/types'

/** Either side of the Telemetry (IPC) transport — hosting a socket, or dialing out to
 *  one — implements this, so callers can hold/swap either without caring which. */
export interface TelemetryTransport {
  connect(): Promise<void>
  disconnect(): Promise<void>
}

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
  target_commitish?: string
  assets?: GhAsset[]
}

export interface ServicesProps {
  projectionService: ProjectionService
  usbService: USBService
  telemetrySocket: TelemetryTransport
}

export interface runtimeStateProps {
  config: Config
  telemetrySocket: TelemetryTransport | null
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
