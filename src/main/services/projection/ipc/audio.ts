import { registerIpcOn } from '@main/ipc/register'
import type { LogicalStreamKey } from '../services/ProjectionAudio'
import type { ProjectionIpcHost } from './types'

type Deps = Pick<ProjectionIpcHost, 'setAudioStreamVolume' | 'setAudioVisualizerEnabled'>

export function registerAudioIpc(host: Deps): void {
  registerIpcOn(
    'projection-set-volume',
    (_evt, payload: { stream: LogicalStreamKey; volume: number }) => {
      const { stream, volume } = payload || {}
      host.setAudioStreamVolume(stream, volume)
    }
  )

  registerIpcOn('projection-set-visualizer-enabled', (_evt, enabled: boolean) => {
    host.setAudioVisualizerEnabled(Boolean(enabled))
  })
}
