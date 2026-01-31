import { TabConfig } from './types'
import Badge from '@mui/material/Badge'
import { ROUTES } from '../../constants'
import { useTheme } from '@mui/material/styles'
import { useStatusStore, useCarplayStore } from '../../store/store'

import PhonelinkOffIcon from '@mui/icons-material/PhonelinkOff'
import PhonelinkIcon from '@mui/icons-material/Phonelink'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import CameraswitchOutlinedIcon from '@mui/icons-material/CameraswitchOutlined'
import MapOutlinedIcon from '@mui/icons-material/MapOutlined'

export const useTabsConfig: (receivingVideo: boolean) => TabConfig[] = (receivingVideo) => {
  const theme = useTheme()
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const cameraFound = useStatusStore((s) => s.cameraFound)

  const enableMaps = useCarplayStore((s) => !!s.settings?.enableMaps)

  return [
    {
      label: 'CarPlay',
      path: ROUTES.HOME,
      icon: isDongleConnected ? (
        isStreaming && receivingVideo ? (
          <Badge
            variant="dot"
            overlap="circular"
            sx={{ '& .MuiBadge-badge': { bgcolor: theme.palette.success.main } }}
          >
            <PhonelinkIcon sx={{ color: theme.palette.success.main, fontSize: 30 }} />
          </Badge>
        ) : (
          <PhonelinkIcon sx={{ color: theme.palette.text.primary, fontSize: 30 }} />
        )
      ) : (
        <PhonelinkOffIcon sx={{ color: theme.palette.text.disabled, fontSize: 30 }} />
      )
    },
    ...(enableMaps
      ? [
          {
            label: 'Maps',
            path: ROUTES.MAPS,
            icon: <MapOutlinedIcon sx={{ fontSize: 30 }} />
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
