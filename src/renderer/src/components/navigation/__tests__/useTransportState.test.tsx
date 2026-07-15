import { act, renderHook } from '@testing-library/react'
import type { Mock } from 'vitest'
import { useTransportState } from '../useTransportState'

type Handler = (...args: unknown[]) => void

function installProjection(over: { getState?: Mock; onEvent?: Mock; offEvent?: Mock } = {}) {
  const ipc = {
    getTransportState: over.getState ?? vi.fn(async () => null),
    onEvent: over.onEvent ?? vi.fn(),
    offEvent: over.offEvent ?? vi.fn()
  }
  ;(window as unknown as { projection: { ipc: typeof ipc } }).projection = { ipc }
  return ipc
}

beforeEach(async () => {
  delete (window as unknown as { projection?: unknown }).projection
})

describe('useTransportState', () => {
  test('initial state is the static INITIAL constant', async () => {
    installProjection()
    const { result } = renderHook(() => useTransportState())
    expect(result.current).toEqual({
      active: null,
      targetTransport: null,
      targetMode: null,
      switchPending: false,
      dongleDetected: false,
      wiredPhoneDetected: false,
      wirelessPhoneDetected: false,
      wirelessPhoneActive: false,
      wiredPhoneActive: false
    })
  })

  test('no-op when window.projection is missing', async () => {
    const { result } = renderHook(() => useTransportState())
    expect(result.current.active).toBeNull()
  })

  test('seeds state from the initial getTransportState resolve', async () => {
    const initial = {
      active: 'aa' as const,
      dongleDetected: false,
      wiredPhoneDetected: false,
      wirelessPhoneActive: true,
      wiredPhoneActive: false
    }
    installProjection({ getState: vi.fn(async () => initial) })
    const { result } = renderHook(() => useTransportState())
    // Wait for promise microtask
    await act(async () => {
      await Promise.resolve()
    })
    expect(result.current).toEqual(initial)
  })

  test('handles getTransportState rejection silently', async () => {
    installProjection({ getState: vi.fn(async () => Promise.reject(new Error('nope'))) })
    const { result } = renderHook(() => useTransportState())
    await act(async () => {
      await Promise.resolve()
    })
    expect(result.current.active).toBeNull()
  })

  test('updates state on a "transportState" IPC event', () => {
    let captured: Handler | null = null
    const onEvent = vi.fn((h: Handler) => {
      captured = h
    })
    installProjection({ onEvent })
    const { result } = renderHook(() => useTransportState())
    expect(onEvent).toHaveBeenCalled()

    const payload = {
      active: 'dongle' as const,
      dongleDetected: true,
      wiredPhoneDetected: false,
      wirelessPhoneActive: false,
      wiredPhoneActive: false
    }
    act(() => {
      captured!({}, { type: 'transportState', payload })
    })
    expect(result.current).toEqual(payload)
  })

  test('ignores IPC events of unrelated type', async () => {
    let captured: Handler | null = null
    installProjection({
      onEvent: vi.fn((h: Handler) => {
        captured = h
      })
    })
    const { result } = renderHook(() => useTransportState())
    act(() => {
      captured!({}, { type: 'somethingElse', payload: { active: 'aa' } })
    })
    expect(result.current.active).toBeNull()
  })

  test('unmount calls offEvent with the same handler', async () => {
    let captured: Handler | null = null
    const offEvent = vi.fn()
    installProjection({
      onEvent: vi.fn((h: Handler) => {
        captured = h
      }),
      offEvent
    })
    const { unmount } = renderHook(() => useTransportState())
    unmount()
    expect(offEvent).toHaveBeenCalledWith(captured)
  })

  test('survives missing offEvent on unmount', async () => {
    const ipc = {
      getTransportState: vi.fn(async () => null),
      onEvent: vi.fn()
    }
    ;(window as unknown as { projection: { ipc: typeof ipc } }).projection = { ipc }
    const { unmount } = renderHook(() => useTransportState())
    expect(() => unmount()).not.toThrow()
  })
})
