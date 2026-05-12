// Icons
import CameraOutlinedIcon from '@mui/icons-material/CameraOutlined'
import CropPortraitOutlinedIcon from '@mui/icons-material/CropPortraitOutlined'
import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import SpeedOutlinedIcon from '@mui/icons-material/SpeedOutlined'
import { useTheme } from '@mui/material/styles'
import { ROUTES } from '../../constants'
import { useLiviStore, useStatusStore } from '../../store/store'
import { getWindowRole } from '../../utils/windowRole'
import { TransportFlipIcon } from './TransportFlipIcon'
import { TabConfig } from './types'
import { useTransportState } from './useTransportState'

export const useTabsConfig: (receivingVideo: boolean) => TabConfig[] = (receivingVideo) => {
  const theme = useTheme()
  const role = getWindowRole()
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const isAaActive = useStatusStore((s) => s.isAaActive)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected || s.isAaActive)
  const cameraFound = useStatusStore((s) => s.cameraFound)
  const cameraConfigured = useLiviStore((s) => Boolean(s.settings?.cameraId))
  const cameraReady = cameraFound || cameraConfigured
  const transport = useTransportState()
  const showFlip = role === 'main' && transport.dongleDetected && transport.nativeDetected
  const clusterEnabled = useLiviStore(
    (s) =>
      s.settings?.cluster?.main === true ||
      s.settings?.cluster?.dash === true ||
      s.settings?.cluster?.aux === true
  )
  const clusterOnRole = useLiviStore((s) =>
    role === 'main' ? (s.settings?.cluster?.main ?? true) : (s.settings?.cluster?.[role] ?? false)
  )
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
      ...(clusterEnabled && clusterOnRole
        ? [
            {
              label: 'Cluster Stream',
              path: ROUTES.CLUSTER,
              icon: <MapOutlinedIcon sx={{ fontSize: 30 }} />
            }
          ]
        : []),
      ...(telemetryOnRole
        ? [
            {
              label: 'Telemetry',
              path: ROUTES.TELEMETRY,
              icon: <SpeedOutlinedIcon sx={{ fontSize: 30 }} />
            }
          ]
        : []),
      ...(mediaOnRole
        ? [
            {
              label: 'Media',
              path: ROUTES.MEDIA,
              icon: <PlayCircleOutlinedIcon sx={{ fontSize: 30 }} />
            }
          ]
        : []),
      ...(cameraOnRole
        ? [
            {
              label: 'Camera',
              path: ROUTES.CAMERA,
              icon: <CameraOutlinedIcon sx={{ fontSize: 30 }} />,
              disabled: !cameraReady
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
        const usbConnected = isDongleConnected
        const phoneActive = isStreaming || isAaActive
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
    ...(clusterEnabled && clusterOnRole
      ? [
          {
            label: 'Cluster Stream',
            path: ROUTES.CLUSTER,
            icon: <MapOutlinedIcon sx={{ fontSize: 30 }} />
          }
        ]
      : []),
    ...(telemetryOnRole
      ? [
          {
            label: 'Telemetry',
            path: ROUTES.TELEMETRY,
            icon: <SpeedOutlinedIcon sx={{ fontSize: 30 }} />
          }
        ]
      : []),
    ...(mediaOnRole
      ? [
          {
            label: 'Media',
            path: ROUTES.MEDIA,
            icon: <PlayCircleOutlinedIcon sx={{ fontSize: 30 }} />
          }
        ]
      : []),
    ...(cameraOnRole
      ? [
          {
            label: 'Camera',
            path: ROUTES.CAMERA,
            icon: <CameraOutlinedIcon sx={{ fontSize: 30 }} />,
            disabled: !cameraReady
          }
        ]
      : []),
    ...(showFlip
      ? [
          {
            label: 'Switch transport',
            path: ROUTES.TRANSPORT_FLIP,
            icon: <TransportFlipIcon active={transport.active} fontSize={30} />
          }
        ]
      : []),
    {
      label: 'Settings',
      path: ROUTES.SETTINGS,
      icon: <SettingsOutlinedIcon sx={{ fontSize: 30 }} />
    }
  ]
}
