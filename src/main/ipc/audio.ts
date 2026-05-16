import { registerIpcHandle } from '@main/ipc/register'
import { listAudioDevices } from '@main/services/audio/AudioDeviceEnumerator'

export function registerAudioIpc(): void {
  registerIpcHandle('audio:listSinks', async () => listAudioDevices('sink'))
  registerIpcHandle('audio:listSources', async () => listAudioDevices('source'))
}
