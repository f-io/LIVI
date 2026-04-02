import CameraswitchOutlinedIcon from '@mui/icons-material/CameraswitchOutlined'
// Icons
import CropPortraitOutlinedIcon from '@mui/icons-material/CropPortraitOutlined'
import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import SpeedOutlinedIcon from '@mui/icons-material/SpeedOutlined'
import { useTheme } from '@mui/material/styles'
import { ROUTES } from '../../constants'
import { useLiviStore, useStatusStore } from '../../store/store'
import { TabConfig } from './types'

export const useTabsConfig: (receivingVideo: boolean) => TabConfig[] = (receivingVideo) => {
  const theme = useTheme()
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const cameraFound = useStatusStore((s) => s.cameraFound)
  const mapsEnabled = useLiviStore((s) => s.settings?.mapsEnabled ?? false)
  const telemetryEnabled = useLiviStore((s) => s.settings?.telemetryEnabled ?? false)

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
              opacity: !phoneActive ? 'var(--ui-breathe-opacity, 1)' : 1
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
