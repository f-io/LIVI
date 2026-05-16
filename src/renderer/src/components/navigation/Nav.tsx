import type { Config } from '@shared/types'
import { useLocation, useNavigate } from 'react-router'
import { ROUTES } from '../../constants'
import { useBlinkingTime } from '../../hooks/useBlinkingTime'
import { useNetworkStatus } from '../../hooks/useNetworkStatus'
import { useStatusStore } from '../../store/store'
import { NavRail, type NavRailItem } from './NavRail'
import { useTabsConfig } from './useTabsConfig'

interface NavProps {
  settings: Config | null
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

  const items: NavRailItem[] = tabs.map((t) => ({
    key: t.path,
    icon: t.icon,
    label: t.label,
    disabled: t.disabled
  }))

  const activeKey =
    items.find((t) => {
      if (t.key === ROUTES.HOME) return pathname === ROUTES.HOME
      return pathname.startsWith(t.key)
    })?.key ??
    items[0]?.key ??
    ROUTES.HOME

  const handleSelect = (key: string) => {
    if (key === ROUTES.QUIT) {
      window.projection.quit().catch(console.error)
      return
    }

    if (key === ROUTES.TRANSPORT_FLIP) {
      void window.projection.ipc.flipTransport?.().catch(console.error)
      return
    }

    if (key === ROUTES.SETTINGS && pathname.startsWith(ROUTES.SETTINGS)) {
      navigate(ROUTES.SETTINGS, { replace: true })
      return
    }

    navigate(key)
  }

  return (
    <NavRail
      items={items}
      activeKey={activeKey}
      onSelect={handleSelect}
      ariaLabel="Navigation Tabs"
    />
  )
}
