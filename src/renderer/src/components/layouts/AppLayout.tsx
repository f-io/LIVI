import { FC, PropsWithChildren } from 'react'
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

  const isVisibleTimeAndWifi = window.innerHeight > UI.MIN_HEIGHT_SHOW_TIME_WIFI

  // Hide nav column while streaming on home screen
  const hideNav = isStreaming && pathname === ROUTES.HOME

  // Steering wheel position
  const isRhd = Number(settings?.hand ?? 0) === 1
  const layoutDirection: 'row' | 'row-reverse' = isRhd ? 'row-reverse' : 'row'

  return (
    <div
      id="main"
      className="App"
      style={{
        height: '100dvh',
        touchAction: 'none',
        display: 'flex',
        flexDirection: layoutDirection
      }}
    >
      {/* NAV COLUMN */}
      {!hideNav && (
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
            zIndex: 10
          }}
        >
          {isVisibleTimeAndWifi && (
            <div
              style={{
                paddingTop: '1rem'
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
      )}

      {/* CONTENT COLUMN */}
      <div
        ref={mainRef}
        id="content-root"
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
