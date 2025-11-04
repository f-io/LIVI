import { useMemo, useRef, useCallback } from 'react'
import { MultiTouchAction, TouchAction } from '@main/carplay/messages/sendable'

type Handlers = {
  onPointerDown: React.PointerEventHandler<HTMLDivElement>
  onPointerMove: React.PointerEventHandler<HTMLDivElement>
  onPointerUp: React.PointerEventHandler<HTMLDivElement>
  onPointerCancel: React.PointerEventHandler<HTMLDivElement>
  onPointerOut: React.PointerEventHandler<HTMLDivElement>
  onLostPointerCapture: React.PointerEventHandler<HTMLDivElement>
  onContextMenu: React.MouseEventHandler<HTMLDivElement>
}

type MTPoint = { id: number; x: number; y: number; action: MultiTouchAction }

const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v)
const norm = (el: HTMLElement, cx: number, cy: number) => {
  const r = el.getBoundingClientRect()
  return { x: clamp01((cx - r.left) / r.width), y: clamp01((cy - r.top) / r.height) }
}

export const useCarplayMultiTouch = (): Handlers => {
  const slotByPointerId = useRef(new Map<number, number>())
  const active = useRef(new Map<number, { x: number; y: number }>())
  const freeSlots = useRef<number[]>([])
  const nextSlot = useRef(0)
  const mouseDown = useRef(false)

  const alloc = useCallback((pid: number) => {
    const old = slotByPointerId.current.get(pid)
    if (old !== undefined) return old
    const reuse = freeSlots.current.pop()
    const slot = reuse ?? nextSlot.current++
    slotByPointerId.current.set(pid, slot)
    return slot
  }, [])

  const free = useCallback((pid: number) => {
    const slot = slotByPointerId.current.get(pid)
    if (slot !== undefined) {
      slotByPointerId.current.delete(pid)
      active.current.delete(slot)
      freeSlots.current.push(slot)
    }
  }, [])

  const sendFullFrame = useCallback((overrides?: Map<number, MultiTouchAction>) => {
    const pts: MTPoint[] = []
    active.current.forEach((pos, id) => {
      const action = overrides?.get(id) ?? MultiTouchAction.Move
      pts.push({ id, x: pos.x, y: pos.y, action })
    })
    if (!pts.length && overrides && overrides.size) {
      overrides.forEach((action, id) => {
        const pos = active.current.get(id)
        if (pos) pts.push({ id, x: pos.x, y: pos.y, action })
      })
    }
    if (!pts.length) return
    window.carplay.ipc.sendMultiTouch(pts)
  }, [])

  const onPointerDown = useCallback<Handlers['onPointerDown']>(
    (e) => {
      const el = e.currentTarget as HTMLElement
      const { x, y } = norm(el, e.clientX, e.clientY)

      if (e.pointerType === 'mouse') {
        mouseDown.current = true
        window.carplay.ipc.sendTouch(x, y, TouchAction.Down)
        return
      }

      el.setPointerCapture?.(e.pointerId)
      const id = alloc(e.pointerId)
      active.current.set(id, { x, y })
      const overrides = new Map<number, MultiTouchAction>()
      overrides.set(id, MultiTouchAction.Down)
      sendFullFrame(overrides)
    },
    [alloc, sendFullFrame]
  )

  const onPointerMove = useCallback<Handlers['onPointerMove']>(
    (e) => {
      const el = e.currentTarget as HTMLElement
      const { x, y } = norm(el, e.clientX, e.clientY)

      if (e.pointerType === 'mouse') {
        if (!mouseDown.current) return
        window.carplay.ipc.sendTouch(x, y, TouchAction.Move)
        return
      }

      const id = slotByPointerId.current.get(e.pointerId)
      if (id === undefined) return
      active.current.set(id, { x, y })
      sendFullFrame()
    },
    [sendFullFrame]
  )

  const finishPointer = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const el = e.currentTarget as HTMLElement
      const { x, y } = norm(el, e.clientX, e.clientY)

      if (e.pointerType === 'mouse') {
        if (!mouseDown.current) return
        mouseDown.current = false
        window.carplay.ipc.sendTouch(x, y, TouchAction.Up)
        return
      }

      const id = slotByPointerId.current.get(e.pointerId)
      if (id !== undefined) {
        active.current.set(id, { x, y })
        const overrides = new Map<number, MultiTouchAction>()
        overrides.set(id, MultiTouchAction.Up)
        sendFullFrame(overrides)

        el.releasePointerCapture?.(e.pointerId)
        free(e.pointerId)
      }
    },
    [free, sendFullFrame]
  )

  const onPointerUp = useCallback<Handlers['onPointerUp']>((e) => finishPointer(e), [finishPointer])
  const onPointerCancel = useCallback<Handlers['onPointerCancel']>(
    (e) => finishPointer(e),
    [finishPointer]
  )
  const onLostPointerCapture = useCallback<Handlers['onLostPointerCapture']>(
    (e) => finishPointer(e),
    [finishPointer]
  )

  const onPointerOut = useCallback<Handlers['onPointerOut']>(() => {}, [])
  const onContextMenu = useCallback<Handlers['onContextMenu']>((e) => e.preventDefault(), [])

  return useMemo(
    () => ({
      onPointerDown,
      onPointerMove,
      onPointerUp,
      onPointerCancel,
      onPointerOut,
      onLostPointerCapture,
      onContextMenu
    }),
    [
      onPointerDown,
      onPointerMove,
      onPointerUp,
      onPointerCancel,
      onPointerOut,
      onLostPointerCapture,
      onContextMenu
    ]
  )
}
