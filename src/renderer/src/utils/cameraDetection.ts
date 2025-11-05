import { ExtraConfig } from '@main/Globals'

export async function updateCameras(
  setCameraFound: (found: boolean) => void,
  saveSettings: (cfg: ExtraConfig) => void,
  currentSettings: ExtraConfig
): Promise<MediaDeviceInfo[]> {
  try {
    const devs = await navigator.mediaDevices.enumerateDevices()
    const cams = devs.filter((d) => d.kind === 'videoinput')
    setCameraFound(cams.length > 0)

    if (!currentSettings.camera && cams.length > 0) {
      const updated = { ...currentSettings, camera: cams[0].deviceId }
      saveSettings(updated)
    }

    return cams
  } catch (err) {
    console.warn('[CameraDetection] enumerateDevices failed', err)
    return []
  }
}
