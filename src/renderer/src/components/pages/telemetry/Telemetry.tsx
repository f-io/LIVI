import { Box, useTheme } from '@mui/material'
import { DashPlaceholder } from '@renderer/components/pages/telemetry/components/DashPlaceholder'
import { DashboardsPagination } from '@renderer/components/pages/telemetry/components/pagination/pagination'
import { DashboardConfig } from '@renderer/components/pages/telemetry/config'
import { normalizeDashComponents } from '@renderer/components/pages/telemetry/utils'
import { AppContext } from '@renderer/context'
import { useNavbarHidden } from '@renderer/hooks/useNavbarHidden'
import { useLiviStore } from '@store/store'
import { clamp } from '@utils/index'
import * as React from 'react'
import { FC, useContext, useEffect, useState } from 'react'
import { useKeyboardNavigation } from './hooks/useKeyboardNavigation'

export const Telemetry: FC = () => {
  const theme = useTheme()
  const settings = useLiviStore((s) => s.settings)
  const { onSetAppContext } = useContext(AppContext)
  const [index, setIndex] = useState(0)

  const { dashboards } = normalizeDashComponents(settings?.telemetryDashboards)
  const { isNavbarHidden } = useNavbarHidden()
  const { prev, next, canPrev, canNext, onPointerDown, onPointerUp } = useKeyboardNavigation({
    dashboards,
    isNavbarHidden,
    index,
    onSetIndex: setIndex
  })

  useEffect(() => {
    setIndex((prev) => clamp(prev, 0, Math.max(0, dashboards.length - 1)))
  }, [dashboards.length])

  React.useEffect(() => {
    if (!onSetAppContext) return

    // register (PATCH only!)
    onSetAppContext({
      telemetryPager: { prev, next, canPrev, canNext }
    })

    // cleanup on unmount
    return () => {
      onSetAppContext({
        telemetryPager: undefined
      })
    }
  }, [onSetAppContext, prev, next, canPrev, canNext])

  const DashboardFallback = ({ message }: { message?: string }) => {
    const msg = message || 'Unknown dash'
    return (
      <Box
        sx={{
          position: 'absolute',
          inset: 0,
          backgroundColor: theme.palette.background.default,
          display: 'grid',
          placeItems: 'center',
          opacity: 0.8
        }}
      >
        <DashPlaceholder title={msg} />
      </Box>
    )
  }

  const renderDashboard = () => {
    return dashboards.map((d) => ({
      id: d.id,
      pos: d.pos,
      Component: DashboardConfig[d.id as keyof typeof DashboardConfig] || <DashboardFallback />
    }))
  }

  return (
    <Box
      sx={{
        position: isNavbarHidden ? 'fixed' : 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        backgroundColor: theme.palette.background.default
      }}
      onPointerDown={onPointerDown}
      onPointerUp={onPointerUp}
    >
      {dashboards.length ? (
        <>{renderDashboard()[index]?.Component || <DashboardFallback />}</>
      ) : (
        <DashboardFallback message="No dashboards enabled" />
      )}

      {dashboards.length > 1 && (
        <DashboardsPagination
          activeIndex={index}
          dotsLength={Number(dashboards.length)}
          onSetIndex={setIndex}
          isNavbarHidden={isNavbarHidden}
        />
      )}
    </Box>
  )
}
