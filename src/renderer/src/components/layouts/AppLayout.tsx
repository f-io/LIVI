import { FC, PropsWithChildren, useCallback, useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router'
import { Nav } from '../navigation'
import { useCarplayStore, useStatusStore } from '@store/store'
import { AppLayoutProps } from './types'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import WifiIcon from '@mui/icons-material/Wifi'
import WifiOffIcon from '@mui/icons-material/WifiOff'
import { useBlinkingTime } from '../../hooks/useBlinkingTime'
import { useNetworkStatus } from '../../hooks/useNetworkStatus'
import { ROUTES, UI } from '../../constants'
import { useTheme } from '@mui/material/styles'

export const AppLayout: FC<PropsWithChildren<AppLayoutProps>> = ({
  children,
  navRef,
  mainRef,
  receivingVideo
}) => {
  const { pathname } = useLocation()
  const settings = useCarplayStore((s) => s.settings)
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const time = useBlinkingTime()
  const network = useNetworkStatus()
  const theme = useTheme()

  const isVisibleTimeAndWifi = window.innerHeight > UI.MIN_HEIGHT_SHOW_TIME_WIFI

  // Auto-hide nav on Maps
  const NAV_HIDE_DELAY_MS = UI.INACTIVITY_HIDE_DELAY_MS
  const hideTimerRef = useRef<number | null>(null)
  const [mapsNavHidden, setMapsNavHidden] = useState(false)

  const inAutoHideNavPage = pathname === ROUTES.MAPS || pathname === ROUTES.TELEMETRY

  const clearHideTimer = useCallback(() => {
    if (hideTimerRef.current != null) {
      window.clearTimeout(hideTimerRef.current)
      hideTimerRef.current = null
    }
  }, [])

  const scheduleHide = useCallback(() => {
    clearHideTimer()
    hideTimerRef.current = window.setTimeout(() => {
      setMapsNavHidden(true)
      hideTimerRef.current = null
    }, NAV_HIDE_DELAY_MS)
  }, [clearHideTimer, NAV_HIDE_DELAY_MS])

  const showNavAndArmHide = useCallback(() => {
    setMapsNavHidden(false)
    if (inAutoHideNavPage) scheduleHide()
  }, [inAutoHideNavPage, scheduleHide])

  useEffect(() => {
    if (!inAutoHideNavPage) {
      clearHideTimer()
      setMapsNavHidden(false)
      return
    }

    setMapsNavHidden(false)
    scheduleHide()

    return () => {
      clearHideTimer()
    }
  }, [inAutoHideNavPage, scheduleHide, clearHideTimer])

  useEffect(() => {
    if (!inAutoHideNavPage) return

    const wake: EventListener = () => {
      showNavAndArmHide()
    }

    window.addEventListener('keydown', wake, { passive: true })
    document.addEventListener('mousemove', wake, { passive: true })
    document.addEventListener('wheel', wake, { passive: true })

    return () => {
      window.removeEventListener('keydown', wake)
      document.removeEventListener('mousemove', wake)
      document.removeEventListener('wheel', wake)
    }
  }, [inAutoHideNavPage, showNavAndArmHide])

  // Hide nav column while streaming on home screen
  const hideNavHome = isStreaming && pathname === ROUTES.HOME

  // Auto-hide nav on Maps after inactivity
  const hideNav = hideNavHome || (inAutoHideNavPage && mapsNavHidden)

  // Steering wheel position
  const isRhd = Number(settings?.hand ?? 0) === 1
  const layoutDirection: 'row' | 'row-reverse' = isRhd ? 'row-reverse' : 'row'

  const onUserActivity = useCallback(() => {
    window.app?.notifyUserActivity?.()
  }, [])

  return (
    <div
      id="main"
      className="App"
      onPointerDownCapture={onUserActivity}
      style={{
        height: '100dvh',
        touchAction: 'none',
        display: 'flex',
        flexDirection: layoutDirection
      }}
    >
      {/* NAV COLUMN */}
      <div
        ref={navRef}
        id="nav-root"
        style={{
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          borderRight: isRhd ? undefined : '1px solid #444',
          borderLeft: isRhd ? '1px solid #444' : undefined,
          flex: '0 0 auto',
          position: 'relative',
          zIndex: 10,
          opacity: hideNav ? 0 : 1,
          transform: hideNav ? (isRhd ? 'translateX(10px)' : 'translateX(-10px)') : 'translateX(0)',
          transition: 'opacity 220ms ease, transform 220ms ease',
          pointerEvents: hideNav ? 'none' : 'auto'
        }}
      >
        {isVisibleTimeAndWifi && (
          <div
            style={{
              paddingTop: '1rem',
              background: theme.palette.background.paper
            }}
          >
            <Box sx={{ display: 'flex', alignItems: 'center', flexDirection: 'column' }}>
              <Typography style={{ fontSize: '1.5rem' }}>{time}</Typography>

              <div>
                {network.type === 'wifi' ? (
                  <WifiIcon fontSize="small" style={{ fontSize: '1rem' }} />
                ) : !network.online ? (
                  <WifiOffIcon fontSize="small" style={{ fontSize: '1rem', opacity: 0.7 }} />
                ) : null}
              </div>
            </Box>
          </div>
        )}

        {/* Nav should fill remaining height */}
        <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
          <Nav receivingVideo={receivingVideo} settings={settings} />
        </div>
      </div>

      {/* CONTENT COLUMN */}
      <div
        ref={mainRef}
        id="content-root"
        data-nav-hidden={hideNav ? '1' : '0'}
        style={{
          flex: 1,
          minWidth: 0,
          height: '100%',
          position: 'relative',
          overflow: 'hidden'
        }}
      >
        {children}
      </div>
    </div>
  )
}
