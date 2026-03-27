import { renderHook } from '@testing-library/react'
import { useTabsConfig } from '../useTabsConfig'

let mockState = {
  isStreaming: false,
  isDongleConnected: false,
  cameraFound: true,
  mapsEnabled: false,
  telemetryEnabled: false,
  settingsMissing: false
}

jest.mock('@mui/material/styles', () => ({
  useTheme: () => ({
    palette: {
      text: { primary: '#fff', disabled: '#777' }
    }
  })
}))

jest.mock('@store/store', () => ({
  useStatusStore: (selector: (s: any) => unknown) =>
    selector({
      isStreaming: mockState.isStreaming,
      isDongleConnected: mockState.isDongleConnected,
      cameraFound: mockState.cameraFound
    }),
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: mockState.settingsMissing
        ? undefined
        : {
            mapsEnabled: mockState.mapsEnabled,
            telemetryEnabled: mockState.telemetryEnabled
          }
    })
}))

describe('useTabsConfig', () => {
  beforeEach(() => {
    mockState = {
      isStreaming: false,
      isDongleConnected: false,
      cameraFound: true,
      mapsEnabled: false,
      telemetryEnabled: false,
      settingsMissing: false
    }
  })

  test('returns base tabs by default', () => {
    const { result } = renderHook(() => useTabsConfig(false))
    expect(result.current.map((t) => t.path)).toEqual(['/', '/media', '/camera', '/settings'])
  })

  test('adds maps and telemetry tabs when enabled', () => {
    mockState.mapsEnabled = true
    mockState.telemetryEnabled = true
    const { result } = renderHook(() => useTabsConfig(false))
    expect(result.current.map((t) => t.path)).toEqual([
      '/',
      '/maps',
      '/telemetry',
      '/media',
      '/camera',
      '/settings'
    ])
  })

  test('disables camera tab when camera is not found', () => {
    mockState.cameraFound = false
    const { result } = renderHook(() => useTabsConfig(false))
    const camera = result.current.find((t) => t.path === '/camera')
    expect(camera?.disabled).toBe(true)
  })

  test('returns active CarPlay icon variant when dongle is connected', () => {
    mockState.isDongleConnected = true

    const { result } = renderHook(() => useTabsConfig(false))
    const carPlayTab = result.current.find((t) => t.path === '/')

    expect(carPlayTab).toBeDefined()
    expect((carPlayTab!.icon as any).props.sx).toEqual(
      expect.objectContaining({
        fontSize: 30,
        color: '#fff',
        opacity: 'var(--ui-breathe-opacity, 1)'
      })
    )
    expect((carPlayTab!.icon as any).props.sx['&, &.MuiSvgIcon-root']).toEqual({
      color: '#fff !important'
    })
  })

  test('falls back to false for maps and telemetry when settings are missing', () => {
    mockState.settingsMissing = true

    const { result } = renderHook(() => useTabsConfig(false))

    expect(result.current.map((t) => t.path)).toEqual(['/', '/media', '/camera', '/settings'])
  })

  test('uses base CarPlay icon styling when streaming is active but receivingVideo is false', () => {
    mockState.isDongleConnected = true
    mockState.isStreaming = true

    const { result } = renderHook(() => useTabsConfig(false))
    const carPlayTab = result.current.find((t) => t.path === '/')

    expect(carPlayTab).toBeDefined()
    expect((carPlayTab!.icon as any).props.sx).toEqual(
      expect.objectContaining({
        fontSize: 30,
        color: '#fff',
        opacity: 'var(--ui-breathe-opacity, 1)'
      })
    )
    expect((carPlayTab!.icon as any).props.sx['&, &.MuiSvgIcon-root']).toEqual({
      color: '#fff !important'
    })
  })

  test('uses highlighted CarPlay icon styling when streaming and receivingVideo are both active', () => {
    mockState.isDongleConnected = true
    mockState.isStreaming = true

    const { result } = renderHook(() => useTabsConfig(true))
    const carPlayTab = result.current.find((t) => t.path === '/')

    expect(carPlayTab).toBeDefined()
    expect((carPlayTab!.icon as any).props.sx).toEqual(
      expect.objectContaining({
        fontSize: 30,
        color: 'var(--ui-highlight)',
        opacity: 1
      })
    )
    expect((carPlayTab!.icon as any).props.sx['&, &.MuiSvgIcon-root']).toEqual({
      color: 'var(--ui-highlight) !important'
    })
  })
})
