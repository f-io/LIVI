import { Box } from '@mui/material'
import type { PropsWithChildren } from 'react'
import * as React from 'react'

type DashShellProps = PropsWithChildren<{
  className?: string
  designWidth?: number
  designHeight?: number
}>

export function DashShell({
  children,
  className,
  designWidth = 1280,
  designHeight = 720
}: DashShellProps) {
  const ref = React.useRef<HTMLDivElement | null>(null)
  const [size, setSize] = React.useState({ w: 0, h: 0 })

  React.useEffect(() => {
    const el = ref.current
    if (!el) return

    const ro = new ResizeObserver(([entry]) => {
      const cr = entry?.contentRect
      if (!cr) return
      setSize({ w: cr.width, h: cr.height })
    })

    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const scale = size.w > 0 && size.h > 0 ? Math.min(size.w / designWidth, size.h / designHeight) : 1

  return (
    <Box
      ref={ref}
      className={className}
      sx={{
        width: '100%',
        height: '100%',
        minWidth: 0,
        minHeight: 0,
        overflow: 'hidden',
        '--dash-scale': String(scale)
      }}
    >
      {children}
    </Box>
  )
}
