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

  test('restart returns false when needsRestart is false', async () => {
    // line 88: if (!needsRestart) return false
    const initial = { width: 800 } as any
    const settings = { width: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))
    // needsRestart is false (no requestRestart called, no baseline diff)
    await act(async () => {
      expect(await result.current.restart()).toBe(false)
    })
    expect((window as any).projection.usb.forceReset).not.toHaveBeenCalled()
  })

  test('needsRestartFromConfig detects when settings differ from restartBaseline', () => {
    // lines 44-53: restartBaseline[key] !== settings[key] for a restart-relevant key
    // The store mock has restartBaseline.width = 800, settings.width = 900 would differ
    const initial = { width: 900 } as any
    const settings = { width: 900 } as any
    // restartBaseline from mock has width: 800 → needsRestartFromConfig = true
    const { result } = renderHook(() => useSmartSettings(initial, settings))
    expect(result.current.needsRestart).toBe(true)
  })

  test('handleFieldChange with transform override applies transformation', () => {
    // lines 68-69: override?.transform is called
    const initial = { volume: 50 } as any
    const settings = { volume: 50 } as any
    const transform = jest.fn((v: unknown) => (v as number) * 2)
    const { result } = renderHook(() =>
      useSmartSettings(initial, settings, {
        overrides: { volume: { transform } }
      })
    )

    act(() => {
      result.current.handleFieldChange('volume', 10)
    })

    expect(transform).toHaveBeenCalledWith(10, 50)
    expect(result.current.state.volume).toBe(20)
  })

  test('handleFieldChange with validate override blocks invalid values', () => {
    // line 69: override?.validate returning false → no state update
    const initial = { volume: 50 } as any
    const settings = { volume: 50 } as any
    const validate = jest.fn(() => false) // always reject
    const { result } = renderHook(() =>
      useSmartSettings(initial, settings, {
        overrides: { volume: { validate } }
      })
    )

    act(() => {
      result.current.handleFieldChange('volume', 999)
    })

    expect(validate).toHaveBeenCalled()
    expect(result.current.state.volume).toBe(50) // unchanged
  })
})
