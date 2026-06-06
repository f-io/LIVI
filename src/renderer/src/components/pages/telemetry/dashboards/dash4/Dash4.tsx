import { Box } from '@mui/material'
import { useStatusStore } from '@store/store'
import { useEffect } from 'react'

/**
 * Cluster stream only
 */
export function Dash4() {
  const setClusterDashActive = useStatusStore((s) => s.setClusterDashActive)
  useEffect(() => {
    setClusterDashActive(true)
    return () => setClusterDashActive(false)
  }, [setClusterDashActive])

  return <Box sx={{ width: '100%', height: '100%' }} />
}
