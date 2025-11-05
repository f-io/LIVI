import { useEffect, useState } from 'react'

export function useBelowNavTop() {
  const [top, setTop] = useState(0)
  useEffect(() => {
    const getTop = () => {
      const nav = document.querySelector('.MuiTabs-root') as HTMLElement | null
      const t = nav ? nav.getBoundingClientRect().bottom : 0
      setTop(Math.max(0, Math.round(t)))
    }
    getTop()
    const onResize = () => getTop()
    window.addEventListener('resize', onResize)
    const nav = document.querySelector('.MuiTabs-root') as HTMLElement | null
    let ro: ResizeObserver | null = null
    if (nav) {
      ro = new ResizeObserver(getTop)
      ro.observe(nav)
    }
    return () => {
      window.removeEventListener('resize', onResize)
      ro?.disconnect()
    }
  }, [])
  return top
}
