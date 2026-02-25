import { useEffect, useState } from 'react'

type NetworkStatus = {
  type: 'wifi' | 'cellular' | 'ethernet' | 'none' | 'unknown'
  effectiveType: string | null
  online: boolean
}

type NetworkInformationLike = {
  type?: unknown
  effectiveType?: unknown
  addEventListener?: (type: 'change', listener: () => void) => void
  removeEventListener?: (type: 'change', listener: () => void) => void
}

function getConnection(): NetworkInformationLike | null {
  const nav = navigator as Navigator & {
    connection?: NetworkInformationLike
    mozConnection?: NetworkInformationLike
    webkitConnection?: NetworkInformationLike
  }

  return nav.connection ?? nav.mozConnection ?? nav.webkitConnection ?? null
}

export function useNetworkStatus(): NetworkStatus {
  const read = (): NetworkStatus => {
    const online = typeof navigator !== 'undefined' ? navigator.onLine : true
    const c = getConnection()

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
    const c = getConnection()
    const update = () => setNetwork(read())

    window.addEventListener('online', update)
    window.addEventListener('offline', update)

    c?.addEventListener?.('change', update)

    return () => {
      window.removeEventListener('online', update)
      window.removeEventListener('offline', update)
      c?.removeEventListener?.('change', update)
    }
  }, [])

  return network
}
