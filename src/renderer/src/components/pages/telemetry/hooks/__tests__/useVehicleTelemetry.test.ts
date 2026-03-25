import { act, renderHook, waitFor } from '@testing-library/react'
import { useVehicleTelemetry } from '../useVehicleTelemetry'

describe('useVehicleTelemetry', () => {
  let onTelemetryCb: ((payload: unknown) => void) | undefined
  const onTelemetryMock = jest.fn()
  const offTelemetryMock = jest.fn()

  beforeEach(() => {
    jest.clearAllMocks()
    onTelemetryCb = undefined
    ;(window as any).projection = {
      ipc: {
        onTelemetry: jest.fn((cb: (payload: unknown) => void) => {
          onTelemetryCb = cb
          onTelemetryMock(cb)
        }),
        offTelemetry: jest.fn((cb: (payload: unknown) => void) => {
          offTelemetryMock(cb)
        })
      }
    }
  })

  test('subscribes to telemetry on mount', () => {
    renderHook(() => useVehicleTelemetry())

    expect((window as any).projection.ipc.onTelemetry).toHaveBeenCalledTimes(1)
    expect(onTelemetryCb).toBeDefined()
  })

  test('unsubscribes from telemetry on unmount', () => {
    const { unmount } = renderHook(() => useVehicleTelemetry())

    const cb = onTelemetryCb
    unmount()

    expect((window as any).projection.ipc.offTelemetry).toHaveBeenCalledTimes(1)
    expect((window as any).projection.ipc.offTelemetry).toHaveBeenCalledWith(cb)
  })

  test('starts with null telemetry and stale state', () => {
    const { result } = renderHook(() => useVehicleTelemetry())

    expect(result.current.telemetry).toBeNull()
    expect(result.current.isStale).toBe(true)
  })

  test('ignores non-object telemetry payloads', () => {
    const { result } = renderHook(() => useVehicleTelemetry())

    act(() => {
      onTelemetryCb?.('invalid')
      onTelemetryCb?.(null)
      onTelemetryCb?.(123)
    })

    expect(result.current.telemetry).toBeNull()
    expect(result.current.isStale).toBe(true)
  })

  test('stores telemetry payload with explicit timestamp', async () => {
    const { result } = renderHook(() => useVehicleTelemetry())

    act(() => {
      onTelemetryCb?.({
        speedKph: 120,
        rpm: 3500,
        ts: Date.now()
      })
    })

    await waitFor(() => {
      expect(result.current.telemetry).toMatchObject({
        speedKph: 120,
        rpm: 3500
      })
    })

    expect(typeof result.current.telemetry?.ts).toBe('number')
    expect(result.current.isStale).toBe(false)
  })

  test('fills missing timestamp with Date.now', async () => {
    const nowSpy = jest.spyOn(Date, 'now').mockReturnValue(123456789)

    const { result } = renderHook(() => useVehicleTelemetry())

    act(() => {
      onTelemetryCb?.({
        speedKph: 88
      })
    })

    await waitFor(() => {
      expect(result.current.telemetry).toMatchObject({
        speedKph: 88,
        ts: 123456789
      })
    })

    nowSpy.mockRestore()
  })

  test('merges new telemetry payload into previous telemetry', async () => {
    const { result } = renderHook(() => useVehicleTelemetry())

    act(() => {
      onTelemetryCb?.({
        speedKph: 90,
        rpm: 2000,
        ts: Date.now()
      })
    })

    await waitFor(() => {
      expect(result.current.telemetry).toMatchObject({
        speedKph: 90,
        rpm: 2000
      })
    })

    act(() => {
      onTelemetryCb?.({
        fuelPct: 55,
        ts: Date.now()
      })
    })

    await waitFor(() => {
      expect(result.current.telemetry).toMatchObject({
        speedKph: 90,
        rpm: 2000,
        fuelPct: 55
      })
    })
  })

  test('reports stale when telemetry timestamp is older than 1500ms', async () => {
    const nowSpy = jest.spyOn(Date, 'now').mockReturnValue(5000)

    const { result } = renderHook(() => useVehicleTelemetry())

    act(() => {
      onTelemetryCb?.({
        speedKph: 100,
        ts: 3000
      })
    })

    await waitFor(() => {
      expect(result.current.telemetry).toMatchObject({
        speedKph: 100,
        ts: 3000
      })
    })

    expect(result.current.isStale).toBe(true)

    nowSpy.mockRestore()
  })

  test('reports not stale when telemetry timestamp is recent', async () => {
    const nowSpy = jest.spyOn(Date, 'now').mockReturnValue(5000)

    const { result } = renderHook(() => useVehicleTelemetry())

    act(() => {
      onTelemetryCb?.({
        speedKph: 100,
        ts: 4000
      })
    })

    await waitFor(() => {
      expect(result.current.telemetry).toMatchObject({
        speedKph: 100,
        ts: 4000
      })
    })

    expect(result.current.isStale).toBe(false)

    nowSpy.mockRestore()
  })
})
