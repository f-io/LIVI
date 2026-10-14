import type { Front } from '@shared/core/contract'
import { ROUTES } from '@shared/types'
import { useEffect, useRef } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { useLiviStore } from '../store/store'

const PAGE_OF: Partial<Record<Front, string>> = { projection: '/', camera: ROUTES.CAMERA }
const CORE_PAGES = Object.values(PAGE_OF)

/**
 * Core moves the main screen on its own for a call, Siri, reversing or the phone handing the
 * screen back. Afterwards the page the user was on comes back.
 */
export function useFollowFront(): void {
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const front = useLiviStore((s) => s.front?.main)
  const phoneActive = useLiviStore((s) => s.sessions.active !== null)
  const backPathRef = useRef<string | null>(null)
  const prevFrontRef = useRef(front)

  useEffect(() => {
    if (!CORE_PAGES.includes(pathname)) backPathRef.current = null
  }, [pathname])

  useEffect(() => {
    const prev = prevFrontRef.current
    prevFrontRef.current = front
    if (prev === undefined || prev === front) return
    const page = front && PAGE_OF[front]
    if (page) {
      if (pathname === page) return
      if (backPathRef.current === null) backPathRef.current = pathname
      else if (backPathRef.current === page) backPathRef.current = null
      navigate(page, { replace: true })
      return
    }
    if (pathname !== PAGE_OF[prev]) return
    const back = backPathRef.current
    if (prev === 'projection') {
      if (!phoneActive) return
      navigate(back ?? ROUTES.MEDIA, { replace: true })
    } else if (back) {
      // Only back from the camera core opened, one the user opened stays.
      navigate(back, { replace: true })
    }
    backPathRef.current = null
  }, [front, pathname, phoneActive, navigate])
}
