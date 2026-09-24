import { act, renderHook } from '@testing-library/react'
import { useSmartSettings } from '../useSmartSettings'

const saveSettings = vi.fn()
const markRestartBaseline = vi.fn()
let mockRestartBaseline: any = { projectionWidth: 800, bindings: { back: 'KeyB' } }

const { projectionActive } = vi.hoisted(() => ({ projectionActive: { value: true } }))

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      saveSettings,
      restartBaseline: mockRestartBaseline,
      markRestartBaseline
    }),
  useProjectionActive: () => projectionActive.value
}))

vi.mock('../../constants', () => ({
  requiresRestartParams: ['projectionWidth', 'bindings', 'carplayIcon180', 'wifiChannel'],
  restartWithoutSessionParams: ['wifiChannel']
}))

describe('useSmartSettings', () => {
  beforeEach(async () => {
    saveSettings.mockReset()
    markRestartBaseline.mockReset()
    mockRestartBaseline = { projectionWidth: 800, bindings: { back: 'KeyB' } }
    ;(window as any).projection = {
      ipc: { restart: vi.fn().mockResolvedValue(undefined) }
    }
  })

  test('handleFieldChange updates state and persists settings', async () => {
    const initial = { projectionWidth: 800, 'bindings.back': 'KeyB' } as any
    const settings = { projectionWidth: 800, bindings: { back: 'KeyB' } } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))

    act(() => {
      result.current.handleFieldChange('projectionWidth', 900)
    })

    expect(result.current.state.projectionWidth).toBe(900)
    expect(saveSettings).toHaveBeenCalled()
    expect(result.current.isDirty).toBe(true)
  })

  test('requestRestart ignores bindings paths but marks relevant paths', async () => {
    const initial = { projectionWidth: 800, 'bindings.back': 'KeyB' } as any
    const settings = { projectionWidth: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))

    act(() => result.current.requestRestart('bindings.back'))
    expect(result.current.needsRestart).toBe(false)

    act(() => result.current.requestRestart('projectionWidth'))
    expect(result.current.needsRestart).toBe(true)
  })

  test('restart fires the generic restart when a restart is needed', async () => {
    const initial = { projectionWidth: 800 } as any
    const settings = { projectionWidth: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))
    act(() => result.current.requestRestart('projectionWidth'))
    await act(async () => {
      await result.current.restart()
    })
    expect((window as any).projection.ipc.restart).toHaveBeenCalled()
    expect(markRestartBaseline).toHaveBeenCalled()
  })

  test('restart returns false when needsRestart is false', async () => {
    // line 88: if (!needsRestart) return false
    const initial = { projectionWidth: 800 } as any
    const settings = { projectionWidth: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))
    // needsRestart is false (no requestRestart called, no baseline diff)
    await act(async () => {
      expect(await result.current.restart()).toBe(false)
    })
    expect((window as any).projection.ipc.restart).not.toHaveBeenCalled()
  })

  test('needsRestartFromConfig detects when settings differ from restartBaseline', async () => {
    // lines 44-53: restartBaseline[key] !== settings[key] for a restart-relevant key
    // The store mock has restartBaseline.projectionWidth = 800, settings.projectionWidth = 900 would differ
    const initial = { projectionWidth: 900 } as any
    const settings = { projectionWidth: 900 } as any
    // restartBaseline from mock has projectionWidth: 800 → needsRestartFromConfig = true
    const { result } = renderHook(() => useSmartSettings(initial, settings))
    expect(result.current.needsRestart).toBe(true)
  })

  test('with nothing projecting only the access point keys still ask for a restart', async () => {
    mockRestartBaseline = { carplayIcon180: '', wifiChannel: 36 }
    projectionActive.value = false

    const icon = { carplayIcon180: 'b64', wifiChannel: 36 } as any
    const idle = renderHook(() => useSmartSettings(icon, icon))
    expect(idle.result.current.needsRestart).toBe(false)

    const wifi = { carplayIcon180: '', wifiChannel: 149 } as any
    const ap = renderHook(() => useSmartSettings(wifi, wifi))
    expect(ap.result.current.needsRestart).toBe(true)

    projectionActive.value = true
    const projecting = renderHook(() => useSmartSettings(icon, icon))
    expect(projecting.result.current.needsRestart).toBe(true)
  })

  test('handleFieldChange with transform override applies transformation', async () => {
    // lines 68-69: override?.transform is called
    const initial = { volume: 50 } as any
    const settings = { volume: 50 } as any
    const transform = vi.fn((v: unknown) => (v as number) * 2)
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

  test('handleFieldChange with validate override blocks invalid values', async () => {
    // line 69: override?.validate returning false → no state update
    const initial = { volume: 50 } as any
    const settings = { volume: 50 } as any
    const validate = vi.fn(() => false) // always reject
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

  test('requestRestart with no path treats it as restart-relevant', async () => {
    const initial = { projectionWidth: 800 } as any
    const settings = { projectionWidth: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))

    act(() => result.current.requestRestart())
    expect(result.current.needsRestart).toBe(true)
  })

  test('needsRestartFromConfig skips bindings keys and tolerates nullish settings and baseline', async () => {
    mockRestartBaseline = null
    const initial = {} as any
    const { result } = renderHook(() => useSmartSettings(initial, null as any))
    expect(result.current.needsRestart).toBe(false)
  })

  test('handleFieldChange clones an empty object when settings is nullish', async () => {
    const initial = { projectionWidth: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, null as any))

    act(() => {
      result.current.handleFieldChange('projectionWidth', 640)
    })

    expect(result.current.state.projectionWidth).toBe(640)
    expect(saveSettings).toHaveBeenCalledWith({ projectionWidth: 640 })
  })

  test('resetState restores the provided initial state', async () => {
    const initial = { projectionWidth: 800 } as any
    const settings = { projectionWidth: 800 } as any
    const { result } = renderHook(() => useSmartSettings(initial, settings))

    act(() => {
      result.current.handleFieldChange('projectionWidth', 900)
    })
    expect(result.current.state.projectionWidth).toBe(900)

    act(() => {
      result.current.resetState()
    })
    expect(result.current.state.projectionWidth).toBe(800)
  })
})
