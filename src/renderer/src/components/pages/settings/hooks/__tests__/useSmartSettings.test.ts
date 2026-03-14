import { act, renderHook } from '@testing-library/react'
import { useSmartSettings } from '../useSmartSettings'

const saveSettings = jest.fn()
const markRestartBaseline = jest.fn()
let isDongleConnected = true

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      saveSettings,
      restartBaseline: { width: 800, bindings: { back: 'KeyB' } },
      markRestartBaseline
    }),
  useStatusStore: (selector: (s: any) => unknown) => selector({ isDongleConnected })
}))

describe('useSmartSettings', () => {
  beforeEach(() => {
    saveSettings.mockReset()
    markRestartBaseline.mockReset()
    isDongleConnected = true
    ;(window as any).projection = { usb: { forceReset: jest.fn().mockResolvedValue(true) } }
  })

  test('handleFieldChange updates state and persists settings', () => {
    const initial = { width: 800, 'bindings.back': 'KeyB' } as any
    const settings = { width: 800, bindings: { back: 'KeyB' } } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))

    act(() => {
      result.current.handleFieldChange('width', 900)
    })

    expect(result.current.state.width).toBe(900)
    expect(saveSettings).toHaveBeenCalled()
    expect(result.current.isDirty).toBe(true)
  })

  test('requestRestart ignores bindings paths but marks relevant paths', () => {
    const initial = { width: 800, 'bindings.back': 'KeyB' } as any
    const settings = { width: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))

    act(() => result.current.requestRestart('bindings.back'))
    expect(result.current.needsRestart).toBe(false)

    act(() => result.current.requestRestart('width'))
    expect(result.current.needsRestart).toBe(true)
  })

  test('restart requires dongle connection and calls forceReset', async () => {
    const initial = { width: 800 } as any
    const settings = { width: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))
    act(() => result.current.requestRestart('width'))
    await act(async () => {
      await result.current.restart()
    })
    expect((window as any).projection.usb.forceReset).toHaveBeenCalled()
    expect(markRestartBaseline).toHaveBeenCalled()

    isDongleConnected = false
    const h2 = renderHook(() => useSmartSettings(initial, settings))
    await act(async () => {
      expect(await h2.result.current.restart()).toBe(false)
    })
  })
})
