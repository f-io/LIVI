import * as React from 'react'
import { Box, Typography, useTheme } from '@mui/material'

export const DashPlaceholder = ({ title }: { title: string }) => {
  const theme = useTheme()
  const [navHidden, setNavHidden] = React.useState(false)

  React.useEffect(() => {
    const el = document.getElementById('content-root')
    if (!el) return

    const read = () => setNavHidden(el.getAttribute('data-nav-hidden') === '1')
    read()

    const mo = new MutationObserver(read)
    mo.observe(el, { attributes: true, attributeFilter: ['data-nav-hidden'] })

    return () => mo.disconnect()
  }, [])

  return (
    <Box
      sx={{
        position: navHidden ? 'fixed' : 'absolute',
        inset: 0,
        display: 'grid',
        placeItems: 'center',
        backgroundColor: theme.palette.background.default
      }}
    >
      <Typography sx={{ fontSize: 'clamp(1.2rem, 4svh, 2.2rem)', opacity: 0.85 }}>
        {title}
      </Typography>
    </Box>
  )
}
