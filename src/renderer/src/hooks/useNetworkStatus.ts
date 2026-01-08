import { useEffect, useState } from 'react'

type NetworkStatus = {
  type: 'wifi' | 'cellular' | 'ethernet' | 'none' | 'unknown'
  effectiveType: string | null
  online: boolean
}

export function useNetworkStatus(): NetworkStatus {
  const read = (): NetworkStatus => {
    const online = typeof navigator !== 'undefined' ? navigator.onLine : true

    const c: any =
      (navigator as any).connection ||
      (navigator as any).mozConnection ||
      (navigator as any).webkitConnection

    // If the API is missing, we can only reliably know "online/offline".
    if (!c) {
      return { type: online ? 'unknown' : 'none', effectiveType: null, online }
    }

    const rawType = typeof c.type === 'string' ? c.type.toLowerCase() : ''
    const type =
      rawType === 'wifi'
        ? 'wifi'
        : rawType === 'cellular'
          ? 'cellular'
          : rawType === 'ethernet'
            ? 'ethernet'
            : online
              ? 'unknown'
              : 'none'

    const effectiveType = typeof c.effectiveType === 'string' ? c.effectiveType.toLowerCase() : null

    return { type, effectiveType, online }
  }

  const [network, setNetwork] = useState<NetworkStatus>(() => read())

  useEffect(() => {
    const c: any =
      (navigator as any).connection ||
      (navigator as any).mozConnection ||
      (navigator as any).webkitConnection

    const update = () => setNetwork(read())

    window.addEventListener('online', update)
    window.addEventListener('offline', update)

    if (c?.addEventListener) c.addEventListener('change', update)

    return () => {
      window.removeEventListener('online', update)
      window.removeEventListener('offline', update)
      if (c?.removeEventListener) c.removeEventListener('change', update)
    }
  }, [])

  return network
}
