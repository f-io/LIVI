import { useCallback, useEffect, useState, useMemo } from 'react'
import { updateCameras as detectCameras } from '@utils/cameraDetection'
import { useStatusStore } from '@store/store'
import { SourcesSettingKey } from './types'

export const useCameraDetection = (initialState: Record<SourcesSettingKey, any>, autoSave: any) => {
  const setCameraFound = useStatusStore((s) => s.setCameraFound)
  const [cameras, setCameras] = useState<MediaDeviceInfo[]>([])

  const cameraOptions = useMemo(
    () =>
      cameras.length
        ? cameras.map((c) => ({ deviceId: c.deviceId ?? '', label: c.label || 'Camera' }))
        : [{ deviceId: '', label: 'No camera' }],
    [cameras]
  )

  const cameraIds = useMemo(() => cameraOptions.map((c) => c.deviceId), [cameraOptions])

  const safeCameraPersist = useCallback(
    async (cfgOrId: string | { camera?: string } | null | undefined) => {
      if (initialState.camera && initialState.camera !== '') return
      const cameraId = typeof cfgOrId === 'string' ? cfgOrId : cfgOrId?.camera
      await autoSave({ camera: cameraId ?? '' })
    },
    [autoSave, initialState.camera]
  )

  useEffect(() => {
    detectCameras(setCameraFound, safeCameraPersist, initialState).then(setCameras)

    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = (args[0] ?? {}) as { type?: string }
      if (data.type && ['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        detectCameras(setCameraFound, safeCameraPersist, initialState).then(setCameras)
      }
    }

    window.carplay.usb.listenForEvents(usbHandler)
    return () => window.carplay.usb.unlistenForEvents(usbHandler)
  }, [initialState, safeCameraPersist, setCameraFound])

  return { cameras, cameraOptions, cameraIds }
}
