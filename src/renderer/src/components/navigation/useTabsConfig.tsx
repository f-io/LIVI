import { TabConfig } from './types'
import { ROUTES } from '../../constants'
import { useTheme } from '@mui/material/styles'
import { useStatusStore, useCarplayStore } from '../../store/store'

// Icons
import CropPortraitOutlinedIcon from '@mui/icons-material/CropPortraitOutlined'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import CameraswitchOutlinedIcon from '@mui/icons-material/CameraswitchOutlined'
import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import SpeedOutlinedIcon from '@mui/icons-material/SpeedOutlined'

export const useTabsConfig: (receivingVideo: boolean) => TabConfig[] = (receivingVideo) => {
  const theme = useTheme()
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const cameraFound = useStatusStore((s) => s.cameraFound)
  const mapsEnabled = useCarplayStore((s) => s.settings?.mapsEnabled ?? false)
  const telemetryEnabled = useCarplayStore((s) => s.settings?.telemetryEnabled ?? false)

  return [
    {
      label: 'CarPlay',
      path: ROUTES.HOME,
      icon: (() => {
        const usbConnected = isDongleConnected
        const phoneActive = isStreaming && receivingVideo
        const baseColor = usbConnected ? theme.palette.text.primary : theme.palette.text.disabled
        const activeColor = 'var(--ui-highlight)'

        if (!usbConnected) {
          return <CropPortraitOutlinedIcon sx={{ color: baseColor, fontSize: 30 }} />
        }

        return (
          <CropPortraitOutlinedIcon
            sx={{
              fontSize: 30,
              color: phoneActive ? activeColor : baseColor,
              '&, &.MuiSvgIcon-root': {
                color: `${phoneActive ? activeColor : baseColor} !important`
              },
              animation: !phoneActive ? 'ui-breathe 1350ms ease-in-out infinite' : 'none'
            }}
          />
        )
      })()
    },
    ...(mapsEnabled
      ? [
          {
            label: 'Maps',
            path: ROUTES.MAPS,
            icon: <MapOutlinedIcon sx={{ fontSize: 30 }} />
          }
        ]
      : []),
    ...(telemetryEnabled
      ? [
          {
            label: 'Telemetry',
            path: ROUTES.TELEMETRY,
            icon: <SpeedOutlinedIcon sx={{ fontSize: 30 }} />
          }
        ]
      : []),
    { label: 'Media', path: ROUTES.MEDIA, icon: <PlayCircleOutlinedIcon sx={{ fontSize: 30 }} /> },
    {
      label: 'Camera',
      path: ROUTES.CAMERA,
      icon: <CameraswitchOutlinedIcon sx={{ fontSize: 30 }} />,
      disabled: !cameraFound
    },
    {
      label: 'Settings',
      path: ROUTES.SETTINGS,
      icon: <SettingsOutlinedIcon sx={{ fontSize: 30 }} />
    }
  ]
}
