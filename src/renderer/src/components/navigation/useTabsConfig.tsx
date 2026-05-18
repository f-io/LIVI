// Icons
import CameraOutlinedIcon from '@mui/icons-material/CameraOutlined'
import CropPortraitOutlinedIcon from '@mui/icons-material/CropPortraitOutlined'
import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import SpeedOutlinedIcon from '@mui/icons-material/SpeedOutlined'
import { useTheme } from '@mui/material/styles'
import { useEffect, useState } from 'react'
import { ROUTES, UI } from '../../constants'
import { useLiviStore, useStatusStore } from '../../store/store'
import { getWindowRole } from '../../utils/windowRole'
import { TransportSwitchIcon } from './TransportSwitchIcon'
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
  const isXSIcons = typeof window !== 'undefined' && window.innerHeight <= UI.XS_ICON_MAX_HEIGHT
  const iconFontSize = isXSIcons ? 24 : 32
  const detectedCount =
    Number(transport.dongleDetected) +
    Number(transport.wiredPhoneDetected) +
    Number(transport.wirelessPhoneDetected)
  const rawShowSwitch = role === 'main' && (detectedCount >= 2 || transport.switchPending)
  const showSwitch = useDelayedHide(rawShowSwitch, 300)
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
              icon: <MapOutlinedIcon sx={{ fontSize: iconFontSize }} />
            }
          ]
        : []),
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
        const usbConnected = isDongleConnected
        const phoneActive = isStreaming || isAaActive
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
    ...(clusterEnabled && clusterOnRole
      ? [
          {
            label: 'Cluster Stream',
            path: ROUTES.CLUSTER,
            icon: <MapOutlinedIcon sx={{ fontSize: iconFontSize }} />
          }
        ]
      : []),
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
    ...(showSwitch
      ? [
          {
            label: 'Switch transport',
            path: ROUTES.TRANSPORT_SWITCH,
            icon: (
              <TransportSwitchIcon
                active={transport.targetTransport ?? transport.active}
                wiredPhoneActive={
                  transport.targetMode
                    ? transport.targetMode === 'wired'
                    : transport.wiredPhoneActive
                }
                fontSize={iconFontSize}
              />
            )
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

function useDelayedHide(value: boolean, delayMs: number): boolean {
  const [held, setHeld] = useState(value)
  useEffect(() => {
    if (value) {
      setHeld(true)
      return
    }
    const t = setTimeout(() => setHeld(false), delayMs)
    return () => clearTimeout(t)
  }, [value, delayMs])
  return held
}
