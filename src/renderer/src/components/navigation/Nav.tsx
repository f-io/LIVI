import React from 'react'
import { useNavigate, useLocation } from 'react-router'
import Tabs from '@mui/material/Tabs'
import Tab from '@mui/material/Tab'
import { useStatusStore } from '../../store/store'
import { ExtraConfig } from '../../../../main/Globals'
import { useTabsConfig } from './useTabsConfig'
import { ROUTES } from '../../constants'
import { useBlinkingTime } from '../../hooks/useBlinkingTime'
import { useNetworkStatus } from '../../hooks/useNetworkStatus'

interface NavProps {
  settings: ExtraConfig | null
  receivingVideo: boolean
}

export const Nav = ({ receivingVideo }: NavProps) => {
  const navigate = useNavigate()
  const { pathname } = useLocation()

  useBlinkingTime()
  useNetworkStatus()

  const isStreaming = useStatusStore((s) => s.isStreaming)
  const tabs = useTabsConfig(receivingVideo)

  if (isStreaming && pathname === ROUTES.HOME) return null

  const activeIndex = tabs.findIndex((t) => {
    if (t.path === ROUTES.HOME) {
      return pathname === ROUTES.HOME
    }
    return pathname.startsWith(t.path)
  })

  const value = activeIndex >= 0 ? activeIndex : 0

  const handleChange = (_: React.SyntheticEvent, newIndex: number) => {
    const tab = tabs[newIndex]
    if (tab.path === ROUTES.QUIT) {
      window.carplay.quit().catch(console.error)
      return
    }
    navigate(tab.path)
  }

  // TODO move it to global UI constants
  const isXSIcons = window.innerHeight <= 320

  const tabSx = {
    minWidth: 0,
    flex: '1 1 0',
    padding: isXSIcons ? '5px 0' : '10px 0',
    '& .MuiTab-iconWrapper': { display: 'grid', placeItems: 'center' },
    '& .MuiSvgIcon-root': {
      fontSize: isXSIcons ? '1.5rem' : '2rem'
    },
    minHeight: 'auto'
  } as const

  return (
    <>
      <Tabs
        value={value}
        onChange={handleChange}
        aria-label="Navigation Tabs"
        variant="fullWidth"
        textColor="inherit"
        visibleScrollbar={false}
        selectionFollowsFocus={false}
        orientation="vertical"
        sx={{
          '& .MuiTabs-indicator': {
            display: 'none'
          },
          '& .MuiTabs-list': {
            height: '100%'
          },
          height: '100%'
        }}
      >
        {tabs.map((tab) => (
          <Tab
            key={tab.path}
            sx={tabSx}
            icon={tab.icon}
            disabled={tab.disabled}
            aria-label={tab.label}
          />
        ))}
      </Tabs>
    </>
  )
}
