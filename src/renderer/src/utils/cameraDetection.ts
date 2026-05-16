import type { Config } from '@shared/types'

export async function updateCameras(
  setCameraFound: (found: boolean) => void,
  saveSettings: (cfg: Config) => void,
  currentSettings: Config
): Promise<MediaDeviceInfo[]> {
  try {
    const devs = await navigator.mediaDevices.enumerateDevices()
    const cams = devs.filter((d) => d.kind === 'videoinput')
    setCameraFound(cams.length > 0)

    if (!currentSettings.cameraId && cams.length > 0) {
      const updated = { ...currentSettings, cameraId: cams[0].deviceId }
      saveSettings(updated)
    }

    return cams
  } catch (err) {
    console.warn('[CameraDetection] enumerateDevices failed', err)
    return []
  }
}
