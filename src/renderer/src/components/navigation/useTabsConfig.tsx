// Icons
import CameraOutlinedIcon from '@mui/icons-material/CameraOutlined'
import CropPortraitOutlinedIcon from '@mui/icons-material/CropPortraitOutlined'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import SpeedOutlinedIcon from '@mui/icons-material/SpeedOutlined'
import { useTheme } from '@mui/material/styles'
import { ROUTES, UI } from '../../constants'
import { useLiviStore, useProjectionActive, useStatusStore } from '../../store/store'
import { getWindowRole } from '../../utils/windowRole'
import { TabConfig } from './types'

export const useTabsConfig: (receivingVideo: boolean) => TabConfig[] = (receivingVideo) => {
  const theme = useTheme()
  const role = getWindowRole()
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const activeProtocol = useStatusStore((s) => s.activeProtocol)
  const isProjectionActive = useProjectionActive()
  const cameraFound = useStatusStore((s) => s.cameraFound)
  const cameraConfigured = useLiviStore((s) => Boolean(s.settings?.cameraId))
  const cameraReady = cameraFound || cameraConfigured
  const isXSIcons = typeof window !== 'undefined' && window.innerHeight <= UI.XS_ICON_MAX_HEIGHT
  const iconFontSize = isXSIcons ? 24 : 32
  const cameraOnRole = useLiviStore((s) =>
    role === 'main' ? (s.settings?.camera?.main ?? true) : (s.settings?.camera?.[role] ?? false)
  )
  const mediaOnRole = useLiviStore((s) =>
    role === 'main' ? (s.settings?.media?.main ?? true) : (s.settings?.media?.[role] ?? false)
  )
  const telemetryOnRole = useLiviStore((s) => {
    const d = s.settings?.dashboards
    if (!d) return false
    return Object.values(d).some((slot) => slot?.[role] === true)
  })

  // Secondary windows only show tabs that are routed to that role
  if (role !== 'main') {
    return [
      ...(telemetryOnRole
        ? [
            {
              label: 'Telemetry',
              path: ROUTES.TELEMETRY,
              icon: <SpeedOutlinedIcon sx={{ fontSize: iconFontSize }} />
            }
          ]
        : []),
      ...(mediaOnRole
        ? [
            {
              label: 'Media',
              path: ROUTES.MEDIA,
              icon: <PlayCircleOutlinedIcon sx={{ fontSize: iconFontSize }} />
            }
          ]
        : []),
      ...(cameraOnRole && cameraReady
        ? [
            {
              label: 'Camera',
              path: ROUTES.CAMERA,
              icon: <CameraOutlinedIcon sx={{ fontSize: iconFontSize }} />
            }
          ]
        : [])
    ]
  }

  return [
    {
      label: 'Projection',
      path: ROUTES.HOME,
      icon: (() => {
        const usbConnected = isProjectionActive
        const phoneActive =
          isStreaming || activeProtocol === 'androidauto' || activeProtocol === 'carplay'
        const baseColor = usbConnected ? theme.palette.text.primary : theme.palette.text.disabled
        const activeColor = 'var(--ui-highlight)'

        if (!usbConnected) {
          return <CropPortraitOutlinedIcon sx={{ color: baseColor, fontSize: iconFontSize }} />
        }

        return (
          <CropPortraitOutlinedIcon
            sx={{
              fontSize: iconFontSize,
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
    ...(telemetryOnRole
      ? [
          {
            label: 'Telemetry',
            path: ROUTES.TELEMETRY,
            icon: <SpeedOutlinedIcon sx={{ fontSize: iconFontSize }} />
          }
        ]
      : []),
    ...(mediaOnRole
      ? [
          {
            label: 'Media',
            path: ROUTES.MEDIA,
            icon: <PlayCircleOutlinedIcon sx={{ fontSize: iconFontSize }} />
          }
        ]
      : []),
    ...(cameraOnRole && cameraReady
      ? [
          {
            label: 'Camera',
            path: ROUTES.CAMERA,
            icon: <CameraOutlinedIcon sx={{ fontSize: iconFontSize }} />
          }
        ]
      : []),
    {
      label: 'Settings',
      path: ROUTES.SETTINGS,
      icon: <SettingsOutlinedIcon sx={{ fontSize: iconFontSize }} />
    }
  ]
}
