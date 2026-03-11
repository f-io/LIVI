import EventEmitter from 'events'

jest.mock('../../messages', () => {
  class MockDongleDriver extends EventEmitter {
    send = jest.fn(async () => true)
    initialise = jest.fn(async () => undefined)
    start = jest.fn(async () => undefined)
    stop = jest.fn(async () => undefined)
    sendBluetoothPairedList = jest.fn(async () => true)
  }

  class StubMsg {
    constructor(public value?: unknown) {}
  }

  return {
    DongleDriver: MockDongleDriver,
    Plugged: class {},
    Unplugged: class {},
    PhoneType: { CarPlay: 3, AndroidAuto: 5 },
    BluetoothPairedList: class {},
    VideoData: class {},
    AudioData: class {},
    MetaData: class {},
    MediaType: { Data: 1 },
    NavigationMetaType: { DashboardInfo: 200 },
    Command: class {},
    BoxInfo: class {},
    SoftwareVersion: class {},
    GnssData: class {},
    SendCommand: StubMsg,
    SendTouch: StubMsg,
    SendMultiTouch: StubMsg,
    SendAudio: StubMsg,
    SendFile: StubMsg,
    SendServerCgiScript: StubMsg,
    SendLiviWeb: StubMsg,
    SendDisconnectPhone: StubMsg,
    SendCloseDongle: StubMsg,
    FileAddress: { ICON_120: '/120', ICON_180: '/180', ICON_256: '/256' },
    BoxUpdateProgress: class {},
    BoxUpdateState: class {},
    MessageType: { NaviVideoData: 0x2c },
    decodeTypeMap: {},
    DEFAULT_CONFIG: { apkVer: '1.0.0', language: 'en' }
  }
})

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: jest.fn(),
  registerIpcOn: jest.fn()
}))

jest.mock('../ProjectionAudio', () => ({
  ProjectionAudio: jest.fn().mockImplementation(() => ({
    setInitialVolumes: jest.fn(),
    resetForSessionStart: jest.fn(),
    resetForSessionStop: jest.fn(),
    setStreamVolume: jest.fn(),
    setVisualizerEnabled: jest.fn(),
    handleAudioData: jest.fn()
  }))
}))

jest.mock('../FirmwareUpdateService', () => ({
  FirmwareUpdateService: jest.fn().mockImplementation(() => ({
    checkForUpdate: jest.fn(async () => ({ ok: true, hasUpdate: false, raw: {} })),
    downloadFirmwareToHost: jest.fn(),
    getLocalFirmwareStatus: jest.fn()
  }))
}))

jest.mock('@main/ipc/utils', () => ({
  configEvents: {
    on: jest.fn(),
    off: jest.fn(),
    emit: jest.fn()
  }
}))

import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { configEvents } from '@main/ipc/utils'

describe('ProjectionService', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('registers IPC handlers and listeners in constructor', () => {
    new ProjectionService()

    expect(registerIpcHandle).toHaveBeenCalled()
    expect(registerIpcOn).toHaveBeenCalled()
    expect((configEvents as any).on).toHaveBeenCalledWith('changed', expect.any(Function))
  })

  test('attachRenderer stores webContents reference', () => {
    const svc = new ProjectionService() as any
    const wc = { send: jest.fn() }

    svc.attachRenderer(wc)

    expect(svc.webContents).toBe(wc)
  })

  test('applyConfigPatch merges incoming patch into config', () => {
    const svc = new ProjectionService() as any
    svc.config = { language: 'en', kiosk: true }

    svc.applyConfigPatch({ language: 'de' })

    expect(svc.config).toEqual({ language: 'de', kiosk: true })
  })

  test('autoStartIfNeeded calls start when dongle is connected', async () => {
    const svc = new ProjectionService() as any
    svc.start = jest.fn(async () => undefined)

    svc.markDongleConnected(true)
    await svc.autoStartIfNeeded()

    expect(svc.start).toHaveBeenCalledTimes(1)
  })

  test('beginShutdown marks service shutting down and unsubscribes config events', () => {
    const svc = new ProjectionService() as any

    svc.beginShutdown()

    expect(svc.shuttingDown).toBe(true)
    expect((configEvents as any).off).toHaveBeenCalledWith('changed', expect.any(Function))
  })

  test('emitDongleInfoIfChanged emits only for new key', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { model: 'A15W' }

    svc.emitDongleInfoIfChanged()
    svc.emitDongleInfoIfChanged()

    expect(send).toHaveBeenCalledTimes(1)
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'dongleInfo',
      payload: {
        dongleFwVersion: '1.0.0',
        boxInfo: { model: 'A15W' }
      }
    })
  })
})
