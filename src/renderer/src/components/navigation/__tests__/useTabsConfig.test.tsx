import { renderHook, waitFor } from '@testing-library/react'
import { UI } from '../../../constants'
import { useTabsConfig } from '../useTabsConfig'

let mockRole = 'main'

let mockState = {
  isStreaming: false,
  isDongleHardwarePresent: false,
  activeProtocol: null as 'carplay' | 'androidauto' | null,
  cameraFound: true,
  telemetryOnMain: false,
  settingsMissing: false,
  mainMedia: true,
  secondaryTelemetry: false,
  secondaryMedia: false,
  secondaryCamera: false,
  secondaryAbsentKeys: false,
  mainCustom: false,
  secondaryCustom: false
}

vi.mock('@mui/material/styles', () => ({
  useTheme: () => ({
    palette: {
      text: { primary: '#fff', disabled: '#777' }
    }
  })
}))

vi.mock('../../../utils/windowRole', () => ({
  getWindowRole: () => mockRole
}))

vi.mock('@store/store', () => ({
  useStatusStore: (selector: (s: any) => unknown) =>
    selector({
      isStreaming: mockState.isStreaming,
      activeProtocol: mockState.activeProtocol,
      cameraFound: mockState.cameraFound
    }),
  useProjectionActive: () => mockState.isDongleHardwarePresent || mockState.activeProtocol != null,
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: mockState.settingsMissing
        ? undefined
        : {
            camera: mockState.secondaryAbsentKeys
              ? { main: true }
              : { main: true, dash: mockState.secondaryCamera, aux: mockState.secondaryCamera },
            custom: mockState.secondaryAbsentKeys
              ? undefined
              : { main: mockState.mainCustom, dash: mockState.secondaryCustom, aux: false },
            media: mockState.secondaryAbsentKeys
              ? { main: true }
              : {
                  main: mockState.mainMedia,
                  dash: mockState.secondaryMedia,
                  aux: mockState.secondaryMedia
                },
            dashboards: {
              dash1: {
                main: mockState.telemetryOnMain,
                dash: mockState.secondaryTelemetry,
                aux: mockState.secondaryTelemetry,
                pos: 1
              },
              dash2: { main: false, dash: false, aux: false, pos: 2 },
              dash3: { main: false, dash: false, aux: false, pos: 3 },
              dash4: { main: false, dash: false, aux: false, pos: 4 }
            }
          }
    })
}))

