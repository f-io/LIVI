import { Box, useTheme } from '@mui/material'
import * as React from 'react'
import { DashboardsPaginationProps } from './types'
import { FC } from 'react'
import { usePaginationDots } from '@renderer/components/pages/telemetry/hooks/usePaginationDots'

export const DashboardsPagination: FC<DashboardsPaginationProps> = ({activeIndex, dotsLength, onSetIndex, isNavbarHidden}) => {
  const theme = useTheme()

  const {
    showDots,
    revealDots
  } = usePaginationDots(isNavbarHidden)

  return (
    <Box
      sx={{
        position: 'absolute',
        left: 0,
        right: 0,
        bottom: 'clamp(10px, 2.2svh, 18px)',
        display: 'flex',
        justifyContent: 'center',
        pointerEvents: 'auto',
        opacity: showDots ? 1 : 0,
        transition: 'opacity 180ms ease-out'
      }}
    >
      <Box
        sx={{
          display: 'flex',
          gap: 'clamp(6px, 1.2svh, 10px)',
          px: 'clamp(10px, 2svh, 14px)',
          py: 'clamp(6px, 1.4svh, 10px)',
          borderRadius: 999,
          backgroundColor:
            theme.palette.mode === 'dark' ? 'rgba(0,0,0,0.35)' : 'rgba(0,0,0,0.18)',
          backdropFilter: 'blur(6px)'
        }}
      >
        {Array(dotsLength).fill(null).map((_, i) => (
          <Box
            key={i}
            role="button"
            tabIndex={0}
            onPointerDown={(e) => {
              e.preventDefault()
              e.stopPropagation()
              onSetIndex(i)
              revealDots()
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                e.stopPropagation()
                onSetIndex(i)
                revealDots()
              }
            }}
            sx={{
              width: 'clamp(14px, 2.6svh, 22px)',
              height: 'clamp(14px, 2.6svh, 22px)',
              display: 'grid',
              placeItems: 'center',
              borderRadius: 999,
              cursor: 'pointer',
              WebkitTapHighlightColor: 'transparent'
            }}
          >
            <Box
              sx={{
                width: 'clamp(6px, 1.2svh, 10px)',
                height: 'clamp(6px, 1.2svh, 10px)',
                borderRadius: 999,
                backgroundColor:
                  i === activeIndex ? theme.palette.primary.main : theme.palette.text.secondary,
                opacity: i === activeIndex ? 1 : 0.45,
                transition: 'opacity 120ms ease-out'
              }}
            />
          </Box>
        ))}
      </Box>
    </Box>
  )
}
