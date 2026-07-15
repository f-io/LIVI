import { PhoneWorkMode } from '@shared/types'
import fs from 'fs'
import type { Mock } from 'vitest'
import {
  AudioData,
  BluetoothPairedList,
  BoxInfo,
  BoxUpdateProgress,
  BoxUpdateState,
  Command,
  decodeTypeMap,
  GnssData,
  PhoneType,
  Plugged,
  SoftwareVersion
} from '../../messages'

vi.mock('../../messages', async () => {
  const EventEmitter = require('events')
  class MockDongleDriver extends EventEmitter {
    send = vi.fn(async () => true)
    initialise = vi.fn(async () => undefined)
    start = vi.fn(async () => undefined)
    stop = vi.fn(async () => undefined)
    close = vi.fn(async () => undefined)
    bringUp = vi.fn(async () => undefined)
    isUp = false
    disconnectPhone = vi.fn(async () => true)
    sendPhoneAudio = vi.fn()
    uploadHostIcons = vi.fn()
    requestClusterFocus = vi.fn()
    requestKeyframe = vi.fn()
    sendBluetoothPairedList = vi.fn(async () => true)
    setPendingStartupConnectTarget = vi.fn()
    clearPendingStartupConnectTarget = vi.fn()
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
    MediaData: class MediaData {},
    NavigationData: class NavigationData {},
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
    MessageType: { ClusterVideoData: 0x2c },
    decodeTypeMap: {},
    DEFAULT_CONFIG: { apkVer: '1.0.0', language: 'en' }
  }
})

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: vi.fn(),
  registerIpcOn: vi.fn()
}))

vi.mock('../ProjectionAudio', () => ({
  ProjectionAudio: vi.fn().mockImplementation(function () {
    return {
      setInitialVolumes: vi.fn(),
      resetForSessionStart: vi.fn(),
      resetForSessionStop: vi.fn(),
      setStreamVolume: vi.fn(),
      setVisualizerEnabled: vi.fn(),
      handleAudioData: vi.fn()
    }
  })
}))

vi.mock('../FirmwareUpdateService', () => ({
  FirmwareUpdateService: vi.fn().mockImplementation(function () {
    return {
      checkForUpdate: vi.fn(async () => ({ ok: true, hasUpdate: false, raw: {} })),
      downloadFirmwareToHost: vi.fn(),
      getLocalFirmwareStatus: vi.fn()
    }
  })
}))

vi.mock('@main/ipc/utils', () => ({
  configEvents: {
    on: vi.fn(),
    off: vi.fn(),
    emit: vi.fn()
  }
}))

vi.mock('@shared/assets/carIcons', () => ({
  ICON_120_B64: '',
  ICON_180_B64: '',
  ICON_256_B64: ''
}))

vi.mock('electron', () => ({
  app: {
    getPath: vi.fn(() => '/tmp/appdata')
  },
  WebContents: class {},
  webContents: { fromId: vi.fn((id: number) => ({ id, isDestroyed: () => false })) }
}))

vi.mock('usb', () => ({
  usb: {
    getDevices: vi.fn(async () => [])
  }
}))

import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { configEvents } from '@main/ipc/utils'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { DEFAULT_MEDIA_DATA_RESPONSE, DEFAULT_NAVIGATION_DATA_RESPONSE } from '../constants'

