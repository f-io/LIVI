import CableOutlinedIcon from '@mui/icons-material/CableOutlined'
import DeviceHubIcon from '@mui/icons-material/DeviceHub'
import SyncIcon from '@mui/icons-material/Sync'
import WifiOutlinedIcon from '@mui/icons-material/WifiOutlined'

type Props = {
  active: 'dongle' | 'aa' | 'cp' | null
  wiredPhoneActive: boolean
  fontSize?: number
}

const BADGE_RATIO = 0.4
const ICON_BADGE_OFFSET_PX = 2

export function TransportSwitchIcon({ active, wiredPhoneActive, fontSize = 30 }: Props) {
  const isPhoneProtocol = active === 'aa' || active === 'cp'
  const badgePx = Math.round(fontSize * BADGE_RATIO)
  const iconBadgeSx = {
    fontSize: badgePx,
    position: 'absolute',
    bottom: -ICON_BADGE_OFFSET_PX,
    right: -ICON_BADGE_OFFSET_PX,
    pointerEvents: 'none'
  } as const

  let badge: React.ReactNode = null
  if (active === 'dongle') {
    badge = <DeviceHubIcon sx={iconBadgeSx} />
  } else if (isPhoneProtocol && wiredPhoneActive) {
    badge = <CableOutlinedIcon sx={iconBadgeSx} />
  } else if (isPhoneProtocol) {
    badge = <WifiOutlinedIcon sx={iconBadgeSx} />
  }

  return (
    <span style={{ position: 'relative', display: 'inline-flex', lineHeight: 0 }}>
      <SyncIcon sx={{ fontSize }} />
      {badge}
    </span>
  )
}
