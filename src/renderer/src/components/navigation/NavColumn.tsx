import WifiIcon from '@mui/icons-material/Wifi'
import WifiOffIcon from '@mui/icons-material/WifiOff'
import Box from '@mui/material/Box'
import { useTheme } from '@mui/material/styles'
import Typography from '@mui/material/Typography'
import { forwardRef } from 'react'
import { UI } from '../../constants'
import { useBlinkingTime } from '../../hooks/useBlinkingTime'
import { useNetworkStatus } from '../../hooks/useNetworkStatus'
import { NavRail, type NavRailItem } from './NavRail'

type Props = {
  items: NavRailItem[]
  activeKey: string
  onSelect: (key: string, index: number) => void
  hidden: boolean
  ariaLabel?: string
  side?: 'left' | 'right'
}

export const NavColumn = forwardRef<HTMLDivElement, Props>(function NavColumn(
  { items, activeKey, onSelect, hidden, ariaLabel = 'Navigation', side = 'left' },
  ref
) {
  const theme = useTheme()
  const time = useBlinkingTime()
  const network = useNetworkStatus()

  const isVisibleTimeAndWifi =
    typeof window !== 'undefined' && window.innerHeight > UI.MIN_HEIGHT_SHOW_TIME_WIFI

  const slideX = hidden ? (side === 'right' ? '10px' : '-10px') : '0'

  return (
    <Box
      ref={ref}
      sx={{
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        borderRight: side === 'left' ? '1px solid #444' : undefined,
        borderLeft: side === 'right' ? '1px solid #444' : undefined,
        flex: '0 0 auto',
        position: 'relative',
        zIndex: 10,
        opacity: hidden ? 0 : 1,
        transform: `translateX(${slideX})`,
        transition: 'opacity 220ms ease, transform 220ms ease',
        pointerEvents: hidden ? 'none' : 'auto'
      }}
    >
      {isVisibleTimeAndWifi && (
        <Box
          sx={{
            paddingTop: '1rem',
            background: theme.palette.background.paper,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center'
          }}
        >
          <Typography sx={{ fontSize: '1.5rem' }}>{time}</Typography>
          <Box>
            {network.type === 'wifi' ? (
              <WifiIcon fontSize="small" sx={{ fontSize: '1rem' }} />
            ) : !network.online ? (
              <WifiOffIcon fontSize="small" sx={{ fontSize: '1rem', opacity: 0.7 }} />
            ) : null}
          </Box>
        </Box>
      )}

      <Box sx={{ flex: 1, minHeight: 0, display: 'flex' }}>
        <NavRail items={items} activeKey={activeKey} onSelect={onSelect} ariaLabel={ariaLabel} />
      </Box>
    </Box>
  )
})