describe('ProjectionService', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    vi.restoreAllMocks()
  })

  function getHandle<T = (...args: any[]) => any>(channel: string): T {
    const row = (registerIpcHandle as Mock).mock.calls.find(([ch]) => ch === channel)
    if (!row) throw new Error(`Missing ipc handle: ${channel}`)
    return row[1] as T
  }

  function getOn<T = (...args: any[]) => any>(channel: string): T {
    const row = (registerIpcOn as Mock).mock.calls.find(([ch]) => ch === channel)
    if (!row) throw new Error(`Missing ipc on: ${channel}`)
    return row[1] as T
  }

  test('registers IPC handlers and listeners in constructor', async () => {
    new ProjectionService()

    expect(registerIpcHandle).toHaveBeenCalled()
    expect(registerIpcOn).toHaveBeenCalled()
    expect((configEvents as any).on).toHaveBeenCalledWith('changed', expect.any(Function))
  })

  test('attachRenderer stores webContents reference', async () => {
    const svc = new ProjectionService() as any
    const wc = { send: vi.fn() }

    svc.attachRenderer(wc)

    expect(svc.webContents).toBe(wc)
  })

  test('applyConfigPatch merges incoming patch into config', async () => {
    const svc = new ProjectionService() as any
    svc.config = { language: 'en', kiosk: true }

    svc.applyConfigPatch({ language: 'de' })

    expect(svc.config).toEqual({ language: 'de', kiosk: true })
  })

  test('autoStartIfNeeded calls start when dongle is connected', async () => {
    const svc = new ProjectionService() as any
    svc.start = vi.fn(async () => undefined)

    svc.markDongleConnected(true)
    await svc.autoStartIfNeeded()

    expect(svc.start).toHaveBeenCalledTimes(1)
  })

  test('autoStartIfNeeded does nothing while shutting down', async () => {
    const svc = new ProjectionService() as any
    svc.start = vi.fn(async () => undefined)
    svc.shuttingDown = true

    svc.markDongleConnected(true)
    await svc.autoStartIfNeeded()

    expect(svc.start).not.toHaveBeenCalled()
  })

  test('autoStartIfNeeded does nothing when already started', async () => {
    const svc = new ProjectionService() as any
    svc.start = vi.fn(async () => undefined)
    svc.started = true

    svc.markDongleConnected(true)
    await svc.autoStartIfNeeded()

    expect(svc.start).not.toHaveBeenCalled()
  })

  test('beginShutdown marks service shutting down and unsubscribes config events', async () => {
    const svc = new ProjectionService() as any

    svc.beginShutdown()

    expect(svc.shuttingDown).toBe(true)
    expect((configEvents as any).off).toHaveBeenCalledWith('changed', expect.any(Function))
  })

  test('emitDongleInfoIfChanged emits only for new key', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
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

  test('emitDongleInfoIfChanged does nothing without renderer', async () => {
    const svc = new ProjectionService() as any
    svc.webContents = null
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { model: 'A15W' }

    expect(() => svc.emitDongleInfoIfChanged()).not.toThrow()
  })

  test('emitDongleInfoIfChanged emits again when key changes', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }

    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { model: 'A15W' }
    svc.emitDongleInfoIfChanged()

    svc.boxInfo = { model: 'A15X' }
    svc.emitDongleInfoIfChanged()

    expect(send).toHaveBeenCalledTimes(2)
  })

  test('getDevToolsUrlCandidates returns strict host/path combinations', async () => {
    const svc = new ProjectionService() as any

    const urls = svc.getDevToolsUrlCandidates()

    expect(urls).toEqual([
      'http://192.168.43.1/',
      'http://192.168.43.1/index.html',
      'http://192.168.43.1/cgi-bin/server.cgi?action=ls&path=/'
    ])
  })

  test('sendChunked does nothing without renderer', async () => {
    const svc = new ProjectionService() as any
    svc.webContents = null

    expect(() =>
      svc.sendChunked('projection-video-chunk', new Uint8Array([1, 2, 3]).buffer, 2)
    ).not.toThrow()
  })

  test('sendChunked does nothing when data is missing', async () => {
    const svc = new ProjectionService() as any
    svc.webContents = { send: vi.fn() }

    svc.sendChunked('projection-video-chunk', undefined, 2)

    expect(svc.webContents.send).not.toHaveBeenCalled()
  })

  test('sendChunked splits payload into envelopes', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
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

  test('reloadConfigFromDisk returns when file is missing', async () => {
    const svc = new ProjectionService() as any
    const existsSpy = vi.spyOn(fs, 'existsSync').mockReturnValue(false)

    svc.config = { language: 'en', apkVer: '1.0.0' }

    await svc.reloadConfigFromDisk()

    expect(svc.config).toEqual({ language: 'en', apkVer: '1.0.0' })
    existsSpy.mockRestore()
  })

  test('reloadConfigFromDisk merges config from disk', async () => {
    const svc = new ProjectionService() as any
    vi.spyOn(fs, 'existsSync').mockReturnValue(true)
    vi.spyOn(fs, 'readFileSync').mockReturnValue(
      JSON.stringify({ language: 'de', audioVolume: 0.3 }) as any
    )

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
    vi.spyOn(fs, 'existsSync').mockReturnValue(true)
    vi.spyOn(fs, 'readFileSync').mockReturnValue('{bad json' as any)

    svc.config = { language: 'en', apkVer: '1.0.0' }

    await expect(svc.reloadConfigFromDisk()).resolves.toBeUndefined()
    expect(svc.config).toEqual({ language: 'en', apkVer: '1.0.0' })
  })

  test('getApkVer returns current apk version from config', async () => {
    const svc = new ProjectionService() as any
    svc.config = { apkVer: '2.3.4' }

    expect(svc.getApkVer()).toBe('2.3.4')
  })

  test('markDongleConnected updates shared dongle connection state', async () => {
    vi.useFakeTimers()
    try {
      const svc = new ProjectionService() as any
      svc.start = vi.fn(async () => undefined)
      svc.markDongleConnected(false)
      vi.runOnlyPendingTimers() // flush detach debounce
      await svc.autoStartIfNeeded()
      expect(svc.start).not.toHaveBeenCalled()

      svc.markDongleConnected(true)
      await svc.autoStartIfNeeded()
      expect(svc.start).toHaveBeenCalledTimes(1)
    } finally {
      vi.runOnlyPendingTimers()
      vi.useRealTimers()
    }
  })

  describe('transport arbiter', () => {
    beforeEach(async () => {
      vi.useFakeTimers()
    })
    afterEach(async () => {
      vi.runOnlyPendingTimers()
      vi.useRealTimers()
    })

    function fakePhoneDevice(): any {
      return {
        deviceDescriptor: { idVendor: 0x18d1, idProduct: 0x2d00 }
      }
    }

    function freshSvc(): any {
      const svc = new ProjectionService() as any
      svc.markPhoneConnected(false)
      svc.markDongleConnected(false)
      vi.runOnlyPendingTimers() // flush detach debounces
      return svc
    }

    test('pickPreferredTransport returns null when nothing is detected', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }
      expect(svc.pickPreferredTransport()).toBeNull()
    })

    test('auto: dongle-only → dongle; phone-only → aa', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }

      svc.markDongleConnected(true)
      expect(svc.pickPreferredTransport()).toBe('dongle')

      svc.markDongleConnected(false)
      vi.runOnlyPendingTimers() // flush detach debounce
      svc.markPhoneConnected(true, fakePhoneDevice())
      expect(svc.pickPreferredTransport()).toBe('aa')
    })

    test('switchTransport is a no-op when only one transport is present', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }
      svc.markDongleConnected(true)
      svc.start = vi.fn(async () => undefined)
      svc.stop = vi.fn(async () => undefined)

      const res = await svc.switchTransport()
      expect(res.ok).toBe(false)
      expect(svc.stop).not.toHaveBeenCalled()
    })

    test('switchTransport restarts on the opposite transport when both are present', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }
      svc.markDongleConnected(true)
      svc.markPhoneConnected(true, fakePhoneDevice())
      svc.started = true
      // dongle is active by default — no AA driver was ever created
      svc.stop = vi.fn(async () => {
        svc.started = false
      })
      svc.start = vi.fn(async () => undefined)

      const res = await svc.switchTransport()
      expect(svc.stop).toHaveBeenCalledTimes(1)
      // override sticks → next pick is 'aa'
      expect(svc.pickPreferredTransport()).toBe('aa')
      expect(res.ok).toBe(true)
    })

    test('override clears when the chosen transport goes away', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }
      svc.markDongleConnected(true)
      svc.markPhoneConnected(true, fakePhoneDevice())
      svc.started = true
      // dongle is active by default — no AA driver was ever created
      svc.stop = vi.fn(async () => {
        svc.started = false
      })
      svc.start = vi.fn(async () => undefined)

      await svc.switchTransport()
      expect(svc.pickPreferredTransport()).toBe('aa')

      svc.markPhoneConnected(false)
      vi.runOnlyPendingTimers() // flush detach debounce
      expect(svc.pickPreferredTransport()).toBe('dongle')
    })
  })

  test('disconnectPhone returns false when service is not started', async () => {
    const svc = new ProjectionService() as any
    svc.started = false

    await expect(svc.disconnectPhone()).resolves.toBe(false)
  })

  test('disconnectPhone delegates to the driver and returns its result', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.driver.disconnectPhone = vi.fn(async () => true)

    await expect(svc.disconnectPhone()).resolves.toBe(true)
    expect(svc.driver.disconnectPhone).toHaveBeenCalledTimes(1)
  })

  test('disconnectPhone returns false when the driver reports failure', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.driver.disconnectPhone = vi.fn(async () => false)

    await expect(svc.disconnectPhone()).resolves.toBe(false)
    expect(svc.driver.disconnectPhone).toHaveBeenCalledTimes(1)
  })

  test('patchAaMediaPlayStatus writes media snapshot and emits projection event', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }

    vi.spyOn(fs, 'writeFileSync').mockImplementation(function () {})

    svc.mediaStore.patchAaPlayStatus({ media: null, nav: null }, 2)

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

  test('patchAaMediaPlayStatus swallows write errors', async () => {
    const svc = new ProjectionService() as any
    vi.spyOn(fs, 'writeFileSync').mockImplementation(function () {
      throw new Error('disk fail')
    })

    expect(() => svc.mediaStore.patchAaPlayStatus({ media: null, nav: null }, 1)).not.toThrow()
  })

  test('resetMediaSnapshot writes default media payload and emits reset event', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    vi.spyOn(fs, 'writeFileSync').mockImplementation(function () {})

    svc.mediaStore.reset('test')

    expect(fs.writeFileSync).toHaveBeenCalled()
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'media-reset',
      reason: 'test'
    })
  })

  test('resetNavigationSnapshot writes default navigation payload and emits reset event', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    vi.spyOn(fs, 'writeFileSync').mockImplementation(function () {})

    svc.navStore.reset('test')

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
    svc.disconnectPhone = vi.fn(async () => true)
    svc.driver.close = vi.fn(async () => undefined)
    svc.audio.resetForSessionStop = vi.fn()
    svc.clearTimeouts = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()

    await svc.stop()

    expect(svc.clearTimeouts).toHaveBeenCalled()
    expect(svc.disconnectPhone).toHaveBeenCalled()
    expect(svc.driver.close).toHaveBeenCalled()
    expect(svc.audio.resetForSessionStop).toHaveBeenCalled()
    expect(svc.started).toBe(false)
    expect(svc.lastDongleInfoEmitKey).toBe('')
  })

  test('stop resets btMacAddr from boxInfo when boxInfo is a record', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.stopping = false
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W', btMacAddr: 'AA:BB:CC' }
    svc.disconnectPhone = vi.fn(async () => false)
    svc.driver.close = vi.fn(async () => undefined)
    svc.audio.resetForSessionStop = vi.fn()
    svc.clearTimeouts = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()

    await svc.stop()

    expect(svc.boxInfo.btMacAddr).toBe('')
  })

  test('stop closes the driver and marks service stopped', async () => {
    const svc = new ProjectionService() as any
    svc.started = true
    svc.stopping = false
    svc.disconnectPhone = vi.fn(async () => false)
    svc.driver.close = vi.fn(async () => undefined)
    svc.audio.resetForSessionStop = vi.fn()
    svc.clearTimeouts = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()

    await svc.stop()

    expect(svc.driver.close).toHaveBeenCalled()
    expect(svc.started).toBe(false)
  })

  test('stop swallows driver.close errors', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(function () {})

    const svc = new ProjectionService() as any
    svc.started = true
    svc.stopping = false
    svc.disconnectPhone = vi.fn(async () => false)
    svc.driver.close = vi.fn(async () => {
      throw new Error('close failed')
    })
    svc.audio.resetForSessionStop = vi.fn()
    svc.clearTimeouts = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()

    await expect(svc.stop()).resolves.toBeUndefined()
    expect(warnSpy).toHaveBeenCalledWith(
      '[ProjectionService] dongle close() failed (ignored)',
      expect.any(Error)
    )
  })

  test('projection-start handler delegates to start', async () => {
    const svc = new ProjectionService() as any
    svc.start = vi.fn(async () => undefined)

    const h = getHandle('projection-start').bind(svc)
    await h()

    expect(svc.start).toHaveBeenCalledTimes(1)
  })

  test('projection-stop handler delegates to stop', async () => {
    const svc = new ProjectionService() as any
    svc.stop = vi.fn(async () => undefined)

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
    svc.driver.sendBluetoothPairedList = vi.fn(async () => true)
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
    svc.drivers.getDongle().isUp = true
    svc.uploadIcons = vi.fn()
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
    svc.drivers.getDongle().isUp = true
    svc.driver.send = vi.fn().mockResolvedValueOnce(true).mockResolvedValueOnce(false)

    const h = getHandle('projection-upload-livi-scripts')
    const result = await h.call(svc)

    expect(result.cgiOk).toBe(true)
    expect(result.webOk).toBe(false)
    expect(result.ok).toBe(false)
    expect(result.urls).toHaveLength(3)
  })

  test('cluster:request disables cluster and clears cached resolution', async () => {
    const svc = new ProjectionService() as any
    svc.lastClusterVideoWidth = 123
    svc.lastClusterVideoHeight = 456

    const h = getHandle('cluster:request')
    await expect(h.call(svc, { sender: { id: 1 } }, false)).resolves.toEqual({
      ok: true,
      enabled: false
    })

    expect(svc.clusterRequestedBy.size).toBe(0)
    expect(svc.lastClusterVideoWidth).toBeUndefined()
    expect(svc.lastClusterVideoHeight).toBeUndefined()
  })

  test('cluster:request enables cluster and requests focus when at least one display targets it', async () => {
    const svc = new ProjectionService() as any
    svc.config = {
      ...svc.config,
      dashboards: { dash3: { main: true, dash: false, aux: false } }
    }
    const h = getHandle('cluster:request')

    await expect(h.call(svc, { sender: { id: 1 } }, true)).resolves.toEqual({
      ok: true,
      enabled: true
    })

    expect(svc.clusterRequestedBy.size).toBe(1)
    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('cluster:request refuses to enable cluster when no display targets it', async () => {
    const svc = new ProjectionService() as any
    svc.config = {
      ...svc.config,
      dashboards: { dash3: { main: false, dash: false, aux: false } }
    }
    const h = getHandle('cluster:request')

    await expect(h.call(svc, { sender: { id: 1 } }, true)).resolves.toEqual({
      ok: true,
      enabled: false
    })

    expect(svc.clusterRequestedBy.size).toBe(0)
    expect(svc.driver.send).not.toHaveBeenCalled()
  })

  test('projection-touch forwards touch payload as message', async () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-touch')

    on.call(svc, null, { x: 1, y: 2, action: 3 })

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-multi-touch ignores empty arrays', async () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-multi-touch')

    on.call(svc, null, [])

    expect(svc.driver.send).not.toHaveBeenCalled()
  })

  test('projection-multi-touch sanitizes points and sends message', async () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-multi-touch')

    on.call(svc, null, [{ id: 3.9, x: -1, y: 2, action: 7.8 }])

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-command forwards command message', async () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-command')

    on.call(svc, null, 'frame')

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-set-volume delegates to ProjectionAudio', async () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-set-volume')

    on.call(svc, null, { stream: 'music', volume: 0.5 })

    expect(svc.audio.setStreamVolume).toHaveBeenCalledWith('music', 0.5)
  })

  test('projection-set-visualizer-enabled delegates to ProjectionAudio', async () => {
    const svc = new ProjectionService() as any
    const on = getOn('projection-set-visualizer-enabled')

    on.call(svc, null, 1)

    expect(svc.audio.setVisualizerEnabled).toHaveBeenCalledWith(true, undefined)
  })

  test('projection-media-read returns the default payload when there is no active session', async () => {
    const svc = new ProjectionService() as any
    vi.spyOn(svc.sessions, 'active').mockReturnValue(null)

    const out = await getHandle('projection-media-read').call(svc)

    expect(typeof out.timestamp).toBe('string')
    expect(out.payload).toEqual(DEFAULT_MEDIA_DATA_RESPONSE.payload)
  })

  test('projection-media-read returns the active session media snapshot', async () => {
    const svc = new ProjectionService() as any
    const media = { type: 1, media: { MediaSongName: 'Song' } }
    vi.spyOn(svc.sessions, 'active').mockReturnValue({ media, nav: null } as any)

    const out = await getHandle('projection-media-read').call(svc)

    expect(out.payload).toEqual(media)
  })

  test('projection-navigation-read returns the default payload when there is no active session', async () => {
    const svc = new ProjectionService() as any
    vi.spyOn(svc.sessions, 'active').mockReturnValue(null)

    const out = await getHandle('projection-navigation-read').call(svc)

    expect(typeof out.timestamp).toBe('string')
    expect(out.payload).toEqual(DEFAULT_NAVIGATION_DATA_RESPONSE.payload)
  })

  test('projection-navigation-read returns the active session navigation snapshot', async () => {
    const svc = new ProjectionService() as any
    const nav = { metaType: 200, navi: null }
    vi.spyOn(svc.sessions, 'active').mockReturnValue({ media: null, nav } as any)

    const out = await getHandle('projection-navigation-read').call(svc)

    expect(out.payload).toEqual(nav)
  })

  test('uploadIcons reloads disk config and sends 3 icon files', async () => {
    const svc = new ProjectionService() as any
    vi.spyOn(fs, 'existsSync').mockReturnValue(true)
    vi.spyOn(fs, 'readFileSync').mockReturnValue(
      JSON.stringify({
        dongleIcon120: Buffer.from('120').toString('base64'),
        dongleIcon180: Buffer.from('180').toString('base64'),
        dongleIcon256: Buffer.from('256').toString('base64')
      }) as any
    )

    svc.uploadIcons()

    expect(svc.driver.uploadHostIcons).toHaveBeenCalledTimes(1)
  })

  test('uploadIcons cancels when icon fields are missing', async () => {
    const svc = new ProjectionService() as any
    vi.spyOn(fs, 'existsSync').mockReturnValue(true)
    vi.spyOn(fs, 'readFileSync').mockReturnValue(JSON.stringify({ dongleIcon120: 'abc' }) as any)

    svc.uploadIcons()

    expect(svc.driver.send).not.toHaveBeenCalled()
  })

  test('dongle-fw check emits start/done events and returns shaped success result', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
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
    const send = vi.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
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
    svc.webContents = { send: vi.fn() }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
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
    const send = vi.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
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

    svc.firmware.downloadFirmwareToHost = vi.fn(async (_check: any, opts: any) => {
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
    const send = vi.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
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

    svc.firmware.downloadFirmwareToHost = vi.fn()

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
    const send = vi.fn()
    svc.webContents = { send }
    svc.config = { apkVer: '9.9.9' }
    svc.dongleFwVersion = '1.0.0'
    svc.boxInfo = { uuid: 'u1', MFD: 'm1', productType: 'A15W' }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
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

    svc.firmware.downloadFirmwareToHost = vi.fn()

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
    const send = vi.fn()
    svc.webContents = { send }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
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
    const send = vi.fn()
    svc.webContents = { send }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

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

    svc.firmware.checkForUpdate = vi.fn(async () => checkResult)
    svc.firmware.downloadFirmwareToHost = vi.fn(async () => ({
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
    const send = vi.fn()
    svc.webContents = { send }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

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

    svc.firmware.checkForUpdate = vi.fn(async () => checkResult)
    svc.firmware.downloadFirmwareToHost = vi.fn(async (_check: unknown, opts?: any) => {
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
    const send = vi.fn()
    svc.webContents = { send }
    svc.started = false
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

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

  test('dongle-fw upload returns error when local firmware status has ok:false', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.started = true
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
      ok: false,
      error: 'status check failed'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'upload' })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'upload:error',
      message: 'status check failed'
    })

    expect(out).toEqual({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: 'status check failed',
      raw: { err: -1, msg: 'status check failed' }
    })
  })

  test('dongle-fw upload sends firmware file to dongle and returns success', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.started = true
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/appdata/firmware/A15W_Update.img',
      bytes: 100,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    vi.spyOn(fs.promises, 'readFile').mockResolvedValue(Buffer.alloc(100) as any)
    svc.driver.send = vi.fn(async () => true)

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'upload' })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'upload:file-sent',
      path: '/tmp/A15W_Update.img',
      bytes: 100
    })

    expect(out).toEqual(
      expect.objectContaining({
        ok: true,
        hasUpdate: true,
        size: 100
      })
    )
  })

  test('dongle-fw upload returns error when SendFile returns false', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.started = true
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/appdata/firmware/A15W_Update.img',
      bytes: 100,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    vi.spyOn(fs.promises, 'readFile').mockResolvedValue(Buffer.alloc(100) as any)
    svc.driver.send = vi.fn(async () => false)

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'upload' })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'upload:error',
      message: 'Dongle upload failed (SendFile returned false)'
    })

    expect(out).toEqual(
      expect.objectContaining({
        ok: false,
        error: 'Dongle upload failed (SendFile returned false)'
      })
    )
  })

  test('dongle-fw upload catches thrown errors and emits upload:error', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.started = true
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/appdata/firmware/A15W_Update.img',
      bytes: 100,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    vi.spyOn(fs.promises, 'readFile').mockRejectedValue(new Error('read error'))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'upload' })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'upload:error',
      message: 'read error'
    })

    expect(out).toEqual(expect.objectContaining({ ok: false, error: 'read error' }))
  })

  test('dongle-fw download catches thrown exceptions and emits download:error', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.checkForUpdate = vi.fn(async () => ({
      ok: true,
      hasUpdate: true,
      latestVer: '2.0.0',
      size: 100,
      token: 'tok',
      request: { foo: 'bar' },
      raw: { err: 0 }
    }))

    svc.firmware.downloadFirmwareToHost = vi.fn(async () => {
      throw new Error('disk full')
    })

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'download' })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'download:error',
      message: 'disk full'
    })

    expect(out).toEqual(expect.objectContaining({ ok: false, error: 'disk full' }))
  })

  test('dongle-fw status returns error shape when getLocalFirmwareStatus returns null', async () => {
    const svc = new ProjectionService() as any
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => null)

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'status' })

    expect(out).toEqual(
      expect.objectContaining({ ok: false, error: 'Local firmware status failed' })
    )
  })

  test('dongle-fw status returns error shape when status ok is false', async () => {
    const svc = new ProjectionService() as any
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
      ok: false,
      error: 'status error'
    }))

    const h = getHandle('dongle-fw')
    const out = await h.call(svc, null, { action: 'status' })

    expect(out).toEqual(expect.objectContaining({ ok: false, error: 'status error' }))
  })

  test('dongle-fw upload returns error when local firmware is not ready', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.started = true
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
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
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
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
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

    svc.firmware.getLocalFirmwareStatus = vi.fn(async () => ({
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
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)

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
  test('driver failure event emits projection failure to renderer', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }

    svc.driver.emit('failure')

    expect(send).toHaveBeenCalledWith('projection-event', { type: 'failure' })
  })

  test('driver SoftwareVersion message updates fw version and emits dongle info', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
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

  test('driver BoxInfo message merges with existing info and emits dongle info', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
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

  test('driver GnssData message forwards gnss payload', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }

    svc.driver.emit('message', new GnssData('$GPGGA,1'))

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'gnss',
      payload: { text: '$GPGGA,1' }
    })
  })

  test('driver BluetoothPairedList message forwards paired list when renderer is attached', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }

    const raw = 'AA:BB:CC:DD:EE:FFDevice A\n11:22:33:44:55:66Device B\n'
    svc.driver.emit('message', new BluetoothPairedList(raw))

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'bluetoothPairedList',
      payload: raw
    })
  })

  test('driver Plugged message emits requestSave, plugged event and starts projection', async () => {
    vi.useFakeTimers()

    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.started = false
    svc.isStarting = false
    svc.start = vi.fn(async () => undefined)
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
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'plugged',
      phoneType: PhoneType.CarPlay
    })
    expect(svc.start).toHaveBeenCalledTimes(1)

    vi.useRealTimers()
  })

  test('dongle phone-disconnected emits unplugged and resets navigation', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.navStore.reset = vi.fn()
    svc.lastPluggedPhoneType = PhoneType.AndroidAuto
    svc.aaPlaybackInferred = 2

    svc.drivers.getDongle().emit('phone-disconnected')

    expect(svc.lastPluggedPhoneType).toBeUndefined()
    expect(svc.aaPlaybackInferred).toBe(1)
    expect(send).toHaveBeenCalledWith('projection-event', { type: 'unplugged' })
    expect(svc.navStore.reset).toHaveBeenCalledWith('phone-disconnect')
  })

  test('driver BoxUpdateProgress message emits fw upload progress', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }

    svc.driver.emit('message', new BoxUpdateProgress(77))

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'upload:progress',
      progress: 77
    })
  })

  test('driver BoxUpdateState terminal success emits state, done and requests frame refresh', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
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
    expect(svc.driver.requestKeyframe).toHaveBeenCalledTimes(1)
  })

  test('driver BoxUpdateState terminal failure emits upload:error', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
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

  test('driver Command message emits command event and requests navi focus when value is 508', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }
    svc.clusterRequestedBy.add(1)

    const msg = new Command(508)
    svc.driver.emit('message', msg)

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'command',
      message: msg
    })
    expect(svc.driver.requestClusterFocus).toHaveBeenCalledTimes(1)
  })

  test('uploadIcons logs warning when config.json reload throws', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(function () {})
    const svc = new ProjectionService() as any
    vi.spyOn(fs, 'existsSync').mockReturnValue(true)
    vi.spyOn(fs, 'readFileSync').mockImplementation(function () {
      throw new Error('parse error')
    })
    svc.config = {
      dongleIcon120: Buffer.from('120').toString('base64'),
      dongleIcon180: Buffer.from('180').toString('base64'),
      dongleIcon256: Buffer.from('256').toString('base64')
    }

    svc.uploadIcons()

    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('failed to reload config.json'),
      expect.any(Error)
    )
    expect(svc.driver.uploadHostIcons).toHaveBeenCalledTimes(1)
  })

  test('uploadIcons swallows errors in outer catch', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(function () {})
    const svc = new ProjectionService() as any
    vi.spyOn(fs, 'existsSync').mockReturnValue(true)
    vi.spyOn(fs, 'readFileSync').mockReturnValue(
      JSON.stringify({
        dongleIcon120: Buffer.from('120').toString('base64'),
        dongleIcon180: Buffer.from('180').toString('base64'),
        dongleIcon256: Buffer.from('256').toString('base64')
      }) as any
    )
    svc.driver.uploadHostIcons = vi.fn(function () {
      throw new Error('send failed')
    })

    expect(() => svc.uploadIcons()).not.toThrow()
    expect(errorSpy).toHaveBeenCalledWith(
      '[ProjectionService] failed to upload icons',
      expect.any(Error)
    )
  })

  test('emitDongleInfoIfChanged uses String(boxInfo) when JSON.stringify throws', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
    svc.webContents = { send }

    const circular: Record<string, unknown> = {}
    circular.self = circular
    svc.boxInfo = circular

    expect(() => svc.emitDongleInfoIfChanged()).not.toThrow()
    expect(send).toHaveBeenCalledTimes(1)
  })

  test('driver AudioData emits audio and audioInfo once per unique decode format', async () => {
    const svc = new ProjectionService() as any
    const send = vi.fn()
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
