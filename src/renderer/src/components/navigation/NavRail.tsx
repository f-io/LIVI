import { useTheme } from '@mui/material/styles'
import Tab from '@mui/material/Tab'
import Tabs from '@mui/material/Tabs'
import type { ReactElement } from 'react'
import { UI } from '../../constants'

export type NavRailItem = {
  key: string
  icon: ReactElement
  label: string
  disabled?: boolean
}

type Props = {
  items: NavRailItem[]
  activeKey: string
  onSelect: (key: string, index: number) => void
  ariaLabel?: string
}

export const NavRail = ({ items, activeKey, onSelect, ariaLabel = 'Navigation' }: Props) => {
  const theme = useTheme()
  const value = Math.max(
    0,
    items.findIndex((t) => t.key === activeKey)
  )
  const isXSIcons = typeof window !== 'undefined' && window.innerHeight <= UI.XS_ICON_MAX_HEIGHT

  const tabSx = {
    minWidth: 0,
    flex: '1 1 0',
    padding: isXSIcons ? '5px 0' : '10px 0',
    '& .MuiTab-iconWrapper': { display: 'grid', placeItems: 'center' },
    '& .MuiSvgIcon-root': {
      fontSize: isXSIcons ? '1.5rem' : '2rem',
      transition: 'color 120ms ease-out'
    },
    minHeight: 'auto',

    '&.Mui-focusVisible, &:hover': { opacity: 1 },
    '&.Mui-focusVisible .MuiSvgIcon-root, &:hover .MuiSvgIcon-root': {
      color: `${theme.palette.primary.main} !important`
    }
  } as const

  return (
    <Tabs
      value={value}
      onChange={(_, i) => onSelect(items[i].key, i)}
      aria-label={ariaLabel}
      variant="fullWidth"
      textColor="inherit"
      visibleScrollbar={false}
      selectionFollowsFocus={false}
      orientation="vertical"
      sx={{
        '& .MuiTabs-indicator': { display: 'none' },
        '& .MuiTabs-list': { height: '100%' },
        height: '100%'
      }}
    >
      {items.map((tab) => (
        <Tab
          key={tab.key}
          sx={tabSx}
          icon={tab.icon}
          aria-label={tab.label}
          disabled={tab.disabled}
          disableRipple
          disableFocusRipple
          disableTouchRipple
          onClick={() => onSelect(tab.key, items.indexOf(tab))}
        />
      ))}
    </Tabs>
  )
}
