import { act, renderHook } from '@testing-library/react'
import { useNetworkStatus } from '../useNetworkStatus'

describe('useNetworkStatus', () => {
  const originalNavigator = global.navigator

  afterEach(() => {
    Object.defineProperty(global, 'navigator', { configurable: true, value: originalNavigator })
  })

  test('falls back to online/offline when connection api is absent', () => {
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: { onLine: true }
    })

    const { result } = renderHook(() => useNetworkStatus())
    expect(result.current).toEqual({ type: 'unknown', effectiveType: null, online: true })
  })

  test('reads connection type and reacts to online/offline events', () => {
    let changeCb: (() => void) | undefined
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        onLine: true,
        connection: {
          type: 'wifi',
          effectiveType: '4g',
          addEventListener: (_: string, cb: () => void) => {
            changeCb = cb
          },
          removeEventListener: jest.fn()
        }
      }
    })

    const { result } = renderHook(() => useNetworkStatus())
    expect(result.current.type).toBe('wifi')
    expect(result.current.effectiveType).toBe('4g')

    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        onLine: false,
        connection: {
          type: 'wifi',
          effectiveType: '3g',
          addEventListener: (_: string, cb: () => void) => {
            changeCb = cb
          },
          removeEventListener: jest.fn()
        }
      }
    })

    act(() => {
      window.dispatchEvent(new Event('offline'))
      changeCb?.()
    })

    expect(result.current.online).toBe(false)
    expect(result.current.type).toBe('wifi')
  })

  test('returns none when offline and connection api is absent', () => {
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: { onLine: false }
    })

    const { result } = renderHook(() => useNetworkStatus())

    expect(result.current).toEqual({ type: 'none', effectiveType: null, online: false })
  })

  test('maps ethernet type and unregisters listeners on unmount', () => {
    const removeConnectionListener = jest.fn()

    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        onLine: true,
        connection: {
          type: 'ethernet',
          effectiveType: '5g',
          addEventListener: jest.fn(),
          removeEventListener: removeConnectionListener
        }
      }
    })

    const removeWindowListenerSpy = jest.spyOn(window, 'removeEventListener')

    const { result, unmount } = renderHook(() => useNetworkStatus())

    expect(result.current).toEqual({
      type: 'ethernet',
      effectiveType: '5g',
      online: true
    })

    unmount()

    expect(removeConnectionListener).toHaveBeenCalledWith('change', expect.any(Function))
    expect(removeWindowListenerSpy).toHaveBeenCalledWith('online', expect.any(Function))
    expect(removeWindowListenerSpy).toHaveBeenCalledWith('offline', expect.any(Function))

    removeWindowListenerSpy.mockRestore()
  })

  test('falls back to unknown and null effectiveType when connection values are not strings', () => {
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        onLine: true,
        connection: {
          type: 123,
          effectiveType: 456,
          addEventListener: jest.fn(),
          removeEventListener: jest.fn()
        }
      }
    })

    const { result } = renderHook(() => useNetworkStatus())

    expect(result.current).toEqual({
      type: 'unknown',
      effectiveType: null,
      online: true
    })
  })

  test('maps cellular type and lowercases effectiveType', () => {
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        onLine: true,
        connection: {
          type: 'cellular',
          effectiveType: '4G',
          addEventListener: jest.fn(),
          removeEventListener: jest.fn()
        }
      }
    })

    const { result } = renderHook(() => useNetworkStatus())

    expect(result.current).toEqual({
      type: 'cellular',
      effectiveType: '4g',
      online: true
    })
  })

  test('returns none when connection exists but type is unknown and browser is offline', () => {
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        onLine: false,
        connection: {
          type: 'bluetooth',
          effectiveType: '3G',
          addEventListener: jest.fn(),
          removeEventListener: jest.fn()
        }
      }
    })

    const { result } = renderHook(() => useNetworkStatus())

    expect(result.current).toEqual({
      type: 'none',
      effectiveType: '3g',
      online: false
    })
  })

  test('defaults to online true when navigator is undefined', () => {
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: undefined
    })

    const { result } = renderHook(() => useNetworkStatus())

    expect(result.current).toEqual({
      type: 'unknown',
      effectiveType: null,
      online: true
    })
  })
})
