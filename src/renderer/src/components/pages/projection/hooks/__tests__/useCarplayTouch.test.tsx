import { renderHook } from '@testing-library/react'
import { createRef } from 'react'
import { useCarplayMultiTouch } from '../useCarplayTouch'
import { MultiTouchAction, TouchAction } from '@shared/types/ProjectionEnums'

describe('useCarplayMultiTouch', () => {
  const sendTouch = jest.fn()
  const sendMultiTouch = jest.fn()

  const createTarget = () => {
    const el = document.createElement('div')
    el.setPointerCapture = jest.fn()
    el.releasePointerCapture = jest.fn()
    Object.defineProperty(el, 'getBoundingClientRect', {
      value: () => ({
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
        width: 100,
        height: 100,
        x: 0,
        y: 0,
        toJSON: () => ({})
      })
    })
    return el
  }

  const ptrEvent = (target: HTMLElement, options: Partial<any>) =>
    ({
      currentTarget: target,
      pointerId: 1,
      pointerType: 'touch',
      clientX: 50,
      clientY: 50,
      ...options
    }) as React.PointerEvent<HTMLDivElement>

  beforeEach(() => {
    jest.clearAllMocks()
    ;(window as any).projection = {
      ipc: {
        sendTouch,
        sendMultiTouch
      }
    }
  })

  test('handles mouse touch sequence', () => {
    const target = createTarget()
    const videoRef = createRef<HTMLElement>()
    videoRef.current = target

    const { result } = renderHook(() => useCarplayMultiTouch(videoRef))

    result.current.onPointerDown(ptrEvent(target, { pointerType: 'mouse' }))
    expect(sendTouch).toHaveBeenCalledWith(0.5, 0.5, TouchAction.Down)

    result.current.onPointerMove(ptrEvent(target, { pointerType: 'mouse', clientX: 60 }))
    expect(sendTouch).toHaveBeenCalledWith(0.6, 0.5, TouchAction.Move)

    result.current.onPointerUp(ptrEvent(target, { pointerType: 'mouse', clientX: 70 }))
    expect(sendTouch).toHaveBeenCalledWith(0.7, 0.5, TouchAction.Up)
  })

  test('ignores mouse move/up when no active mouse down', () => {
    const target = createTarget()
    const videoRef = createRef<HTMLElement>()
    videoRef.current = target

    const { result } = renderHook(() => useCarplayMultiTouch(videoRef))

    result.current.onPointerMove(ptrEvent(target, { pointerType: 'mouse' }))
    result.current.onPointerUp(ptrEvent(target, { pointerType: 'mouse' }))

    expect(sendTouch).not.toHaveBeenCalled()
  })

  test('ignores events outside bounds', () => {
    const target = createTarget()
    const videoRef = createRef<HTMLElement>()
    videoRef.current = target

    const { result } = renderHook(() => useCarplayMultiTouch(videoRef))

    result.current.onPointerDown(ptrEvent(target, { clientX: 200, clientY: 200 }))
    result.current.onPointerMove(ptrEvent(target, { clientX: 200, clientY: 200 }))

    expect(sendTouch).not.toHaveBeenCalled()
    expect(sendMultiTouch).not.toHaveBeenCalled()
  })

  test('handles touch down/move/up with slot allocation and release', () => {
    const target = createTarget()
    const videoRef = createRef<HTMLElement>()
    videoRef.current = target

    const { result } = renderHook(() => useCarplayMultiTouch(videoRef))

    result.current.onPointerDown(ptrEvent(target, { pointerType: 'touch', pointerId: 11 }))
    expect(target.setPointerCapture).toHaveBeenCalledWith(11)
    expect(sendMultiTouch).toHaveBeenCalledWith([
      expect.objectContaining({ id: 0, action: MultiTouchAction.Down, x: 0.5, y: 0.5 })
    ])

    result.current.onPointerMove(
      ptrEvent(target, { pointerType: 'touch', pointerId: 11, clientX: 60 })
    )
    expect(sendMultiTouch).toHaveBeenCalledWith([
      expect.objectContaining({ id: 0, action: MultiTouchAction.Move, x: 0.6, y: 0.5 })
    ])

    result.current.onPointerUp(
      ptrEvent(target, { pointerType: 'touch', pointerId: 11, clientX: 80 })
    )
    expect(sendMultiTouch).toHaveBeenCalledWith([
      expect.objectContaining({ id: 0, action: MultiTouchAction.Up, x: 0.8, y: 0.5 })
    ])
    expect(target.releasePointerCapture).toHaveBeenCalledWith(11)
  })

  test('supports cancel/lost-capture and slot reuse', () => {
    const target = createTarget()
    const videoRef = createRef<HTMLElement>()
    videoRef.current = target

    const { result } = renderHook(() => useCarplayMultiTouch(videoRef))

    result.current.onPointerDown(ptrEvent(target, { pointerId: 1 }))
    result.current.onPointerCancel(ptrEvent(target, { pointerId: 1, clientX: 55 }))

    result.current.onPointerDown(ptrEvent(target, { pointerId: 2 }))
    result.current.onLostPointerCapture(ptrEvent(target, { pointerId: 2, clientX: 65 }))

    const ids = sendMultiTouch.mock.calls
      .flatMap((c) => c[0] as Array<{ id: number; action: MultiTouchAction }>)
      .filter((x) => x.action === MultiTouchAction.Down)
      .map((x) => x.id)

    expect(ids).toContain(0)
  })

  test('uses last known touch point when finish event is out of bounds', () => {
    const target = createTarget()
    const videoRef = createRef<HTMLElement>()
    videoRef.current = target

    const { result } = renderHook(() => useCarplayMultiTouch(videoRef))

    result.current.onPointerDown(ptrEvent(target, { pointerId: 12, clientX: 40, clientY: 40 }))
    result.current.onPointerOut(ptrEvent(target, { pointerId: 12 }))
    result.current.onPointerUp(ptrEvent(target, { pointerId: 12, clientX: 120, clientY: 120 }))

    expect(sendMultiTouch).toHaveBeenCalledWith([
      expect.objectContaining({ action: MultiTouchAction.Up, x: 0.4, y: 0.4 })
    ])
  })

  test('prevents context menu', () => {
    const target = createTarget()
    const videoRef = createRef<HTMLElement>()
    videoRef.current = target

    const { result } = renderHook(() => useCarplayMultiTouch(videoRef))
    const preventDefault = jest.fn()

    result.current.onContextMenu({ preventDefault } as unknown as React.MouseEvent<HTMLDivElement>)
    expect(preventDefault).toHaveBeenCalled()
  })
})
