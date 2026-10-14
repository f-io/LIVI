import { Box } from '@mui/material'
import { useStatusStore } from '@store/store'
import { useEffect } from 'react'

/**
 * Cluster stream only
 */
export function Dashboard4() {
  const setClusterDashboardActive = useStatusStore((s) => s.setClusterDashboardActive)
  useEffect(() => {
    setClusterDashboardActive(true)
    return () => setClusterDashboardActive(false)
  }, [setClusterDashboardActive])

  return <Box sx={{ width: '100%', height: '100%' }} />
}