describe('useTabsConfig', () => {
  beforeEach(() => {
    mockRole = 'main'
    mockState = {
      isStreaming: false,
      isDongleHardwarePresent: false,
      activeProtocol: null,
      cameraFound: true,
      telemetryOnMain: false,
      settingsMissing: false,
      mainMedia: true,
      secondaryTelemetry: false,
      secondaryMedia: false,
      secondaryCamera: false,
      secondaryAbsentKeys: false,
      mainCustom: false,
      secondaryCustom: false
    }
  })

  test('returns base tabs by default', () => {
    const { result } = renderHook(() => useTabsConfig(false))
    expect(result.current.map((t) => t.path)).toEqual(['/', '/media', '/camera', '/settings'])
  })

  test('adds the telemetry tab when a dashboard is routed to main', () => {
    mockState.telemetryOnMain = true
    const { result } = renderHook(() => useTabsConfig(false))
    expect(result.current.map((t) => t.path)).toEqual([
      '/',
      '/telemetry',
      '/media',
      '/camera',
      '/settings'
    ])
  })

  test('hides the media tab when media is not routed to main', () => {
    mockState.mainMedia = false
    const { result } = renderHook(() => useTabsConfig(false))
    expect(result.current.map((t) => t.path)).toEqual(['/', '/camera', '/settings'])
  })

  test('adds the custom tab above settings when routed to main', () => {
    mockState.mainCustom = true
    const { result } = renderHook(() => useTabsConfig(false))
    expect(result.current.map((t) => t.path)).toEqual([
      '/',
      '/media',
      '/camera',
      '/custom',
      '/settings'
    ])
    expect(result.current.find((t) => t.path === '/custom')?.label).toBe('Custom')
  })

  test('a secondary window shows the custom tab when routed there', () => {
    mockRole = 'dash'
    mockState.secondaryCustom = true
    const { result } = renderHook(() => useTabsConfig(false))
    expect(result.current.map((t) => t.path)).toContain('/custom')
  })

  test('hides camera tab when camera is not found', () => {
    mockState.cameraFound = false
    const { result } = renderHook(() => useTabsConfig(false))
    const camera = result.current.find((t) => t.path === '/camera')
    expect(camera).toBeUndefined()
  })

  test('returns active CarPlay icon variant when dongle is connected', () => {
    mockState.isDongleHardwarePresent = true

    const { result } = renderHook(() => useTabsConfig(false))
    const carPlayTab = result.current.find((t) => t.path === '/')

    expect(carPlayTab).toBeDefined()
    expect((carPlayTab!.icon as any).props.sx).toEqual(
      expect.objectContaining({
        fontSize: 32,
        color: '#fff',
        opacity: 'var(--ui-breathe-opacity, 1)'
      })
    )
    expect((carPlayTab!.icon as any).props.sx['&, &.MuiSvgIcon-root']).toEqual({
      color: '#fff !important'
    })
  })

  test('falls back to no telemetry tab when settings are missing', () => {
    mockState.settingsMissing = true

    const { result } = renderHook(() => useTabsConfig(false))

    expect(result.current.map((t) => t.path)).toEqual(['/', '/media', '/camera', '/settings'])
  })

  test('uses highlighted CarPlay icon styling when streaming is active regardless of receivingVideo', () => {
    mockState.isDongleHardwarePresent = true
    mockState.isStreaming = true

    const { result } = renderHook(() => useTabsConfig(false))
    const carPlayTab = result.current.find((t) => t.path === '/')

    expect(carPlayTab).toBeDefined()
    expect((carPlayTab!.icon as any).props.sx).toEqual(
      expect.objectContaining({
        fontSize: 32,
        color: 'var(--ui-highlight)',
        opacity: 1
      })
    )
  })

  test('uses highlighted CarPlay icon styling when streaming and receivingVideo are both active', () => {
    mockState.isDongleHardwarePresent = true
    mockState.isStreaming = true

    const { result } = renderHook(() => useTabsConfig(true))
    const carPlayTab = result.current.find((t) => t.path === '/')

    expect(carPlayTab).toBeDefined()
    expect((carPlayTab!.icon as any).props.sx).toEqual(
      expect.objectContaining({
        fontSize: 32,
        color: 'var(--ui-highlight)',
        opacity: 1
      })
    )
    expect((carPlayTab!.icon as any).props.sx['&, &.MuiSvgIcon-root']).toEqual({
      color: 'var(--ui-highlight) !important'
    })
  })

  test('secondary window with nothing routed to it shows no tabs', () => {
    mockRole = 'dash'

    const { result } = renderHook(() => useTabsConfig(false))

    expect(result.current).toEqual([])
  })

  test('secondary window shows only the tabs routed to its role', () => {
    mockRole = 'aux'
    mockState.secondaryTelemetry = true
    mockState.secondaryMedia = true
    mockState.secondaryCamera = true

    const { result } = renderHook(() => useTabsConfig(false))

    expect(result.current.map((t) => t.path)).toEqual(['/telemetry', '/media', '/camera'])
  })

  test('secondary window hides the camera tab when the camera is unavailable', () => {
    mockRole = 'dash'
    mockState.secondaryCamera = true
    mockState.cameraFound = false

    const { result } = renderHook(() => useTabsConfig(false))

    expect(result.current.map((t) => t.path)).toEqual([])
  })

  test('secondary window shows only the telemetry tab when only telemetry is routed', () => {
    mockRole = 'dash'
    mockState.secondaryTelemetry = true

    const { result } = renderHook(() => useTabsConfig(false))

    expect(result.current.map((t) => t.path)).toEqual(['/telemetry'])
  })

  test('secondary window treats absent routing keys as not routed', () => {
    mockRole = 'aux'
    mockState.secondaryAbsentKeys = true

    const { result } = renderHook(() => useTabsConfig(false))

    expect(result.current).toEqual([])
  })

  test('uses the extra-small icon size on short viewports', () => {
    const originalInnerHeight = window.innerHeight
    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      writable: true,
      value: UI.XS_ICON_MAX_HEIGHT
    })

    const { result } = renderHook(() => useTabsConfig(false))
    const home = result.current.find((t) => t.path === '/')
    expect((home!.icon as any).props.sx.fontSize).toBe(24)

    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      writable: true,
      value: originalInnerHeight
    })
  })

  describe('the custom tab icon', () => {
    const customIconUrl = vi.fn()

    beforeEach(() => {
      customIconUrl.mockReset()
      ;(window as unknown as { app: unknown }).app = { customIconUrl }
      mockRole = 'main'
      mockState.mainCustom = true
    })

    function customTab() {
      const { result } = renderHook(() => useTabsConfig(false))
      return result
    }

    test('the folder icon is drawn as a mask in the current colour', async () => {
      customIconUrl.mockResolvedValue('app://index.html/custom/icon.svg')
      const result = customTab()

      await waitFor(() => {
        const icon = result.current.find((t) => t.label === 'Custom')?.icon as {
          props: { className?: string; style?: Record<string, string> }
        }
        expect(icon.props.className).toBe('MuiSvgIcon-root')
        expect(icon.props.style?.backgroundColor).toBe('currentColor')
        expect(icon.props.style?.maskImage).toBe('url(app://index.html/custom/icon.svg)')
      })
    })

    test('without one the default icon stays', async () => {
      customIconUrl.mockResolvedValue(null)
      const result = customTab()

      await waitFor(() => expect(customIconUrl).toHaveBeenCalled())
      const icon = result.current.find((t) => t.label === 'Custom')?.icon as {
        props: { className?: string }
      }
      expect(icon.props.className).toBeUndefined()
    })

    test('a failing lookup leaves the default icon', async () => {
      customIconUrl.mockRejectedValue(new Error('no'))
      const result = customTab()

      await waitFor(() => expect(customIconUrl).toHaveBeenCalled())
      expect(result.current.find((t) => t.label === 'Custom')).toBeTruthy()
    })

    test('an answer landing after unmount is dropped', async () => {
      let settle: (v: unknown) => void = () => {}
      customIconUrl.mockReturnValue(new Promise((r) => (settle = r)))
      const { unmount } = renderHook(() => useTabsConfig(false))

      unmount()
      settle('app://index.html/custom/icon.svg')
      await Promise.resolve()
    })

    test('stays on the default when the bridge is missing', async () => {
      ;(window as unknown as { app: unknown }).app = {}
      const result = customTab()

      expect(result.current.find((t) => t.label === 'Custom')).toBeTruthy()
    })
  })
})
