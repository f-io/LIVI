import { RefObject } from 'react'
import { FLASH_TIMEOUT_MS } from '../constants'

export function flash(ref: RefObject<HTMLButtonElement | null>, ms = FLASH_TIMEOUT_MS) {
  const el = ref.current
  if (!el) return
  const prevTransform = el.style.transform
  const prevShadow = el.style.boxShadow
  el.style.transform = 'scale(0.94)'
  el.style.boxShadow = '0 0 0 5px rgba(255,255,255,0.35) inset'
  window.setTimeout(() => {
    el.style.transform = prevTransform
    el.style.boxShadow = prevShadow
  }, ms)
}
