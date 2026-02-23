import { ExtraConfig } from '@main/Globals'
import { Socket } from '@main/services/Socket'

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

export interface runtimeStateProps {
  config: ExtraConfig
  socket: Socket | null
  isQuitting: boolean
  suppressNextFsSync: boolean
}

export interface Stream {
  speed: number
  rpm: number
  temperature: number
}

export interface ServerToClientEvents {
  settings: (config: ExtraConfig) => void
  reverse: (reverse: boolean) => void
  lights: (lights: boolean) => void
}

export interface ClientToServerEvents {
  connection: () => void
  getSettings: () => void
  saveSettings: (settings: ExtraConfig) => void
  stream: (stream: Stream) => void
}
