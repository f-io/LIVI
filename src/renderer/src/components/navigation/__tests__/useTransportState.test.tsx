import { act, renderHook } from '@testing-library/react'
import { useTransportState } from '../useTransportState'

type Handler = (...args: unknown[]) => void

function installProjection(
  over: { getState?: jest.Mock; onEvent?: jest.Mock; offEvent?: jest.Mock } = {}
) {
  const ipc = {
    getTransportState: over.getState ?? jest.fn(async () => null),
    onEvent: over.onEvent ?? jest.fn(),
    offEvent: over.offEvent ?? jest.fn()
  }
  ;(window as unknown as { projection: { ipc: typeof ipc } }).projection = { ipc }
  return ipc
}

beforeEach(() => {
  delete (window as unknown as { projection?: unknown }).projection
})

describe('useTransportState', () => {
  test('initial state is the static INITIAL constant', () => {
    installProjection()
    const { result } = renderHook(() => useTransportState())
    expect(result.current).toEqual({
      active: null,
      dongleDetected: false,
      nativeDetected: false,
      preference: 'auto'
    })
  })

  test('no-op when window.projection is missing', () => {
    const { result } = renderHook(() => useTransportState())
    expect(result.current.active).toBeNull()
  })

  test('seeds state from the initial getTransportState resolve', async () => {
    const initial = {
      active: 'aa' as const,
      dongleDetected: false,
      nativeDetected: true,
      preference: 'native' as const
    }
    installProjection({ getState: jest.fn(async () => initial) })
    const { result } = renderHook(() => useTransportState())
    // Wait for promise microtask
    await act(async () => {
      await Promise.resolve()
    })
    expect(result.current).toEqual(initial)
  })

  test('handles getTransportState rejection silently', async () => {
    installProjection({ getState: jest.fn(async () => Promise.reject(new Error('nope'))) })
    const { result } = renderHook(() => useTransportState())
    await act(async () => {
      await Promise.resolve()
    })
    expect(result.current.active).toBeNull()
  })

  test('updates state on a "transportState" IPC event', () => {
    let captured: Handler | null = null
    const onEvent = jest.fn((h: Handler) => {
      captured = h
    })
    installProjection({ onEvent })
    const { result } = renderHook(() => useTransportState())
    expect(onEvent).toHaveBeenCalled()

    const payload = {
      active: 'dongle' as const,
      dongleDetected: true,
      nativeDetected: false,
      preference: 'dongle' as const
    }
    act(() => {
      captured!({}, { type: 'transportState', payload })
    })
    expect(result.current).toEqual(payload)
  })

  test('ignores IPC events of unrelated type', () => {
    let captured: Handler | null = null
    installProjection({
      onEvent: jest.fn((h: Handler) => {
        captured = h
      })
    })
    const { result } = renderHook(() => useTransportState())
    act(() => {
      captured!({}, { type: 'somethingElse', payload: { active: 'aa' } })
    })
    expect(result.current.active).toBeNull()
  })

  test('unmount calls offEvent with the same handler', () => {
    let captured: Handler | null = null
    const offEvent = jest.fn()
    installProjection({
      onEvent: jest.fn((h: Handler) => {
        captured = h
      }),
      offEvent
    })
    const { unmount } = renderHook(() => useTransportState())
    unmount()
    expect(offEvent).toHaveBeenCalledWith(captured)
  })

  test('survives missing offEvent on unmount', () => {
    const ipc = {
      getTransportState: jest.fn(async () => null),
      onEvent: jest.fn()
    }
    ;(window as unknown as { projection: { ipc: typeof ipc } }).projection = { ipc }
    const { unmount } = renderHook(() => useTransportState())
    expect(() => unmount()).not.toThrow()
  })
})
