import React from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import Tabs from '@mui/material/Tabs'
import Tab from '@mui/material/Tab'
import Tooltip from '@mui/material/Tooltip'
import Badge from '@mui/material/Badge'

import PhonelinkOffIcon from '@mui/icons-material/PhonelinkOff'
import PhonelinkIcon from '@mui/icons-material/Phonelink'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import InfoOutlinedIcon from '@mui/icons-material/InfoOutlined'
import CameraswitchOutlinedIcon from '@mui/icons-material/CameraswitchOutlined'
import PowerSettingsNewIcon from '@mui/icons-material/PowerSettingsNew'

import { useTheme } from '@mui/material/styles'
import { useStatusStore } from '../store/store'
import { ExtraConfig } from '../../../main/Globals'
import { indexToRoute, ROUTES, routeToIndex } from '../constants'

interface NavProps {
  settings: ExtraConfig | null
  receivingVideo: boolean
}

export default function Nav({ receivingVideo }: NavProps) {
  const theme = useTheme()
  const navigate = useNavigate()
  const { pathname } = useLocation()

  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const cameraFound = useStatusStore((s) => s.cameraFound)

  if (isStreaming && pathname === '/') return null

  const value = routeToIndex[pathname] ?? 0

  const blurActive = () => {
    const el = document.activeElement as HTMLElement | null
    if (el && typeof el.blur === 'function') el.blur()
  }

  const connectionColor = !isDongleConnected
    ? theme.palette.text.disabled
    : isStreaming && receivingVideo
      ? theme.palette.success.main
      : theme.palette.text.primary

  const connectionIcon = (
    <Tooltip
      title={
        isDongleConnected ? (isStreaming ? 'Connected (streaming)' : 'Connected') : 'Not connected'
      }
      disableFocusListener={value === 0}
    >
      <span>
        {isDongleConnected ? (
          isStreaming && receivingVideo ? (
            <Badge
              variant="dot"
              overlap="circular"
              sx={{ '& .MuiBadge-badge': { bgcolor: theme.palette.success.main } }}
            >
              <PhonelinkIcon sx={{ color: connectionColor, fontSize: 30 }} />
            </Badge>
          ) : (
            <PhonelinkIcon sx={{ color: connectionColor, fontSize: 30 }} />
          )
        ) : (
          <PhonelinkOffIcon sx={{ color: connectionColor, fontSize: 30 }} />
        )}
      </span>
    </Tooltip>
  )

  const mediaIcon = (
    <Tooltip title="Media" disableFocusListener={value === 1}>
      <span>
        <PlayCircleOutlinedIcon sx={{ fontSize: 30 }} />
      </span>
    </Tooltip>
  )

  const cameraIcon = (
    <Tooltip
      title={cameraFound ? 'Camera' : 'Camera (not available)'}
      disableFocusListener={value === 2}
    >
      <span>
        <CameraswitchOutlinedIcon sx={{ fontSize: 30 }} />
      </span>
    </Tooltip>
  )

  const infoIcon = (
    <Tooltip title="Info" disableFocusListener={value === 3}>
      <span>
        <InfoOutlinedIcon sx={{ fontSize: 30 }} />
      </span>
    </Tooltip>
  )

  const settingsIcon = (
    <Tooltip title="Settings" disableFocusListener={value === 4}>
      <span>
        <SettingsOutlinedIcon sx={{ fontSize: 30 }} />
      </span>
    </Tooltip>
  )

  const quitIcon = (
    <Tooltip title="Quit" disableFocusListener={value === 5}>
      <span>
        <PowerSettingsNewIcon sx={{ fontSize: 30 }} />
      </span>
    </Tooltip>
  )

  const quit = () => {
    window.carplay.quit().catch((err) => console.error('Quit failed:', err))
  }

  const handleChange = (_: React.SyntheticEvent, newIndex: number) => {
    const dest = indexToRoute[newIndex]
    if (dest === ROUTES.QUIT) {
      quit()
      requestAnimationFrame(blurActive)
      return
    }
    if (typeof dest === 'string') {
      navigate(dest)
      requestAnimationFrame(blurActive)
    }
  }

  const tabSx = {
    minWidth: 0,
    flex: '1 1 0',
    padding: '10px 0',
    '& .MuiTab-iconWrapper': { display: 'grid', placeItems: 'center' }
  } as const

  return (
    <Tabs
      value={value}
      onChange={handleChange}
      aria-label="Navigation Tabs"
      variant="fullWidth"
      textColor="inherit"
      indicatorColor="secondary"
      selectionFollowsFocus={false}
    >
      <Tab aria-label="CarPlay" sx={tabSx} icon={connectionIcon} />
      <Tab aria-label="Media" sx={tabSx} icon={mediaIcon} />
      <Tab aria-label="Camera" sx={tabSx} icon={cameraIcon} disabled={!cameraFound} />
      <Tab aria-label="Info" sx={tabSx} icon={infoIcon} />
      <Tab aria-label="Settings" sx={tabSx} icon={settingsIcon} />
      <Tab aria-label="Quit" sx={tabSx} icon={quitIcon} />
    </Tabs>
  )
}
