import { Box } from '@mui/material'
import type { PropsWithChildren } from 'react'
import * as React from 'react'

type DashboardShellProps = PropsWithChildren<{
  className?: string
  designWidth?: number
  designHeight?: number
}>

export function DashboardShell({
  children,
  className,
  designWidth = 1280,
  designHeight = 720
}: DashboardShellProps) {
  const [size, setSize] = React.useState({ w: window.innerWidth, h: window.innerHeight })

  React.useEffect(() => {
    let settle: ReturnType<typeof setTimeout> | null = null
    const onResize = () => {
      if (settle != null) clearTimeout(settle)
      settle = setTimeout(() => {
        settle = null
        setSize({ w: window.innerWidth, h: window.innerHeight })
      }, 150)
    }
    window.addEventListener('resize', onResize)
    return () => {
      if (settle != null) clearTimeout(settle)
      window.removeEventListener('resize', onResize)
    }
  }, [])

  const scale = size.w > 0 && size.h > 0 ? Math.min(size.w / designWidth, size.h / designHeight) : 1

  return (
    <Box
      className={className}
      sx={{
        width: '100%',
        height: '100%',
        minWidth: 0,
        minHeight: 0,
        overflow: 'hidden',
        WebkitFontSmoothing: 'antialiased',
        '--dashboard-scale': String(scale)
      }}
    >
      {children}
    </Box>
  )
}
