import type { SettingsCustomPageProps } from '../../type'
import type { ExtraConfig } from '@main/Globals'
import { updateCameras as detectCameras } from '@utils/cameraDetection'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useStatusStore } from '@store/store'
import { UsbEvent } from '../../../media/types'
import { MenuItem, Select, Typography } from '@mui/material'

function coerceSelectValue<T extends string | number>(
  value: T | null | undefined,
  options: readonly T[]
): T | '' {
  return value != null && options.includes(value as T) ? (value as T) : ''
}

export const Camera: React.FC<SettingsCustomPageProps<ExtraConfig, string>> = ({
  state,
  onChange
}) => {
  const [cameras, setCameras] = useState<MediaDeviceInfo[]>([])
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  const setCameraFound = useStatusStore((s) => s.setCameraFound)

  const safeCameraPersist = useCallback(
    async (cfgOrId: string | { camera?: string } | null | undefined) => {
      if (state.camera && state.camera !== '') return
      const cameraId = typeof cfgOrId === 'string' ? cfgOrId : cfgOrId?.camera

      if (cameraId && cameraId !== '') onChange(cameraId)
    },
    [onChange, state.camera]
  )

  useEffect(() => {
    detectCameras(setCameraFound, safeCameraPersist, state).then(setCameras)

    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = (args[0] ?? {}) as UsbEvent
      if (data.type && ['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        detectCameras(setCameraFound, safeCameraPersist, state).then(setCameras)
      }
    }
    window.carplay.usb.listenForEvents(usbHandler)
    return () => window.carplay.usb.unlistenForEvents(usbHandler)
  }, [safeCameraPersist, setCameraFound, state])

  const cameraOptions = useMemo<readonly { deviceId: string; label: string }[]>(
    () =>
      cameras.length
        ? cameras.map((c) => ({ deviceId: c.deviceId ?? '', label: c.label || 'Camera' }))
        : [{ deviceId: '', label: 'No camera' }],
    [cameras]
  )
  const cameraIds = useMemo<readonly string[]>(
    () => cameraOptions.map((c) => c.deviceId),
    [cameraOptions]
  )
  const cameraValue = coerceSelectValue(state.camera ?? '', cameraIds)

  return (
    <>
      <div style={{ marginTop: 16 }}>
        {cameraOptions.length ? (
          <>
            <Select
              size="small"
              variant="outlined"
              value={cameraValue}
              sx={{
                minWidth: 200,
                width: '100%',

                '&.Mui-focused .MuiOutlinedInput-notchedOutline': {
                  borderColor: 'primary.main',
                  borderWidth: '1px'
                },

                '& .MuiSelect-select': {
                  display: 'flex',
                  alignItems: 'center',
                  minHeight: 0
                }
              }}
              onChange={(e) => onChange(e.target.value)}
            >
              {cameraOptions.map((o) => (
                <MenuItem key={o.deviceId || 'none'} value={o.deviceId}>
                  {o.label}
                </MenuItem>
              ))}
            </Select>

            <Typography color="text.secondary" mb={2}>
              Source
            </Typography>
          </>
        ) : (
          <>
            <Typography color="text.secondary" mb={2}>
              No camera detected
            </Typography>
          </>
        )}
      </div>
    </>
  )
}
