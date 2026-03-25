import EventEmitter from 'events'
import fs from 'fs'
import {
  Plugged,
  Unplugged,
  SoftwareVersion,
  BoxInfo,
  GnssData,
  BluetoothPairedList,
  BoxUpdateProgress,
  BoxUpdateState,
  Command,
  AudioData,
  PhoneType,
  decodeTypeMap
} from '../../messages'
import { PhoneWorkMode } from '@shared/types'

jest.mock('../../messages', () => {
  class MockDongleDriver extends EventEmitter {
    send = jest.fn(async () => true)
    initialise = jest.fn(async () => undefined)
    start = jest.fn(async () => undefined)
    stop = jest.fn(async () => undefined)
    close = jest.fn(async () => undefined)
    sendBluetoothPairedList = jest.fn(async () => true)
  }

  class StubMsg {
    constructor(
      public value?: unknown,
      public value2?: unknown
    ) {}
  }

  return {
    DongleDriver: MockDongleDriver,
    Plugged: class {
      constructor(public phoneType?: number) {}
    },
    Unplugged: class {},
    PhoneType: { CarPlay: 3, AndroidAuto: 5 },
    BluetoothPairedList: class {
      constructor(public data?: unknown) {}
    },
    VideoData: class {},
    AudioData: class {},
    MetaData: class {},
    MediaType: { Data: 1 },
    NavigationMetaType: { DashboardInfo: 200 },
    Command: class {
      constructor(public value?: unknown) {}
    },
    BoxInfo: class {
      constructor(public settings?: unknown) {}
    },
    SoftwareVersion: class {
      constructor(public version?: string) {}
    },
    GnssData: class {
      constructor(public text?: string) {}
    },
    SendRawMessage: StubMsg,
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
    BoxUpdateProgress: class {
      constructor(public progress?: number) {}
    },
    BoxUpdateState: class {
      status = 0
      statusText = 'ok'
      isOta = false
      isTerminal = false
      ok = true
    },
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

jest.mock('electron', () => ({
  app: {
    getPath: jest.fn(() => '/tmp/appdata')
  },
  WebContents: class {}
}))

jest.mock('usb', () => ({
  WebUSBDevice: {
    createInstance: jest.fn(async () => ({
      open: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined),
      reset: jest.fn(async () => undefined)
    }))
  },
  usb: {
    getDeviceList: jest.fn(() => [])
  }
}))

jest.mock('../utils/readMediaFile', () => ({
  readMediaFile: jest.fn(() => ({
    timestamp: 't',
    payload: {
      type: 1,
      media: { MediaSongName: 'Song', MediaPlayStatus: 1 },
      base64Image: 'img'
    }
  }))
}))

jest.mock('../utils/readNavigationFile', () => ({
  readNavigationFile: jest.fn(() => ({
    timestamp: 't',
    payload: {
      metaType: 200,
      navi: null,
      rawUtf8: '',
      error: false
    }
  }))
}))

import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { configEvents } from '@main/ipc/utils'
import { readMediaFile } from '../utils/readMediaFile'
import { readNavigationFile } from '../utils/readNavigationFile'
import { WebUSBDevice, usb } from 'usb'

describe('ProjectionService', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    jest.restoreAllMocks()
  })

  function getHandle<T = (...args: any[]) => any>(channel: string): T {
    const row = (registerIpcHandle as jest.Mock).mock.calls.find(([ch]) => ch === channel)
    if (!row) throw new Error(`Missing ipc handle: ${channel}`)
    return row[1] as T
  }

  function getOn<T = (...args: any[]) => any>(channel: string): T {
    const row = (registerIpcOn as jest.Mock).mock.calls.find(([ch]) => ch === channel)
    if (!row) throw new Error(`Missing ipc on: ${channel}`)
    return row[1] as T
  }

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

  test('autoStartIfNeeded does nothing while shutting down', async () => {
    const svc = new ProjectionService() as any
    svc.start = jest.fn(async () => undefined)
    svc.shuttingDown = true

    svc.markDongleConnected(true)
    await svc.autoStartIfNeeded()

    expect(svc.start).not.toHaveBeenCalled()
  })

  test('autoStartIfNeeded does nothing when already started', async () => {
    const svc = new ProjectionService() as any
    svc.start = jest.fn(async () => undefined)
    svc.started = true

    svc.markDongleConnected(true)
    await svc.autoStartIfNeeded()

    expect(svc.start).not.toHaveBeenCalled()
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

  test('emitDongleInfoIfChanged does nothing without renderer', () => {
    const svc = new ProjectionService() as any
    svc.webContents = null
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { model: 'A15W' }

    expect(() => svc.emitDongleInfoIfChanged()).not.toThrow()
  })

  test('emitDongleInfoIfChanged emits again when key changes', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { model: 'A15W' }
    svc.emitDongleInfoIfChanged()

    svc.boxInfo = { model: 'A15X' }
    svc.emitDongleInfoIfChanged()

    expect(send).toHaveBeenCalledTimes(2)
  })

  test('getDevToolsUrlCandidates returns strict host/path combinations', () => {
    const svc = new ProjectionService() as any

    const urls = svc.getDevToolsUrlCandidates()

    expect(urls).toEqual([
      'http://192.168.50.1/',
      'http://192.168.50.1/index.html',
      'http://192.168.50.1/cgi-bin/server.cgi?action=ls&path=/',
      'http://192.168.43.1/',
      'http://192.168.43.1/index.html',
      'http://192.168.43.1/cgi-bin/server.cgi?action=ls&path=/',
      'http://192.168.3.1/',
      'http://192.168.3.1/index.html',
      'http://192.168.3.1/cgi-bin/server.cgi?action=ls&path=/'
    ])
  })

  test('sendChunked does nothing without renderer', () => {
    const svc = new ProjectionService() as any
    svc.webContents = null

    expect(() =>
      svc.sendChunked('projection-video-chunk', new Uint8Array([1, 2, 3]).buffer, 2)
    ).not.toThrow()
  })

  test('sendChunked does nothing when data is missing', () => {
    const svc = new ProjectionService() as any
    svc.webContents = { send: jest.fn() }

    svc.sendChunked('projection-video-chunk', undefined, 2)

    expect(svc.webContents.send).not.toHaveBeenCalled()
  })

  test('sendChunked splits payload into envelopes', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    svc.sendChunked('projection-video-chunk', new Uint8Array([1, 2, 3, 4, 5]).buffer, 2, {
      kind: 'video'
    })

    expect(send).toHaveBeenCalledTimes(3)

    const first = send.mock.calls[0][1]
    const second = send.mock.calls[1][1]
    const third = send.mock.calls[2][1]

    expect(send.mock.calls[0][0]).toBe('projection-video-chunk')
    expect(first.offset).toBe(0)
    expect(first.total).toBe(5)
    expect(first.isLast).toBe(false)
    expect(first.kind).toBe('video')
    expect(Buffer.isBuffer(first.chunk)).toBe(true)

    expect(second.offset).toBe(2)
    expect(second.isLast).toBe(false)

    expect(third.offset).toBe(4)
    expect(third.isLast).toBe(true)
  })

  test('clearTimeouts clears pair timeout and frame interval', () => {
    const svc = new ProjectionService() as any
    const clearTimeoutSpy = jest.spyOn(global, 'clearTimeout')
    const clearIntervalSpy = jest.spyOn(global, 'clearInterval')

    svc.pairTimeout = setTimeout(() => {}, 1000)
    svc.frameInterval = setInterval(() => {}, 1000)

    svc.clearTimeouts()

    expect(clearTimeoutSpy).toHaveBeenCalled()
    expect(clearIntervalSpy).toHaveBeenCalled()
    expect(svc.pairTimeout).toBeNull()
    expect(svc.frameInterval).toBeNull()
  })

  test('reloadConfigFromDisk returns when file is missing', async () => {
    const svc = new ProjectionService() as any
    const existsSpy = jest.spyOn(fs, 'existsSync').mockReturnValue(false)

    svc.config = { language: 'en', apkVer: '1.0.0' }

    await svc.reloadConfigFromDisk()

    expect(svc.config).toEqual({ language: 'en', apkVer: '1.0.0' })
    existsSpy.mockRestore()
  })

  test('reloadConfigFromDisk merges config from disk', async () => {
    const svc = new ProjectionService() as any
    jest.spyOn(fs, 'existsSync').mockReturnValue(true)
    jest
      .spyOn(fs, 'readFileSync')
      .mockReturnValue(JSON.stringify({ language: 'de', audioVolume: 0.3 }) as any)

    svc.config = { language: 'en', apkVer: '1.0.0' }

    await svc.reloadConfigFromDisk()

    expect(svc.config).toEqual({
      language: 'de',
      apkVer: '1.0.0',
      audioVolume: 0.3
    })
  })

  test('reloadConfigFromDisk swallows invalid json', async () => {
    const svc = new ProjectionService() as any
    jest.spyOn(fs, 'existsSync').mockReturnValue(true)
    jest.spyOn(fs, 'readFileSync').mockReturnValue('{bad json' as any)

    svc.config = { language: 'en', apkVer: '1.0.0' }

    await expect(svc.reloadConfigFromDisk()).resolves.toBeUndefined()
    expect(svc.config).toEqual({ language: 'en', apkVer: '1.0.0' })
  })

  test('getApkVer returns current apk version from config', () => {
    const svc = new ProjectionService() as any
    svc.config = { apkVer: '2.3.4' }

    expect(svc.getApkVer()).toBe('2.3.4')
  })

  test('markDongleConnected updates shared dongle connection state', async () => {
    const svc = new ProjectionService() as any
    svc.start = jest.fn(async () => undefined)

    svc.markDongleConnected(false)
    await svc.autoStartIfNeeded()
    expect(svc.start).not.toHaveBeenCalled()

    svc.markDongleConnected(true)
    await svc.autoStartIfNeeded()
    expect(svc.start).toHaveBeenCalledTimes(1)
  })

  test('disconnectPhone returns false when service is not started', async () => {
    const svc = new ProjectionService() as any
    svc.started = false

    await expect(svc.disconnectPhone()).resolves.toBe(false)
  })

  test('disconnectPhone sends disconnect and close commands and waits on success', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.driver.send = jest.fn().mockResolvedValueOnce(true).mockResolvedValueOnce(false)

    const setTimeoutSpy = jest.spyOn(global, 'setTimeout').mockImplementation(((
      fn: (...args: any[]) => void
    ) => {
      fn()
      return 0 as any
    }) as typeof setTimeout)

    await expect(svc.disconnectPhone()).resolves.toBe(true)
    expect(svc.driver.send).toHaveBeenCalledTimes(2)
    expect(setTimeoutSpy).toHaveBeenCalled()
  })

  test('disconnectPhone swallows command errors and returns false when both fail', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.driver.send = jest.fn().mockRejectedValue(new Error('boom'))

    await expect(svc.disconnectPhone()).resolves.toBe(false)
    expect(svc.driver.send).toHaveBeenCalledTimes(2)
  })

  test('patchAaMediaPlayStatus writes media snapshot and emits projection event', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    jest.spyOn(fs, 'writeFileSync').mockImplementation(() => {})

    svc.patchAaMediaPlayStatus(2)

    expect(fs.writeFileSync).toHaveBeenCalled()
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'media',
      payload: {
        mediaType: 1,
        payload: {
          type: 1,
          media: {
            MediaPlayStatus: 2
          }
        }
      }
    })
  })

  test('patchAaMediaPlayStatus swallows write errors', () => {
    const svc = new ProjectionService() as any
    jest.spyOn(fs, 'writeFileSync').mockImplementation(() => {
      throw new Error('disk fail')
    })

    expect(() => svc.patchAaMediaPlayStatus(1)).not.toThrow()
  })

  test('resetMediaSnapshot writes default media payload and emits reset event', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    jest.spyOn(fs, 'writeFileSync').mockImplementation(() => {})

    svc.resetMediaSnapshot('test')

    expect(fs.writeFileSync).toHaveBeenCalled()
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'media-reset',
      reason: 'test'
    })
  })

  test('resetNavigationSnapshot writes default navigation payload and emits reset event', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    jest.spyOn(fs, 'writeFileSync').mockImplementation(() => {})

    svc.resetNavigationSnapshot('test')

    expect(fs.writeFileSync).toHaveBeenCalled()
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'navigation-reset',
      reason: 'test'
    })
  })

  test('stop returns early when already stopping or not started', async () => {
    const svc = new ProjectionService() as any

    svc.isStopping = true
    svc.stopPromise = Promise.resolve()
    await expect(svc.stop()).resolves.toBeUndefined()

    svc.isStopping = false
    svc.started = false
    svc.stopping = false
    await expect(svc.stop()).resolves.toBeUndefined()
  })

  test('stop resets session state and closes driver', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.stopping = false
    svc.disconnectPhone = jest.fn(async () => true)
    svc.driver.close = jest.fn(async () => undefined)
    svc.audio.resetForSessionStop = jest.fn()
    svc.clearTimeouts = jest.fn()
    svc.resetMediaSnapshot = jest.fn()
    svc.resetNavigationSnapshot = jest.fn()

    await svc.stop()

    expect(svc.clearTimeouts).toHaveBeenCalled()
    expect(svc.disconnectPhone).toHaveBeenCalled()
    expect(svc.driver.close).toHaveBeenCalled()
    expect(svc.audio.resetForSessionStop).toHaveBeenCalled()
    expect(svc.started).toBe(false)
    expect(svc.lastDongleInfoEmitKey).toBe('')
  })

  test('projection-start handler delegates to start', async () => {
    const svc = new ProjectionService() as any
    svc.start = jest.fn(async () => undefined)

    const h = getHandle('projection-start').bind(svc)
    await h()

    expect(svc.start).toHaveBeenCalledTimes(1)
  })

  test('projection-stop handler delegates to stop', async () => {
    const svc = new ProjectionService() as any
    svc.stop = jest.fn(async () => undefined)

    const h = getHandle('projection-stop').bind(svc)
    await h()

    expect(svc.stop).toHaveBeenCalledTimes(1)
  })

  test('projection-sendframe handler sends frame command', async () => {
    const svc = new ProjectionService() as any
    const h = getHandle('projection-sendframe')

    await h.call(svc)

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-bt-pairedlist-set returns false when not started', async () => {
    const svc = new ProjectionService() as any
    svc.started = false
    const h = getHandle('projection-bt-pairedlist-set')

    await expect(h.call(svc, null, 'abc')).resolves.toEqual({ ok: false })
  })

  test('projection-bt-pairedlist-set forwards list when started', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.driver.sendBluetoothPairedList = jest.fn(async () => true)
    const h = getHandle('projection-bt-pairedlist-set')

    await expect(h.call(svc, null, 'abc')).resolves.toEqual({ ok: true })
    expect(svc.driver.sendBluetoothPairedList).toHaveBeenCalledWith('abc')
  })

  test('projection-upload-icons throws when projection is not started', async () => {
    const svc = new ProjectionService() as any
    svc.started = false
    svc.webUsbDevice = null
    const h = getHandle('projection-upload-icons')

    await expect(h.call(svc)).rejects.toThrow(
      '[ProjectionService] Projection is not started or dongle not connected'
    )
  })

  test('projection-upload-icons calls uploadIcons when ready', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.webUsbDevice = {}
    svc.uploadIcons = jest.fn()
    const h = getHandle('projection-upload-icons')

    await h.call(svc)

    expect(svc.uploadIcons).toHaveBeenCalledTimes(1)
  })

  test('projection-upload-livi-scripts throws when projection is not ready', async () => {
    const svc = new ProjectionService() as any
    svc.started = false
    svc.webUsbDevice = null
    const h = getHandle('projection-upload-livi-scripts')

    await expect(h.call(svc)).rejects.toThrow(
      '[ProjectionService] Projection is not started or dongle not connected'
    )
  })

  test('projection-upload-livi-scripts uploads both assets and returns result object', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.webUsbDevice = {}
    svc.driver.send = jest.fn().mockResolvedValueOnce(true).mockResolvedValueOnce(false)

    const h = getHandle('projection-upload-livi-scripts')
    const result = await h.call(svc)

    expect(result.cgiOk).toBe(true)
    expect(result.webOk).toBe(false)
    expect(result.ok).toBe(false)
    expect(result.urls).toHaveLength(9)
  })

  test('maps:request disables maps and clears cached resolution', async () => {
    const svc = new ProjectionService() as any
    svc.lastNaviVideoWidth = 123
    svc.lastNaviVideoHeight = 456

    const h = getHandle('maps:request')
    await expect(h.call(svc, null, false)).resolves.toEqual({ ok: true, enabled: false })

    expect(svc.mapsRequested).toBe(false)
    expect(svc.lastNaviVideoWidth).toBeUndefined()
    expect(svc.lastNaviVideoHeight).toBeUndefined()
  })

  test('maps:request enables maps and requests focus', async () => {
    const svc = new ProjectionService() as any
    const h = getHandle('maps:request')

    await expect(h.call(svc, null, true)).resolves.toEqual({ ok: true, enabled: true })

    expect(svc.mapsRequested).toBe(true)
    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-touch forwards touch payload as message', () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-touch')

    on.call(svc, null, { x: 1, y: 2, action: 3 })

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-multi-touch ignores empty arrays', () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-multi-touch')

    on.call(svc, null, [])

    expect(svc.driver.send).not.toHaveBeenCalled()
  })

  test('projection-multi-touch sanitizes points and sends message', () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-multi-touch')

    on.call(svc, null, [{ id: 3.9, x: -1, y: 2, action: 7.8 }])

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-raw-message ignores payload when not started', () => {
    const svc = new ProjectionService() as any
    svc.started = false
    const on = getOn('projection-raw-message')

    on.call(svc, null, { type: 1, data: [1, 2, 3] })

    expect(svc.driver.send).not.toHaveBeenCalled()
  })

  test('projection-raw-message sends raw message when started', () => {
    const svc = new ProjectionService() as any
    svc.started = true
    const on = getOn('projection-raw-message')

    on.call(svc, null, { type: 9, data: [1, 2, 3] })

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-command forwards command message', () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-command')

    on.call(svc, null, 'frame')

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-set-volume delegates to ProjectionAudio', () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-set-volume')

    on.call(svc, null, { stream: 'music', volume: 0.5 })

    expect(svc.audio.setStreamVolume).toHaveBeenCalledWith('music', 0.5)
  })

  test('projection-set-visualizer-enabled delegates to ProjectionAudio', () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-set-visualizer-enabled')

    on.call(svc, null, 1)

    expect(svc.audio.setVisualizerEnabled).toHaveBeenCalledWith(true)
  })

  test('projection-media-read returns default response when file is missing', async () => {
    const svc = new ProjectionService() as any
    jest.spyOn(fs, 'existsSync').mockReturnValue(false)

    const h = getHandle('projection-media-read')
    const out = await h.call(svc)

    expect(out).toEqual(expect.objectContaining({ payload: expect.any(Object) }))
    expect(readMediaFile).not.toHaveBeenCalled()
  })

  test('projection-media-read reads file when it exists', async () => {
    const svc = new ProjectionService() as any
    jest.spyOn(fs, 'existsSync').mockReturnValue(true)

    const h = getHandle('projection-media-read')
    const out = await h.call(svc)

    expect(readMediaFile).toHaveBeenCalledWith('/tmp/appdata/mediaData.json')
    expect(out).toEqual({
      timestamp: 't',
      payload: {
        type: 1,
        media: { MediaSongName: 'Song', MediaPlayStatus: 1 },
        base64Image: 'img'
      }
    })
  })

  test('projection-navigation-read returns default response when service is not started', async () => {
    const svc = new ProjectionService() as any
    svc.started = false

    const h = getHandle('projection-navigation-read')
    const out = await h.call(svc)

    expect(out).toEqual(expect.objectContaining({ payload: expect.any(Object) }))
    expect(readNavigationFile).not.toHaveBeenCalled()
  })

  test('projection-navigation-read returns default response when file is missing', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    jest.spyOn(fs, 'existsSync').mockReturnValue(false)

    const h = getHandle('projection-navigation-read')
    const out = await h.call(svc)

    expect(out).toEqual(expect.objectContaining({ payload: expect.any(Object) }))
    expect(readNavigationFile).not.toHaveBeenCalled()
  })

  test('projection-navigation-read reads file when started and file exists', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    jest.spyOn(fs, 'existsSync').mockReturnValue(true)

    const h = getHandle('projection-navigation-read')
    const out = await h.call(svc)

    expect(readNavigationFile).toHaveBeenCalledWith('/tmp/appdata/navigationData.json')
    expect(out).toEqual({
      timestamp: 't',
      payload: {
        metaType: 200,
        navi: null,
        rawUtf8: '',
        error: false
      }
    })
  })

  test('uploadIcons reloads disk config and sends 3 icon files', () => {
    const svc = new ProjectionService() as any
    jest.spyOn(fs, 'existsSync').mockReturnValue(true)
    jest.spyOn(fs, 'readFileSync').mockReturnValue(
      JSON.stringify({
        dongleIcon120: Buffer.from('120').toString('base64'),
        dongleIcon180: Buffer.from('180').toString('base64'),
        dongleIcon256: Buffer.from('256').toString('base64')
      }) as any
    )

    svc.uploadIcons()

    expect(svc.driver.send).toHaveBeenCalledTimes(3)
  })

  test('uploadIcons cancels when icon fields are missing', () => {
    const svc = new ProjectionService() as any
    jest.spyOn(fs, 'existsSync').mockReturnValue(true)
    jest.spyOn(fs, 'readFileSync').mockReturnValue(JSON.stringify({ dongleIcon120: 'abc' }) as any)

    svc.uploadIcons()

    expect(svc.driver.send).not.toHaveBeenCalled()
  })

  test('start returns early when no matching usb dongle exists', async () => {
    const svc = new ProjectionService() as any
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([])
    svc.audio.setInitialVolumes = jest.fn()
    svc.audio.resetForSessionStart = jest.fn()
    svc.resetMediaSnapshot = jest.fn()
    svc.resetNavigationSnapshot = jest.fn()

    await svc.start()

    expect(svc.audio.setInitialVolumes).toHaveBeenCalled()
    expect(svc.audio.resetForSessionStart).toHaveBeenCalled()
    expect(svc.resetMediaSnapshot).toHaveBeenCalledWith('session-start')
    expect(svc.resetNavigationSnapshot).toHaveBeenCalledWith('session-start')
    expect(svc.started).toBe(false)
  })

  test('start initialises webusb, driver and marks service started', async () => {
    const svc = new ProjectionService() as any
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([
      { deviceDescriptor: { idVendor: 0x1314, idProduct: 0x1520 } }
    ])

    await svc.start()

    expect(WebUSBDevice.createInstance).toHaveBeenCalled()
    expect(svc.driver.initialise).toHaveBeenCalled()
    expect(svc.driver.start).toHaveBeenCalled()
    expect(svc.started).toBe(true)
  })

  test('start closes webUsbDevice and leaves started=false when driver init fails', async () => {
    const svc = new ProjectionService() as any
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([
      { deviceDescriptor: { idVendor: 0x1314, idProduct: 0x1520 } }
    ])
    svc.driver.initialise = jest.fn(async () => {
      throw new Error('init fail')
    })

    await svc.start()

    expect(svc.started).toBe(false)
    expect(svc.webUsbDevice).toBeNull()
  })
  test('dongle-fw check emits start/done events and returns shaped success result', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.checkForUpdate = jest.fn(async () => ({
      ok: true,
      hasUpdate: true,
      latestVer: '2.0.0',
      size: 321,
      token: 'tok',
      id: 'id1',
      notes: 'note1',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'raw-token',
        ver: '2.0.0',
        size: '321',
        id: 'raw-id',
        notes: 'raw-note'
      }
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'check' })

    expect(svc.reloadConfigFromDisk).toHaveBeenCalledTimes(1)
    expect(svc.firmware.checkForUpdate).toHaveBeenCalledWith({
      appVer: '9.9.9',
      dongleFwVersion: '1.0.0',
      boxInfo: { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    })

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'check:start'
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'check:done',
      result: {
        ok: true,
        hasUpdate: true,
        size: 321,
        token: 'tok',
        request: { foo: 'bar' },
        raw: {
          err: 0,
          token: 'tok',
          ver: '2.0.0',
          size: 321,
          id: 'id1',
          notes: 'note1',
          msg: undefined,
          error: undefined
        }
      }
    })

    expect(out).toEqual({
      ok: true,
      hasUpdate: true,
      size: 321,
      token: 'tok',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'tok',
        ver: '2.0.0',
        size: 321,
        id: 'id1',
        notes: 'note1',
        msg: undefined,
        error: undefined
      }
    })
  })

  test('dongle-fw check converts failed firmware check into renderer error shape', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.checkForUpdate = jest.fn(async () => ({
      ok: false,
      error: 'network down'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'check' })

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'check:start'
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'check:done',
      result: {
        ok: false,
        hasUpdate: false,
        size: 0,
        error: 'network down',
        raw: { err: -1, msg: 'network down' }
      }
    })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'network down',
      raw: { err: -1, msg: 'network down' }
    })
  })

  test('dongle-fw check falls back to unknown error text when failed result has no message', async () => {
    const svc = new ProjectionService() as any
    svc.webContents = { send: jest.fn() }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.checkForUpdate = jest.fn(async () => ({
      ok: false
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'check' })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'Unknown error',
      raw: { err: -1, msg: 'Unknown error' }
    })
  })

  test('dongle-fw download path downloads update and emits progress events', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.checkForUpdate = jest.fn(async () => ({
      ok: true,
      hasUpdate: true,
      latestVer: '2.0.0',
      size: 321,
      token: 'tok',
      id: 'id1',
      notes: 'note1',
      request: { foo: 'bar' },
      raw: { err: 0, ver: '2.0.0', size: 321, token: 'tok', id: 'id1', notes: 'note1' }
    }))

    svc.firmware.downloadFirmwareToHost = jest.fn(async (_check: any, opts: any) => {
      opts.onProgress?.({ received: 50, total: 100, percent: 0.5 })
      opts.onProgress?.({ received: 100, total: 100, percent: 1 })
      return {
        ok: true,
        path: '/tmp/appdata/firmware/A15W_Update.img',
        bytes: 321
      }
    })

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'download' })

    expect(svc.firmware.checkForUpdate).toHaveBeenCalledTimes(1)
    expect(svc.firmware.downloadFirmwareToHost).toHaveBeenCalledTimes(1)
    expect(svc.firmware.downloadFirmwareToHost).toHaveBeenCalledWith(
      expect.objectContaining({
        ok: true,
        hasUpdate: true,
        latestVer: '2.0.0'
      }),
      expect.objectContaining({
        overwrite: true,
        onProgress: expect.any(Function)
      })
    )

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'download:start'
    })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'download:progress',
      received: 50,
      total: 100,
      percent: 0.5
    })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'download:progress',
      received: 100,
      total: 100,
      percent: 1
    })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'download:done',
      path: '/tmp/appdata/firmware/A15W_Update.img',
      bytes: 321
    })

    expect(out).toEqual({
      ok: true,
      hasUpdate: true,
      size: 321,
      token: 'tok',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'tok',
        ver: '2.0.0',
        size: 321,
        id: 'id1',
        notes: 'note1',
        msg: undefined,
        error: undefined
      }
    })
  })

  test('dongle-fw download returns shaped check result when no update is available', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.checkForUpdate = jest.fn(async () => ({
      ok: true,
      hasUpdate: false,
      latestVer: '1.0.0',
      size: 111,
      token: 'tok',
      id: 'id1',
      notes: 'up-to-date',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'tok',
        ver: '1.0.0',
        size: 111,
        id: 'id1',
        notes: 'up-to-date'
      }
    }))

    svc.firmware.downloadFirmwareToHost = jest.fn()

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'download' })

    expect(svc.firmware.checkForUpdate).toHaveBeenCalledTimes(1)
    expect(svc.firmware.downloadFirmwareToHost).not.toHaveBeenCalled()

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:start'
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:done',
      path: null,
      bytes: 0
    })

    expect(out).toEqual({
      ok: true,
      hasUpdate: false,
      size: 111,
      token: 'tok',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'tok',
        ver: '1.0.0',
        size: 111,
        id: 'id1',
        notes: 'up-to-date',
        msg: undefined,
        error: undefined
      }
    })
  })
  test('dongle-fw download returns shaped check result when no update is available', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.checkForUpdate = jest.fn(async () => ({
      ok: true,
      hasUpdate: false,
      latestVer: '1.0.0',
      size: 111,
      token: 'tok',
      id: 'id1',
      notes: 'up-to-date',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'tok',
        ver: '1.0.0',
        size: 111,
        id: 'id1',
        notes: 'up-to-date'
      }
    }))

    svc.firmware.downloadFirmwareToHost = jest.fn()

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'download' })

    expect(svc.firmware.checkForUpdate).toHaveBeenCalledTimes(1)
    expect(svc.firmware.downloadFirmwareToHost).not.toHaveBeenCalled()

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:start'
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:done',
      path: null,
      bytes: 0
    })

    expect(out).toEqual({
      ok: true,
      hasUpdate: false,
      size: 111,
      token: 'tok',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'tok',
        ver: '1.0.0',
        size: 111,
        id: 'id1',
        notes: 'up-to-date',
        msg: undefined,
        error: undefined
      }
    })
  })

  test('dongle-fw download returns error shape when check fails', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.checkForUpdate = jest.fn(async () => ({
      ok: false,
      error: 'check failed'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'download' })

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:start'
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:error',
      message: 'check failed'
    })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'check failed',
      raw: { err: -1, msg: 'check failed' }
    })
  })

  test('dongle-fw download returns error shape when host download fails', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    const checkResult = {
      ok: true,
      hasUpdate: true,
      latestVer: '2.0.0',
      size: 222,
      token: 'tok',
      id: 'id1',
      notes: 'note',
      request: { foo: 'bar' },
      raw: { err: 0, ver: '2.0.0', size: 222 }
    }

    svc.firmware.checkForUpdate = jest.fn(async () => checkResult)
    svc.firmware.downloadFirmwareToHost = jest.fn(async () => ({
      ok: false,
      error: 'download broken'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'download' })

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:start'
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'download:error',
      message: 'download broken'
    })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'download broken',
      raw: { err: -1, msg: 'download broken' }
    })
  })

  test('dongle-fw download emits progress and done when firmware download succeeds', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    const checkResult = {
      ok: true,
      hasUpdate: true,
      latestVer: '2.0.0',
      size: 222,
      token: 'tok',
      id: 'id1',
      notes: 'note',
      request: { foo: 'bar' },
      raw: { err: 0, ver: '2.0.0', size: 222 }
    }

    svc.firmware.checkForUpdate = jest.fn(async () => checkResult)
    svc.firmware.downloadFirmwareToHost = jest.fn(async (_check: unknown, opts?: any) => {
      opts?.onProgress?.({ received: 50, total: 100, percent: 0.5 })
      return {
        ok: true,
        path: '/tmp/fw.img',
        bytes: 100
      }
    })

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'download' })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'download:progress',
      received: 50,
      total: 100,
      percent: 0.5
    })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'download:done',
      path: '/tmp/fw.img',
      bytes: 100
    })

    expect(out).toEqual({
      ok: true,
      hasUpdate: true,
      size: 222,
      token: 'tok',
      request: { foo: 'bar' },
      raw: {
        err: 0,
        token: 'tok',
        ver: '2.0.0',
        size: 222,
        id: 'id1',
        notes: 'note',
        msg: undefined,
        error: undefined
      }
    })
  })

  test('dongle-fw upload returns error when projection is not started', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.started = false
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'upload' })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'Projection not started / dongle not connected',
      raw: { err: -1, msg: 'Projection not started / dongle not connected' }
    })

    expect(send).not.toHaveBeenCalledWith(
      'projection-event',
      expect.objectContaining({ stage: 'upload:start' })
    )
  })

  test('dongle-fw upload returns error when local firmware is not ready', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.started = true
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: false,
      reason: 'No firmware ready to upload'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'upload' })

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'upload:start'
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'upload:error',
      message: 'No firmware ready to upload'
    })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'No firmware ready to upload',
      raw: { err: -1, msg: 'No firmware ready to upload' }
    })
  })

  test('dongle-fw status returns local:not-ready shape', async () => {
    const svc = new ProjectionService() as any
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: false,
      reason: 'missing'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'status' })

    expect(out).toEqual({
      ok: true,
      hasUpdate: false,
      size: 0,
      token: undefined,
      request: {
        local: {
          ok: true,
          ready: false,
          reason: 'missing'
        }
      },
      raw: {
        err: 0,
        msg: 'local:not-ready'
      }
    })
  })

  test('dongle-fw status returns local:ready shape', async () => {
    const svc = new ProjectionService() as any
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/fw.img',
      bytes: 444,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'status' })

    expect(out).toEqual({
      ok: true,
      hasUpdate: true,
      size: 444,
      token: undefined,
      request: {
        local: {
          ok: true,
          ready: true,
          path: '/tmp/fw.img',
          bytes: 444,
          model: 'A15W',
          latestVer: '2.0.0'
        }
      },
      raw: {
        err: 0,
        ver: '2.0.0',
        size: 444,
        msg: 'local:ready'
      }
    })
  })

  test('dongle-fw returns unknown action error shape', async () => {
    const svc = new ProjectionService() as any
    svc.reloadConfigFromDisk = jest.fn(async () => undefined)

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'wat' })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'Unknown action: wat',
      raw: { err: -1, msg: 'Unknown action: wat' }
    })
  })
  test('driver failure event emits projection failure to renderer', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    svc.driver.emit('failure')

    expect(send).toHaveBeenCalledWith('projection-event', { type: 'failure' })
  })

  test('driver SoftwareVersion message updates fw version and emits dongle info', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    svc.driver.emit('message', new SoftwareVersion('2025.03.19.1126'))

    expect(svc.dongleFwVersion).toBe('2025.03.19.1126')
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'dongleInfo',
      payload: {
        dongleFwVersion: '2025.03.19.1126',
        boxInfo: undefined
      }
    })
  })

  test('driver BoxInfo message merges with existing info and emits dongle info', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.boxInfo = {
      uuid: 'u1',
      MFD: 'm1',
      productType: 'A15W',
      supportFeatures: 'wireless'
    }

    svc.driver.emit(
      'message',
      new BoxInfo({
        uuid: '',
        MFD: 'm1-new',
        productType: '',
        hwVersion: '2.0'
      })
    )

    expect(svc.boxInfo).toEqual({
      uuid: 'u1',
      MFD: 'm1-new',
      productType: 'A15W',
      supportFeatures: 'wireless',
      hwVersion: '2.0'
    })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'dongleInfo',
      payload: {
        dongleFwVersion: undefined,
        boxInfo: {
          uuid: 'u1',
          MFD: 'm1-new',
          productType: 'A15W',
          supportFeatures: 'wireless',
          hwVersion: '2.0'
        }
      }
    })
  })

  test('driver GnssData message forwards gnss payload', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    svc.driver.emit('message', new GnssData('$GPGGA,1'))

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'gnss',
      payload: { text: '$GPGGA,1' }
    })
  })

  test('driver BluetoothPairedList message forwards paired list when renderer is attached', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    svc.driver.emit('message', new BluetoothPairedList('Device A\nDevice B'))

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'bluetoothPairedList',
      payload: 'Device A\nDevice B'
    })
  })

  test('driver Plugged message emits requestSave, plugged event and starts projection', async () => {
    jest.useFakeTimers()

    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.started = false
    svc.isStarting = false
    svc.start = jest.fn(async () => undefined)
    svc.config = {
      language: 'en',
      phoneConfig: {
        [PhoneType.CarPlay]: {
          frameInterval: 250
        }
      }
    }

    svc.driver.emit('message', new Plugged(PhoneType.CarPlay))

    expect(svc.lastPluggedPhoneType).toBe(PhoneType.CarPlay)
    expect(svc.aaPlaybackInferred).toBe(1)
    expect((configEvents as any).emit).toHaveBeenCalledWith('requestSave', {
      lastPhoneWorkMode: PhoneWorkMode.CarPlay
    })
    expect(send).toHaveBeenCalledWith('projection-event', { type: 'plugged' })
    expect(svc.start).toHaveBeenCalledTimes(1)

    jest.useRealTimers()
  })

  test('driver Unplugged message emits unplugged, resets navigation and stops service', async () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.shuttingDown = false
    svc.stopping = false
    svc.stop = jest.fn(async () => undefined)
    svc.resetNavigationSnapshot = jest.fn()
    svc.lastPluggedPhoneType = PhoneType.AndroidAuto
    svc.aaPlaybackInferred = 2

    svc.driver.emit('message', new Unplugged())

    expect(svc.lastPluggedPhoneType).toBeUndefined()
    expect(svc.aaPlaybackInferred).toBe(1)
    expect(send).toHaveBeenCalledWith('projection-event', { type: 'unplugged' })
    expect(svc.resetNavigationSnapshot).toHaveBeenCalledWith('unplugged')
    expect(svc.stop).toHaveBeenCalledTimes(1)
  })

  test('driver BoxUpdateProgress message emits fw upload progress', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    svc.driver.emit('message', new BoxUpdateProgress(77))

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'upload:progress',
      progress: 77
    })
  })

  test('driver BoxUpdateState terminal success emits state, done and requests frame refresh', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.lastDongleInfoEmitKey = 'old-key'

    const msg = new BoxUpdateState()
    msg.status = 2
    msg.statusText = 'Update finished'
    msg.isOta = false
    msg.isTerminal = true
    msg.ok = true

    svc.driver.emit('message', msg)

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'upload:state',
      status: 2,
      statusText: 'Update finished',
      isOta: false,
      isTerminal: true,
      ok: true
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'upload:done',
      message: 'Update finished',
      status: 2,
      isOta: false
    })

    expect(svc.lastDongleInfoEmitKey).toBe('')
    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('driver BoxUpdateState terminal failure emits upload:error', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }

    const msg = new BoxUpdateState()
    msg.status = 3
    msg.statusText = 'Update failed'
    msg.isOta = false
    msg.isTerminal = true
    msg.ok = false

    svc.driver.emit('message', msg)

    expect(send).toHaveBeenNthCalledWith(1, 'projection-event', {
      type: 'fwUpdate',
      stage: 'upload:state',
      status: 3,
      statusText: 'Update failed',
      isOta: false,
      isTerminal: true,
      ok: false
    })

    expect(send).toHaveBeenNthCalledWith(2, 'projection-event', {
      type: 'fwUpdate',
      stage: 'upload:error',
      message: 'Update failed',
      status: 3,
      isOta: false
    })
  })

  test('driver Command message emits command event and requests navi focus when value is 508', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.mapsRequested = true

    const msg = new Command(508)
    svc.driver.emit('message', msg)

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'command',
      message: msg
    })
    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('driver AudioData emits audio and audioInfo once per unique decode format', () => {
    const svc = new ProjectionService() as any
    const send = jest.fn()
    svc.webContents = { send }
    svc.lastPluggedPhoneType = PhoneType.CarPlay
    ;(decodeTypeMap as any)[7] = {
      frequency: 48000,
      channel: 2,
      bitDepth: 16,
      format: 'pcm'
    }

    const msg = new AudioData()
    msg.command = 10
    msg.audioType = 4
    msg.decodeType = 7
    msg.volume = 0.75

    svc.driver.emit('message', msg)
    svc.driver.emit('message', msg)

    expect(svc.audio.handleAudioData).toHaveBeenCalledTimes(2)

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'audio',
      payload: {
        command: 10,
        audioType: 4,
        decodeType: 7,
        volume: 0.75
      }
    })

    expect(
      send.mock.calls.filter(
        ([channel, payload]) => channel === 'projection-event' && payload?.type === 'audioInfo'
      )
    ).toHaveLength(1)

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'audioInfo',
      payload: {
        codec: 'pcm',
        sampleRate: 48000,
        channels: 2,
        bitDepth: 16
      }
    })
  })
})
