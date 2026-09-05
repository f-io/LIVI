import type { Mock } from 'vitest'

const registerIpcOnMock = vi.fn()
const registerIpcHandleMock = vi.fn()
const { configEvents } = vi.hoisted(() => ({
  configEvents: {
    on: vi.fn(),
    off: vi.fn(),
    emit: vi.fn()
  }
}))
const getAllRendererWebContentsMock = vi.fn(() => [])

vi.mock('@main/ipc/register', () => ({
  registerIpcOn: (...a: unknown[]) => registerIpcOnMock(...a),
  registerIpcHandle: (...a: unknown[]) => registerIpcHandleMock(...a)
}))

vi.mock('@main/ipc/utils', () => ({
  configEvents
}))

vi.mock('@main/window/broadcast', () => ({
  getAllRendererWebContents: () => getAllRendererWebContentsMock()
}))

const removeAllListenersMock = vi.fn()
const removeHandlerMock = vi.fn()
vi.mock('electron', () => ({
  ipcMain: {
    removeAllListeners: (...a: unknown[]) => removeAllListenersMock(...a),
    removeHandler: (...a: unknown[]) => removeHandlerMock(...a)
  }
}))

import type { ProjectionService } from '@main/services/projection/services/ProjectionService'
import type { Config } from '@shared/types'
import { setupTelemetry } from '../setupTelemetry'
import { TelemetryStore } from '../TelemetryStore'

function fakeProjection(): ProjectionService {
  return {
    getAaDriver: vi.fn(() => null),
    getCpDriver: vi.fn(() => null),
    setBlinkerSoundActive: vi.fn(),
    addPluggedHook: vi.fn(() => () => {})
  } as unknown as ProjectionService
}

beforeEach(() => {
  registerIpcOnMock.mockReset()
  registerIpcHandleMock.mockReset()
  configEvents.on.mockReset()
  configEvents.off.mockReset()
  removeAllListenersMock.mockReset()
  removeHandlerMock.mockReset()
  getAllRendererWebContentsMock.mockReturnValue([])
  vi.spyOn(console, 'warn').mockImplementation(function () {})
})
afterEach(() => vi.restoreAllMocks())

describe('setupTelemetry', () => {
  test('registers telemetry:push and telemetry:snapshot IPC', () => {
    const store = new TelemetryStore()
    setupTelemetry({ store })
    expect(registerIpcOnMock).toHaveBeenCalledWith('telemetry:push', expect.any(Function))
    expect(registerIpcHandleMock).toHaveBeenCalledWith('telemetry:snapshot', expect.any(Function))
  })

  test('telemetry:push routes into store.merge', () => {
    const store = new TelemetryStore()
    setupTelemetry({ store })
    const cb = registerIpcOnMock.mock.calls[0][1] as (
      _evt: unknown,
      payload: Record<string, unknown>
    ) => void
    cb(null, { speedKph: 42 })
    expect(store.snapshot().speedKph).toBe(42)
  })

  test('telemetry:snapshot returns the current snapshot', () => {
    const store = new TelemetryStore()
    store.merge({ speedKph: 50 })
    setupTelemetry({ store })
    const handler = registerIpcHandleMock.mock.calls[0][1] as () => unknown
    expect(handler()).toEqual(expect.objectContaining({ speedKph: 50 }))
  })

  test('appearanceMode "night" seeds nightMode=true', () => {
    const store = new TelemetryStore()
    setupTelemetry({ store, initialConfig: { appearanceMode: 'night' } as Config })
    expect(store.snapshot().nightMode).toBe(true)
  })

  test('appearanceMode "day" seeds nightMode=false', () => {
    const store = new TelemetryStore()
    setupTelemetry({ store, initialConfig: { appearanceMode: 'day' } as Config })
    expect(store.snapshot().nightMode).toBe(false)
  })

  test('appearanceMode change is forwarded to the store', () => {
    const store = new TelemetryStore()
    setupTelemetry({ store, initialConfig: { appearanceMode: 'day' } as Config })
    const onChange = configEvents.on.mock.calls.find((c) => c[0] === 'changed')![1] as (
      cfg: Config
    ) => void
    onChange({ appearanceMode: 'night' } as Config)
    expect(store.snapshot().nightMode).toBe(true)
    onChange({ appearanceMode: 'night' } as Config)
    expect(store.snapshot().nightMode).toBe(true)
  })

  test('initialConfig.lastKnownGps hydrates the store', () => {
    const store = new TelemetryStore()
    setupTelemetry({
      store,
      initialConfig: {
        lastKnownGps: { lat: 52, lng: 13, ts: 1_700_000_000 }
      } as unknown as Config
    })
    expect(store.snapshot().gps).toMatchObject({ lat: 52, lng: 13 })
  })

  test('with a projectionService, plugged hook fires hydrate calls', () => {
    const store = new TelemetryStore()
    const proj = fakeProjection()
    setupTelemetry({ store, projectionService: proj })
    expect(proj.addPluggedHook).toHaveBeenCalled()
    // Calling the hook should not throw
    const hook = (proj.addPluggedHook as Mock).mock.calls[0][0] as () => void
    expect(() => hook()).not.toThrow()
  })

  test('dispose removes all IPC + listeners', () => {
    const store = new TelemetryStore()
    const handle = setupTelemetry({ store })
    handle.dispose()
    expect(removeAllListenersMock).toHaveBeenCalledWith('telemetry:push')
    expect(removeHandlerMock).toHaveBeenCalledWith('telemetry:snapshot')
    expect(configEvents.off).toHaveBeenCalled()
  })

  test('adapters resolve their drivers through the projection service', () => {
    const store = new TelemetryStore()
    const proj = fakeProjection()
    setupTelemetry({ store, projectionService: proj })

    store.merge({ nightMode: true, turn: 'left' })
    expect(proj.getAaDriver).toHaveBeenCalled()
    expect(proj.getCpDriver).toHaveBeenCalled()
    expect(proj.setBlinkerSoundActive).toHaveBeenCalledWith(true)

    const hook = (proj.addPluggedHook as Mock).mock.calls[0][0] as () => void
    hook()
  })

  test('a throwing hydrate is contained per adapter', () => {
    const store = new TelemetryStore()
    const proj = fakeProjection()
    ;(proj.getAaDriver as Mock).mockImplementation(() => {
      throw new Error('aa gone')
    })
    ;(proj.getCpDriver as Mock).mockImplementation(() => {
      throw new Error('cp gone')
    })
    store.merge({ nightMode: true })
    setupTelemetry({ store, projectionService: proj })

    const hook = (proj.addPluggedHook as Mock).mock.calls[0][0] as () => void
    expect(() => hook()).not.toThrow()
    expect(console.warn).toHaveBeenCalledWith(
      '[setupTelemetry] aa.hydrate threw (ignored)',
      expect.any(Error)
    )
    expect(console.warn).toHaveBeenCalledWith(
      '[setupTelemetry] cp.hydrate threw (ignored)',
      expect.any(Error)
    )
  })

  test('dispose with a projection service detaches the adapters too', () => {
    const store = new TelemetryStore()
    const proj = fakeProjection()
    const offPlug = vi.fn()
    ;(proj.addPluggedHook as Mock).mockReturnValue(offPlug)
    const handle = setupTelemetry({ store, projectionService: proj })
    handle.dispose()
    expect(offPlug).toHaveBeenCalledTimes(1)

    ;(proj.getAaDriver as Mock).mockClear()
    store.merge({ nightMode: true })
    expect(proj.getAaDriver).not.toHaveBeenCalled()
  })
})
