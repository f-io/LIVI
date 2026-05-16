const registerIpcOnMock = jest.fn()
const registerIpcHandleMock = jest.fn()
const configEvents = {
  on: jest.fn(),
  off: jest.fn(),
  emit: jest.fn()
}
const getAllRendererWebContentsMock = jest.fn(() => [])

jest.mock('@main/ipc/register', () => ({
  registerIpcOn: (...a: unknown[]) => registerIpcOnMock(...a),
  registerIpcHandle: (...a: unknown[]) => registerIpcHandleMock(...a)
}))

jest.mock('@main/ipc/utils', () => ({
  configEvents
}))

jest.mock('@main/window/broadcast', () => ({
  getAllRendererWebContents: () => getAllRendererWebContentsMock()
}))

const removeAllListenersMock = jest.fn()
const removeHandlerMock = jest.fn()
jest.mock('electron', () => ({
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
    getAaDriver: jest.fn(() => null),
    getDongleDriver: jest.fn(() => null),
    addPluggedHook: jest.fn(() => () => {})
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
  jest.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

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
    const hook = (proj.addPluggedHook as jest.Mock).mock.calls[0][0] as () => void
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
})
