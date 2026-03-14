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
})
