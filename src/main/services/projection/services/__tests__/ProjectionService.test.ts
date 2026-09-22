import { EventEmitter } from 'node:events'
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
  PhoneType,
  Plugged,
  SoftwareVersion
} from '../../messages'

vi.mock('../../messages', async () => {
  class StubMsg {
    constructor(
      public value?: unknown,
      public value2?: unknown
    ) {}
  }

  return {
    Plugged: class {
      constructor(public phoneType?: number) {}
    },
    Unplugged: class {},
    PhoneType: { CarPlay: 3, AndroidAuto: 5 },
    BluetoothPairedList: class {
      constructor(public data?: unknown) {}
    },
    BluetoothPeerConnected: class {
      constructor(public address?: string) {}
    },
    AudioData: class {},
    DuckAudio: class {},
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

import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { configEvents } from '@main/ipc/utils'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { DEFAULT_MEDIA_DATA_RESPONSE, DEFAULT_NAVIGATION_DATA_RESPONSE } from '../constants'

// getActive() is null with no session routed; tests that drive the active driver route a mock.
function routeMockDriver(svc: any): any {
  const d: any = Object.assign(new EventEmitter(), {
    requestClusterFocus: vi.fn(),
    requestKeyframe: vi.fn(),
    handleInput: vi.fn(),
    disconnectPhone: vi.fn(async () => true),
    close: vi.fn(async () => undefined),
    send: vi.fn(async () => true),
    setStreamVolume: vi.fn(),
    uploadHostIcons: vi.fn()
  })
  svc.drivers.route(d)
  return d
}

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
    routeMockDriver(svc)
    const wc = { send: vi.fn() }

    svc.attachRenderer(wc)

    expect(svc.webContents).toBe(wc)
  })

  test('applyConfigPatch merges incoming patch into config', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.config = { language: 'en', kiosk: true }

    svc.applyConfigPatch({ language: 'de' })

    expect(svc.config).toEqual({ language: 'de', kiosk: true })
  })

  test('autoStartIfNeeded does nothing while shutting down', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.start = vi.fn(async () => undefined)
    svc.shuttingDown = true

    await svc.autoStartIfNeeded()

    expect(svc.start).not.toHaveBeenCalled()
  })

  test('autoStartIfNeeded does nothing when already started', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.start = vi.fn(async () => undefined)
    svc.started = true

    await svc.autoStartIfNeeded()

    expect(svc.start).not.toHaveBeenCalled()
  })

  test('beginShutdown marks service shutting down and unsubscribes config events', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)

    svc.beginShutdown()

    expect(svc.shuttingDown).toBe(true)
    expect((configEvents as any).off).toHaveBeenCalledWith('changed', expect.any(Function))
  })

  test('sendChunked does nothing without renderer', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.webContents = null

    expect(() =>
      svc.sendChunked('projection-video-chunk', new Uint8Array([1, 2, 3]).buffer, 2)
    ).not.toThrow()
  })

  test('sendChunked does nothing when data is missing', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.webContents = { send: vi.fn() }

    svc.sendChunked('projection-video-chunk', undefined, 2)

    expect(svc.webContents.send).not.toHaveBeenCalled()
  })

  test('sendChunked splits payload into envelopes', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
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
    routeMockDriver(svc)
    const existsSpy = vi.spyOn(fs, 'existsSync').mockReturnValue(false)

    svc.config = { language: 'en', apkVer: '1.0.0' }

    await svc.reloadConfigFromDisk()

    expect(svc.config).toEqual({ language: 'en', apkVer: '1.0.0' })
    existsSpy.mockRestore()
  })

  test('reloadConfigFromDisk merges config from disk', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
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
    routeMockDriver(svc)
    vi.spyOn(fs, 'existsSync').mockReturnValue(true)
    vi.spyOn(fs, 'readFileSync').mockReturnValue('{bad json' as any)

    svc.config = { language: 'en', apkVer: '1.0.0' }

    await expect(svc.reloadConfigFromDisk()).resolves.toBeUndefined()
    expect(svc.config).toEqual({ language: 'en', apkVer: '1.0.0' })
  })

  describe('transport arbiter', () => {
    beforeEach(async () => {
      vi.useFakeTimers()
    })
    afterEach(async () => {
      vi.runOnlyPendingTimers()
      vi.useRealTimers()
    })

    // The helper announced a phone on USB, the session stays held until activated
    function plugWiredAa(svc: any): number {
      const driver = Object.assign(new EventEmitter(), {
        isWiredMode: () => true,
        usbSerial: () => 'serial-1',
        close: vi.fn(async () => undefined),
        requestKeyframe: vi.fn(),
        setVideoActive: vi.fn(),
        send: vi.fn(async () => true)
      })
      return svc.sessions.upsert(driver, 'androidauto', 'usb', { usbSerial: 'serial-1' }).index
    }

    function freshSvc(): any {
      const svc = new ProjectionService() as any
      routeMockDriver(svc)
      routeMockDriver(svc)
      vi.runOnlyPendingTimers() // flush detach debounce
      return svc
    }

    test('pickPreferredTransport returns null when nothing is detected', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }
      expect(svc.pickPreferredTransport()).toBeNull()
    })

    test('switchTransport is a no-op when only one transport is present', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }
      svc.start = vi.fn(async () => undefined)
      svc.stop = vi.fn(async () => undefined)

      const res = await svc.switchTransport()
      expect(res.ok).toBe(false)
      expect(svc.stop).not.toHaveBeenCalled()
    })

    test('override clears when the chosen transport goes away', async () => {
      const svc = freshSvc()
      svc.config = { aa: false, connectionPreference: 'auto' }
      const wired = plugWiredAa(svc)
      svc.started = true
      svc.stop = vi.fn(async () => {
        svc.started = false
      })
      svc.start = vi.fn(async () => undefined)

      await svc.switchTransport()
      expect(svc.pickPreferredTransport()).toBe('aa')

      // The wired candidate follows the helper session, no detach debounce
      svc.sessions.close(wired)
    })
  })

  test('disconnectPhone returns false when service is not started', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.started = false

    await expect(svc.disconnectPhone()).resolves.toBe(false)
  })

  test('disconnectPhone delegates to the driver and returns its result', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.started = true
    svc.driver.disconnectPhone = vi.fn(async () => true)

    await expect(svc.disconnectPhone()).resolves.toBe(true)
    expect(svc.driver.disconnectPhone).toHaveBeenCalledTimes(1)
  })

  test('disconnectPhone returns false when the driver reports failure', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.started = true
    svc.driver.disconnectPhone = vi.fn(async () => false)

    await expect(svc.disconnectPhone()).resolves.toBe(false)
    expect(svc.driver.disconnectPhone).toHaveBeenCalledTimes(1)
  })

  test('patchAaMediaPlayStatus writes media snapshot and emits projection event', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const send = vi.fn()
    svc.webContents = { send }

    vi.spyOn(fs, 'writeFileSync').mockImplementation(function () {})

    svc.mediaStore.patchAaPlayStatus({ media: null, nav: null }, 2)

    expect(fs.writeFileSync).toHaveBeenCalled()
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'media',
      payload: {
        payload: {
          type: 1,
          media: {
            MediaSongName: '-',
            MediaAlbumName: '-',
            MediaArtistName: '-',
            MediaAPPName: '-',
            MediaSongDuration: 0,
            MediaSongPlayTime: 0,
            MediaPlayStatus: 2,
            MediaLyrics: '-'
          }
        }
      }
    })
  })

  test('patchAaMediaPlayStatus swallows write errors', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    vi.spyOn(fs, 'writeFileSync').mockImplementation(function () {
      throw new Error('disk fail')
    })

    expect(() => svc.mediaStore.patchAaPlayStatus({ media: null, nav: null }, 1)).not.toThrow()
  })

  test('resetMediaSnapshot writes default media payload and emits reset event', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
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
    routeMockDriver(svc)
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
    routeMockDriver(svc)

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
    routeMockDriver(svc)
    svc.started = true
    svc.stopping = false
    svc.disconnectPhone = vi.fn(async () => true)
    svc.audio.resetForSessionStop = vi.fn()
    svc.clearTimeouts = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()

    await svc.stop()

    expect(svc.clearTimeouts).toHaveBeenCalled()
    expect(svc.disconnectPhone).toHaveBeenCalled()
    expect(svc.audio.resetForSessionStop).toHaveBeenCalled()
    expect(svc.started).toBe(false)
  })

  test('stop closes the driver and marks service stopped', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.started = true
    svc.stopping = false
    svc.disconnectPhone = vi.fn(async () => false)
    svc.audio.resetForSessionStop = vi.fn()
    svc.clearTimeouts = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()

    await svc.stop()

    expect(svc.started).toBe(false)
  })

  test('projection-start handler delegates to start', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.start = vi.fn(async () => undefined)

    const h = getHandle('projection-start').bind(svc)
    await h()

    expect(svc.start).toHaveBeenCalledTimes(1)
  })

  test('projection-stop handler delegates to stop', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    svc.stop = vi.fn(async () => undefined)

    const h = getHandle('projection-stop').bind(svc)
    await h()

    expect(svc.stop).toHaveBeenCalledTimes(1)
  })

  test('projection-sendframe handler sends frame command', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const h = getHandle('projection-sendframe')

    await h.call(svc)

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('cluster:request disables cluster and clears cached resolution', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
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
    routeMockDriver(svc)
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
    routeMockDriver(svc)
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
    routeMockDriver(svc)
    const on = getOn('projection-touch')

    on.call(svc, null, { x: 1, y: 2, action: 3 })

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-multi-touch ignores empty arrays', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const on = getOn('projection-multi-touch')

    on.call(svc, null, [])

    expect(svc.driver.send).not.toHaveBeenCalled()
  })

  test('projection-multi-touch sanitizes points and sends message', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const on = getOn('projection-multi-touch')

    on.call(svc, null, [{ id: 3.9, x: -1, y: 2, action: 7.8 }])

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-command forwards command message', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const on = getOn('projection-command')

    on.call(svc, null, 'frame')

    expect(svc.driver.send).toHaveBeenCalledTimes(1)
  })

  test('projection-set-volume delegates to ProjectionAudio', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const on = getOn('projection-set-volume')

    on.call(svc, null, { stream: 'music', volume: 0.5 })

    expect(svc.audio.setStreamVolume).toHaveBeenCalledWith('music', 0.5)
  })

  test('a stream level reaches the driver that plays it out', async () => {
    const { ProjectionAudio } = await import('../ProjectionAudio')
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const applyStreamVolume = vi.mocked(ProjectionAudio).mock.calls.at(-1)?.[3] as (
      audioType: number,
      level: number,
      rampMs: number
    ) => void

    const withVolume = { setStreamVolume: vi.fn() }
    svc.drivers.getActive = vi.fn(() => withVolume)
    applyStreamVolume(3, 0.5, 250)
    expect(withVolume.setStreamVolume).toHaveBeenCalledWith(3, 0.5, 250)

    svc.drivers.getActive = vi.fn(() => ({}))
    expect(() => applyStreamVolume(3, 0.5, 0)).not.toThrow()
  })

  test('projection-set-visualizer-enabled delegates to ProjectionAudio', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const on = getOn('projection-set-visualizer-enabled')

    on.call(svc, null, 1)

    expect(svc.audio.setVisualizerEnabled).toHaveBeenCalledWith(true, undefined)
  })

  test('projection-media-read returns the default payload when there is no active session', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    vi.spyOn(svc.sessions, 'active').mockReturnValue(null)

    const out = await getHandle('projection-media-read').call(svc)

    expect(typeof out.timestamp).toBe('string')
    expect(out.payload).toEqual(DEFAULT_MEDIA_DATA_RESPONSE.payload)
  })

  test('projection-media-read returns the active session media snapshot', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const media = { type: 1, media: { MediaSongName: 'Song' } }
    vi.spyOn(svc.sessions, 'active').mockReturnValue({ media, nav: null } as any)

    const out = await getHandle('projection-media-read').call(svc)

    expect(out.payload).toEqual(media)
  })

  test('projection-navigation-read returns the default payload when there is no active session', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    vi.spyOn(svc.sessions, 'active').mockReturnValue(null)

    const out = await getHandle('projection-navigation-read').call(svc)

    expect(typeof out.timestamp).toBe('string')
    expect(out.payload).toEqual(DEFAULT_NAVIGATION_DATA_RESPONSE.payload)
  })

  test('projection-navigation-read returns the active session navigation snapshot', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const nav = { metaType: 200, navi: null }
    vi.spyOn(svc.sessions, 'active').mockReturnValue({ media: null, nav } as any)

    const out = await getHandle('projection-navigation-read').call(svc)

    expect(out.payload).toEqual(nav)
  })

  test('driver failure event emits projection failure to renderer', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
    const send = vi.fn()
    svc.webContents = { send }

    svc.driver.emit('failure')

    expect(send).toHaveBeenCalledWith('projection-event', { type: 'failure' })
  })

  test('driver Command message emits command event and requests navi focus when value is 508', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
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

  test('driver AudioData emits audio and audioInfo once per unique decode format', async () => {
    const svc = new ProjectionService() as any
    routeMockDriver(svc)
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

    svc.driver.emit('message', msg)
    svc.driver.emit('message', msg)

    expect(svc.audio.handleAudioData).toHaveBeenCalledTimes(2)

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'audio',
      payload: {
        command: 10,
        audioType: 4,
        decodeType: 7
      }
    })

    expect(
      send.mock.calls.filter(
        ([channel, payload]) => channel === 'projection-event' && payload?.type === 'audioInfo'
      )
    ).toHaveLength(1)

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'audioInfo',
      payload: { sampleRate: 48000 }
    })
  })
})
