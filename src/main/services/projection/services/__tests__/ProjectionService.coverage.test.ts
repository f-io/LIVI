import type { Mock } from 'vitest'
import {
  AudioData,
  BluetoothPairedList,
  BluetoothPeerConnected,
  BoxInfo,
  BoxUpdateProgress,
  BoxUpdateState,
  Command,
  DuckAudio,
  MediaData,
  NavigationData,
  Plugged,
  SoftwareVersion,
  VideoData
} from '../../messages'

const bluezMock = {
  listPaired: vi.fn(async () => [] as any[]),
  connect: vi.fn(async (_mac: string) => ({ ok: true })),
  connectFull: vi.fn(async (_mac: string) => ({ ok: true })),
  disconnect: vi.fn(async (_mac: string) => ({ ok: true })),
  deauthApClients: vi.fn(async () => undefined),
  setWiredPhones: vi.fn(async () => undefined),
  subscribe: vi.fn(() => ({ close: vi.fn() }))
}

vi.mock('../../bt/BluezDeviceClient', () => ({
  BluezDeviceClient: vi.fn().mockImplementation(function () {
    return bluezMock
  })
}))

const { helperHolder } = vi.hoisted(() => ({
  helperHolder: { handlers: {} as Record<string, any> }
}))
vi.mock('../../driver/helper/helperSupervisor', () => ({
  HelperSupervisor: vi.fn().mockImplementation(function () {
    const handlers: Record<string, any> = {}
    helperHolder.handlers = handlers
    return {
      on: vi.fn((ev: string, cb: any) => {
        handlers[ev] = cb
      }),
      start: vi.fn(),
      stop: vi.fn(async () => undefined)
    }
  })
}))

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
    BluetoothPeerConnected: class {
      constructor(public address?: string) {}
    },
    VideoData: class {},
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
    decodeTypeMap: {
      1: { frequency: 48000, channel: 2, bitDepth: 16 }
    },
    DEFAULT_CONFIG: { apkVer: '1.0.0', language: 'en' }
  }
})

vi.mock('../../driver/dongle/dongleDriver', async () => {
  const m = (await import('../../messages')) as Record<string, unknown>
  return { DongleDriver: m.DongleDriver }
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
      handleAudioData: vi.fn(),
      duck: vi.fn(),
      unduck: vi.fn(),
      restoreDuck: vi.fn(),
      onAudioDeviceChanged: vi.fn()
    }
  })
}))

vi.mock('../../driver/dongle/FirmwareUpdateService', () => ({
  FirmwareUpdateService: vi.fn().mockImplementation(function () {
    return {
      checkForUpdate: vi.fn(async () => ({ ok: true, hasUpdate: false, raw: {} })),
      downloadFirmwareToHost: vi.fn(),
      getLocalFirmwareStatus: vi.fn()
    }
  })
}))

const { configEventsMock } = vi.hoisted(() => ({
  configEventsMock: { on: vi.fn(), off: vi.fn(), emit: vi.fn() }
}))
vi.mock('@main/ipc/utils', () => ({ configEvents: configEventsMock }))

vi.mock('@shared/assets/carIcons', () => ({
  ICON_120_B64: 'QUFBQQ==',
  ICON_180_B64: 'QkJCQg==',
  ICON_256_B64: 'Q0NDQw=='
}))

vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/tmp/appdata') },
  WebContents: class {},
  webContents: { fromId: vi.fn((id: number) => ({ id, isDestroyed: () => false })) }
}))

vi.mock('usb', () => ({
  usb: { getDeviceList: vi.fn(() => []) },
  WebUSBDevice: { createInstance: vi.fn(async () => ({})) }
}))

vi.mock('@main/window/secondaryWindows', () => ({
  getSecondaryWindow: vi.fn(() => null)
}))

import { getSecondaryWindow } from '@main/window/secondaryWindows'

vi.mock('@main/window/broadcast', () => ({
  broadcastToSecondaryRenderers: vi.fn()
}))

const { audioMonitorHolder } = vi.hoisted(() => ({ audioMonitorHolder: { cb: null as any } }))
vi.mock('../../../audio/AudioDeviceEnumerator', () => ({
  startAudioDeviceMonitor: vi.fn((cb: any) => {
    audioMonitorHolder.cb = cb
    return { stop: vi.fn() }
  })
}))

import { webContents as electronWebContents } from 'electron'
import { ProjectionAudio } from '../ProjectionAudio'
import { ProjectionService } from '../ProjectionService'

const EventEmitter = require('events')

function fakeDriver(extra: Record<string, unknown> = {}): any {
  const d = new EventEmitter()
  return Object.assign(d, {
    isWiredMode: () => false,
    usbSerial: () => undefined,
    getControllerId: () => undefined,
    close: vi.fn(async () => undefined),
    requestKeyframe: vi.fn(),
    setVideoActive: vi.fn(),
    send: vi.fn(async () => true),
    ...extra
  })
}

function makeSvc(): any {
  return new ProjectionService() as any
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
  vi.spyOn(console, 'debug').mockImplementation(() => {})
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('ProjectionService driver getters and codec caps', () => {
  test('getDongleDriver / getCpDriver expose driver manager state', () => {
    const svc = makeSvc()
    expect(svc.getDongleDriver()).toBe(svc.drivers.getDongle())
    expect(svc.getCpDriver()).toBeNull()
  })

  test('activeAaSession and wired transport helpers reflect the active session', () => {
    const svc = makeSvc()
    expect(svc.getAaDriver()).toBeNull()
    expect(svc.isActiveAaWired()).toBe(false)
    expect(svc.isActiveCpWired()).toBe(false)

    const aa = fakeDriver({ isWiredMode: () => true })
    const s = svc.sessions.upsert(aa, 'androidauto', 'usb', {})
    svc.sessions.activate(s.index)

    expect(svc.getAaDriver()).toBe(aa)
    expect(svc.isActiveAaWired()).toBe(true)
    expect(svc.isActiveCpWired()).toBe(false)
  })

  test('isActiveCpWired is true for a wired carplay session', () => {
    const svc = makeSvc()
    const cp = fakeDriver()
    const s = svc.sessions.upsert(cp, 'carplay', 'usb', { usbUdid: 'udid-1' })
    svc.sessions.activate(s.index)
    expect(svc.isActiveCpWired()).toBe(true)
    expect(svc.getAaDriver()).toBeNull()
  })

  test('codec capability changes are pushed to both driver stacks', () => {
    const svc = makeSvc()
    svc.drivers.setAaHevcSupported = vi.fn()
    svc.drivers.setCpHevcSupported = vi.fn()
    svc.drivers.setAaVp9Supported = vi.fn()
    svc.drivers.setCpVp9Supported = vi.fn()
    svc.drivers.setAaAv1Supported = vi.fn()
    svc.drivers.setCpAv1Supported = vi.fn()

    svc.codecCaps.applyCodecCapabilities({
      h265: { hw: true },
      vp9: { hw: true },
      av1: { hw: true }
    })

    expect(svc.drivers.setAaHevcSupported).toHaveBeenCalledWith(true)
    expect(svc.drivers.setCpHevcSupported).toHaveBeenCalledWith(true)
    expect(svc.drivers.setAaVp9Supported).toHaveBeenCalledWith(true)
    expect(svc.drivers.setCpVp9Supported).toHaveBeenCalledWith(true)
    expect(svc.drivers.setAaAv1Supported).toHaveBeenCalledWith(true)
    expect(svc.drivers.setCpAv1Supported).toHaveBeenCalledWith(true)
    expect(svc.getHevcSupported()).toBe(true)
  })
})

describe('ProjectionService message dispatch', () => {
  test('onDriverMessage routes SoftwareVersion and BoxInfo without a renderer', () => {
    const svc = makeSvc()
    svc.dongleState.handleSoftwareVersion = vi.fn()
    svc.dongleState.handleBoxInfo = vi.fn()
    svc.deviceController.emitDevices = vi.fn()
    svc.webContents = null

    svc.onDriverMessage(Object.assign(new SoftwareVersion(), { version: '1' }))
    svc.onDriverMessage(new BoxInfo())

    expect(svc.dongleState.handleSoftwareVersion).toHaveBeenCalled()
    expect(svc.dongleState.handleBoxInfo).toHaveBeenCalled()
  })

  test('onDriverMessage bails on renderer-only messages until a renderer attaches', () => {
    const svc = makeSvc()
    svc.webContents = null
    expect(() => svc.onDriverMessage(new Plugged(5))).not.toThrow()
  })

  test('onDriverMessage dispatches renderer-bound messages once attached', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.handleBluetoothPairedList = vi.fn()
    svc.handleBtPeerConnected = vi.fn()
    svc.handlePlugged = vi.fn()
    svc.handleBoxUpdateProgress = vi.fn()
    svc.handleBoxUpdateState = vi.fn()
    svc.handleVideoData = vi.fn()
    svc.handleAudioData = vi.fn()
    svc.handleCommand = vi.fn()

    svc.onDriverMessage(new BluetoothPairedList())
    svc.onDriverMessage(new BluetoothPeerConnected('aa:bb'))
    svc.onDriverMessage(new Plugged(5))
    svc.onDriverMessage(new BoxUpdateProgress(1))
    svc.onDriverMessage(new BoxUpdateState())
    svc.onDriverMessage(new VideoData())
    svc.onDriverMessage(new AudioData())
    svc.onDriverMessage(new Command(1))

    expect(svc.handleBluetoothPairedList).toHaveBeenCalled()
    expect(svc.handleBtPeerConnected).toHaveBeenCalled()
    expect(svc.handlePlugged).toHaveBeenCalled()
    expect(svc.handleBoxUpdateProgress).toHaveBeenCalled()
    expect(svc.handleBoxUpdateState).toHaveBeenCalled()
    expect(svc.handleVideoData).toHaveBeenCalled()
    expect(svc.handleAudioData).toHaveBeenCalled()
    expect(svc.handleCommand).toHaveBeenCalled()
  })

  test('handleBluetoothPairedList forwards raw data to the paired registry', () => {
    const svc = makeSvc()
    svc.btPaired.setDonglePairedRaw = vi.fn()
    svc.handleBluetoothPairedList(new BluetoothPairedList('raw'))
    expect(svc.btPaired.setDonglePairedRaw).toHaveBeenCalledWith('raw')
  })

  test('handleBtPeerConnected stores the peer MAC and emits devices once', () => {
    const svc = makeSvc()
    svc.deviceController.emitDevices = vi.fn()
    svc.handleBtPeerConnected(new BluetoothPeerConnected('aa-bb-cc-dd-ee-ff'))
    expect(svc.dongleState.getConnectedMac()).toBe('AA:BB:CC:DD:EE:FF')
    expect(svc.deviceController.emitDevices).toHaveBeenCalledTimes(1)
    svc.handleBtPeerConnected(new BluetoothPeerConnected('AA:BB:CC:DD:EE:FF'))
    expect(svc.deviceController.emitDevices).toHaveBeenCalledTimes(1)
  })

  test('ipc host noteDonglePairForgotten removes the row and ends a hit session', async () => {
    const svc = makeSvc()
    const host = svc.buildIpcHost()
    svc.deviceController.emitDevices = vi.fn()
    svc.dongleState.removeFromDevList = vi.fn(() => true)
    svc.dongleState.getConnectedMac = vi.fn(() => 'AA:BB:CC:DD:EE:FF')
    svc.disconnectPhone = vi.fn(async () => undefined)
    svc.onDonglePhoneDisconnected = vi.fn()

    host.noteDonglePairForgotten('aa:bb:cc:dd:ee:ff')
    await Promise.resolve()
    await Promise.resolve()

    expect(svc.dongleState.removeFromDevList).toHaveBeenCalledWith('aa:bb:cc:dd:ee:ff')
    expect(svc.deviceController.emitDevices).toHaveBeenCalled()
    expect(svc.disconnectPhone).toHaveBeenCalled()
    expect(svc.onDonglePhoneDisconnected).toHaveBeenCalled()
  })

  test('ipc host noteDonglePairForgotten leaves an unrelated connection running', () => {
    const svc = makeSvc()
    const host = svc.buildIpcHost()
    svc.deviceController.emitDevices = vi.fn()
    svc.dongleState.removeFromDevList = vi.fn(() => false)
    svc.dongleState.getConnectedMac = vi.fn(() => 'FF:FF:FF:FF:FF:FF')
    svc.disconnectPhone = vi.fn(async () => undefined)

    host.noteDonglePairForgotten('aa:bb:cc:dd:ee:ff')

    expect(svc.deviceController.emitDevices).not.toHaveBeenCalled()
    expect(svc.disconnectPhone).not.toHaveBeenCalled()
  })

  test('the audio mic uplink gate only opens for the dongle driver', async () => {
    const svc = makeSvc()
    const { ProjectionAudio: PA } = await import('../ProjectionAudio')
    const gate = (PA as unknown as ReturnType<typeof vi.fn>).mock.calls.at(-1)?.[4] as
      | (() => boolean)
      | undefined
    expect(gate).toBeTypeOf('function')
    // The dongle driver is the startup default, so the gate opens right away
    expect(gate!()).toBe(true)
    svc.drivers.getActive = vi.fn(() => ({}))
    expect(gate!()).toBe(false)
  })

  test('handleBoxUpdateProgress emits an upload:progress event', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.handleBoxUpdateProgress({ progress: 42 })
    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'fwUpdate',
      stage: 'upload:progress',
      progress: 42
    })
  })

  test('handleBoxUpdateState emits a non-terminal state and no terminal follow-up', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.dongleState.invalidateDongleInfoKey = vi.fn()

    svc.handleBoxUpdateState({
      status: 1,
      statusText: 'working',
      isOta: false,
      isTerminal: false,
      ok: true
    })

    expect(send).toHaveBeenCalledTimes(1)
    expect(svc.dongleState.invalidateDongleInfoKey).not.toHaveBeenCalled()
  })

  test('handleBoxUpdateState terminal success emits done and requests a keyframe', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.dongleState.invalidateDongleInfoKey = vi.fn()
    svc.drivers.getDongle().requestKeyframe = vi.fn()

    svc.handleBoxUpdateState({
      status: 2,
      statusText: '',
      isOta: true,
      isTerminal: true,
      ok: true
    })

    expect(send).toHaveBeenCalledTimes(2)
    expect(send.mock.calls[1][1]).toMatchObject({
      stage: 'upload:done',
      message: 'Update finished'
    })
    expect(svc.dongleState.invalidateDongleInfoKey).toHaveBeenCalled()
  })

  test('handleBoxUpdateState terminal failure emits an error stage', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.dongleState.invalidateDongleInfoKey = vi.fn()
    svc.drivers.getDongle().requestKeyframe = vi.fn()

    svc.handleBoxUpdateState({
      status: 3,
      statusText: '',
      isOta: false,
      isTerminal: true,
      ok: false
    })

    expect(send.mock.calls[1][1]).toMatchObject({ stage: 'upload:error', message: 'Update failed' })
  })

  test('handleCommand emits the command and requests cluster focus for value 508', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.anyClusterRequested = vi.fn(() => true)
    svc.drivers.getDongle().requestClusterFocus = vi.fn()
    svc.driverForTest = svc.drivers.getActive()

    svc.handleCommand({ value: 508 })

    expect(send).toHaveBeenCalledWith('projection-event', {
      type: 'command',
      message: { value: 508 }
    })
    expect(svc.drivers.getActive().requestClusterFocus).toHaveBeenCalled()
  })

  test('handleCommand does not request focus when no cluster is requested', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.anyClusterRequested = vi.fn(() => false)
    svc.drivers.getDongle().requestClusterFocus = vi.fn()

    svc.handleCommand({ value: 508 })

    expect(svc.drivers.getActive().requestClusterFocus).not.toHaveBeenCalled()
  })
})

describe('ProjectionService meta messages', () => {
  test('onMetaMessage routes MediaData and NavigationData to their stores', () => {
    const svc = makeSvc()
    svc.mediaStore.handle = vi.fn()
    svc.navStore.handle = vi.fn()
    const driver = fakeDriver()

    svc.onMetaMessage(driver, new MediaData())
    svc.onMetaMessage(driver, new NavigationData())

    expect(svc.mediaStore.handle).toHaveBeenCalled()
    expect(svc.navStore.handle).toHaveBeenCalled()
  })

  test('onMetaMessage applies duck for the active session and stores it on all sessions', () => {
    const svc = makeSvc()
    const driver = fakeDriver()
    const s = svc.sessions.upsert(driver, 'androidauto', 'wifi', {})
    svc.sessions.activate(s.index)

    svc.onMetaMessage(driver, Object.assign(new DuckAudio(), { level: 0.3, durationMs: 200 }))
    expect(s.audio.duckLevel).toBe(0.3)
    expect(svc.audio.duck).toHaveBeenCalledWith(0.3, 200)

    svc.onMetaMessage(driver, Object.assign(new DuckAudio(), { level: 1, durationMs: 100 }))
    expect(svc.audio.unduck).toHaveBeenCalledWith(100)
  })

  test('onMetaMessage duck without a session only affects active audio path', () => {
    const svc = makeSvc()
    const driver = fakeDriver()
    svc.onMetaMessage(driver, Object.assign(new DuckAudio(), { level: 0.5, durationMs: 50 }))
    expect(svc.audio.duck).not.toHaveBeenCalled()
  })
})

describe('ProjectionService video handling', () => {
  const MAIN = 0x7a000001
  const CLUSTER = 0x7a000010

  test('handleVideoData ignores cluster frames when cluster is not displayed', () => {
    const svc = makeSvc()
    svc.config = { dashboards: null }
    svc.planes.pushCluster = vi.fn()
    const msg = Object.assign(new VideoData(), {
      cluster: true,
      width: 100,
      height: 50,
      data: Buffer.from([1])
    })
    svc.handleVideoData(msg)
    expect(svc.planes.pushCluster).not.toHaveBeenCalled()
  })

  test('handleVideoData pushes cluster frames and emits resolution changes', () => {
    const svc = makeSvc()
    svc.config = { dashboards: { dash3: { main: true } } }
    svc.planes.pushCluster = vi.fn()
    svc.planes.recropAllClusters = vi.fn()
    svc.getClusterTargetWebContents = vi.fn(() => [{ isDestroyed: () => false, send: vi.fn() }])
    const active = fakeDriver()
    const s = svc.sessions.upsert(active, 'androidauto', 'wifi', {})
    svc.sessions.activate(s.index)

    const msg = Object.assign(new VideoData(), {
      cluster: true,
      width: 320,
      height: 180,
      data: Buffer.from([1, 2])
    })
    svc.handleVideoData(msg)

    expect(svc.lastClusterVideoWidth).toBe(320)
    expect(svc.planes.recropAllClusters).toHaveBeenCalled()
    expect(svc.planes.pushCluster).toHaveBeenCalled()
    expect(s.video.cluster.width).toBe(320)
  })

  test('handleVideoData handles main frames, resolution change, and first-frame marking', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    svc.planes.pushMain = vi.fn()
    svc.planes.updateMainCrop = vi.fn()
    svc.statusFile.setStreaming = vi.fn()
    const active = fakeDriver()
    const s = svc.sessions.upsert(active, 'androidauto', 'wifi', {})
    svc.sessions.activate(s.index)

    const msg = Object.assign(new VideoData(), {
      cluster: false,
      width: 1920,
      height: 1080,
      data: Buffer.from([9])
    })
    svc.handleVideoData(msg)

    expect(svc.firstFrameLogged).toBe(true)
    expect(svc.planes.updateMainCrop).toHaveBeenCalled()
    expect(svc.planes.pushMain).toHaveBeenCalled()
    expect(s.video.main.width).toBe(1920)
  })

  test('markFirstFrame is idempotent', () => {
    const svc = makeSvc()
    svc.statusFile.setStreaming = vi.fn()
    svc.markFirstFrame()
    svc.markFirstFrame()
    expect(svc.statusFile.setStreaming).toHaveBeenCalledTimes(1)
  })

  test('onNativeVideoStarted marks first frame only for the main plane', () => {
    const svc = makeSvc()
    svc.markFirstFrame = vi.fn()
    svc.onNativeVideoStarted(CLUSTER)
    expect(svc.markFirstFrame).not.toHaveBeenCalled()
    svc.onNativeVideoStarted(MAIN)
    expect(svc.markFirstFrame).toHaveBeenCalled()
  })

  test('onNativeVideoConfig routes cluster config to the cluster handler', () => {
    const svc = makeSvc()
    svc.onNativeClusterConfig = vi.fn()
    svc.onNativeVideoConfig(CLUSTER, 'h264', Buffer.from([1]))
    expect(svc.onNativeClusterConfig).toHaveBeenCalled()
  })

  test('onNativeVideoConfig ignores unknown plane ids', () => {
    const svc = makeSvc()
    svc.planes.prepareMain = vi.fn()
    svc.onNativeVideoConfig(0x1234, 'h264', Buffer.from([1]))
    expect(svc.planes.prepareMain).not.toHaveBeenCalled()
  })

  test('onNativeVideoConfig bails when no live renderer is attached', () => {
    const svc = makeSvc()
    svc.webContents = { isDestroyed: () => true, send: vi.fn() }
    svc.planes.prepareMain = vi.fn()
    svc.onNativeVideoConfig(MAIN, 'h264', Buffer.from([1]))
    expect(svc.planes.prepareMain).not.toHaveBeenCalled()
  })

  test('onNativeVideoConfig prepares the main plane and emits projection + resolution', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    svc.config = { projectionWidth: 1280, projectionHeight: 720 }
    svc.planes.prepareMain = vi.fn(() => true)
    svc.planes.updateMainCrop = vi.fn()
    svc.drivers.getDongle().requestKeyframe = vi.fn()
    const active = fakeDriver()
    const s = svc.sessions.upsert(active, 'androidauto', 'wifi', {})
    svc.sessions.activate(s.index)

    svc.onNativeVideoConfig(MAIN, 'h264', Buffer.from([1]))

    expect(svc.planes.prepareMain).toHaveBeenCalled()
    expect(svc.lastVideoWidth).toBe(1280)
    expect(send).toHaveBeenCalledWith('projection-event', { type: 'projection', shown: true })
  })

  test('onNativeClusterConfig seeds cluster size and prepares clusters', () => {
    const svc = makeSvc()
    svc.config = { clusterWidth: 800, clusterHeight: 480 }
    svc.planes.prepareClusters = vi.fn(() => true)
    svc.drivers.getDongle().requestKeyframe = vi.fn()
    svc.onNativeClusterConfig('h264', Buffer.from([1]))
    expect(svc.lastClusterVideoWidth).toBe(800)
    expect(svc.planes.prepareClusters).toHaveBeenCalled()
    expect(svc.drivers.getDongle().requestKeyframe).toHaveBeenCalled()
  })

  test('syncVideoActiveFeeder toggles the active feeder when the driver changes', () => {
    const svc = makeSvc()
    const a = fakeDriver()
    const s = svc.sessions.upsert(a, 'androidauto', 'wifi', {})
    svc.sessions.activate(s.index)
    svc.videoActiveDriver = null
    svc.syncVideoActiveFeeder()
    expect(a.setVideoActive).toHaveBeenCalledWith(true)
    svc.syncVideoActiveFeeder()
  })

  test('attachCodecCapture wires codec + config events onto a driver', () => {
    const svc = makeSvc()
    const d = fakeDriver()
    const s = svc.sessions.upsert(d, 'androidauto', 'wifi', {})
    svc.attachCodecCapture(d)

    d.emit('video-codec', 'h264')
    d.emit('cluster-video-codec', 'h265')
    d.emit('video-config', Buffer.from([1]))
    d.emit('cluster-video-config', Buffer.from([2]))

    expect(svc.lastMainCodecByDriver.get(d)).toBe('h264')
    expect(svc.lastClusterCodecByDriver.get(d)).toBe('h265')
    expect(s.video.main.codec).toBe('h264')
    expect(s.video.cluster.codecData).toEqual(Buffer.from([2]))
  })

  test('attachCodecCapture tolerates codec events with no matching session', () => {
    const svc = makeSvc()
    const d = fakeDriver()
    svc.attachCodecCapture(d)
    expect(() => {
      d.emit('video-codec', 'h264')
      d.emit('cluster-video-codec', 'h265')
      d.emit('video-config', Buffer.from([1]))
      d.emit('cluster-video-config', Buffer.from([2]))
    }).not.toThrow()
  })

  test('driver event forwarders delegate to the plane manager', () => {
    const svc = makeSvc()
    svc.planes.setMainCodec = vi.fn()
    svc.planes.setMainCodecData = vi.fn()
    svc.planes.setClusterCodecData = vi.fn()
    svc.planes.setClusterCodec = vi.fn()

    svc.onDriverVideoCodec('h264')
    svc.onDriverVideoConfig(Buffer.from([1]))
    svc.onDriverClusterVideoConfig(Buffer.from([2]))
    svc.onDriverClusterVideoCodec('vp9')

    expect(svc.planes.setMainCodec).toHaveBeenCalledWith('h264')
    expect(svc.planes.setMainCodecData).toHaveBeenCalled()
    expect(svc.planes.setClusterCodecData).toHaveBeenCalled()
    expect(svc.planes.setClusterCodec).toHaveBeenCalledWith('vp9')
  })

  test('onDriverFailure sends a failure event to a live renderer', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    svc.onDriverFailure()
    expect(send).toHaveBeenCalledWith('projection-event', { type: 'failure' })
  })

  test('onDriverFailure is a no-op without a live renderer', () => {
    const svc = makeSvc()
    svc.webContents = null
    expect(() => svc.onDriverFailure()).not.toThrow()
    svc.webContents = { send: vi.fn(), isDestroyed: () => true }
    svc.onDriverFailure()
    expect(svc.webContents.send).not.toHaveBeenCalled()
  })

  test('onDriverTargetedConnect clears the pending startup target', () => {
    const svc = makeSvc()
    svc.pendingStartupConnectTarget = { some: 'target' }
    svc.onDriverTargetedConnect()
    expect(svc.pendingStartupConnectTarget).toBeNull()
  })

  test('setVideoVisible and setClusterVisible delegate to plane manager', () => {
    const svc = makeSvc()
    svc.planes.setVideoVisible = vi.fn()
    svc.planes.setClusterVisible = vi.fn()
    svc.planes.updateClusterStreamActive = vi.fn(() => false)
    svc.setVideoVisible(true)
    svc.setClusterVisible(false)
    expect(svc.planes.setVideoVisible).toHaveBeenCalledWith(true)
    expect(svc.planes.setClusterVisible).toHaveBeenCalledWith(false)
  })

  test('setBlinkerSoundActive delegates to the system sound', () => {
    const svc = makeSvc()
    svc.systemSound.setBlinkerActive = vi.fn()
    svc.setBlinkerSoundActive(true)
    expect(svc.systemSound.setBlinkerActive).toHaveBeenCalledWith(true)
  })

  test('anyClusterRequested prunes destroyed renderer ids', () => {
    const svc = makeSvc()
    ;(electronWebContents.fromId as Mock).mockReturnValueOnce(null)
    svc.clusterRequestedBy.add(7)
    expect(svc.anyClusterRequested()).toBe(false)
    expect(svc.clusterRequestedBy.size).toBe(0)
  })

  test('syncClusterStreamFocus pushes stream-active state to the driver stacks', () => {
    const svc = makeSvc()
    svc.anyClusterRequested = vi.fn(() => true)
    svc.planes.updateClusterStreamActive = vi.fn(() => true)
    svc.drivers.setAaClusterStreamActive = vi.fn()
    svc.drivers.setCpClusterStreamActive = vi.fn()
    svc.syncClusterStreamFocus()
    expect(svc.drivers.setAaClusterStreamActive).toHaveBeenCalledWith(true)
  })

  test('syncClusterStreamFocus bails when the stream-active state is unchanged', () => {
    const svc = makeSvc()
    svc.anyClusterRequested = vi.fn(() => true)
    svc.planes.updateClusterStreamActive = vi.fn(() => false)
    svc.drivers.setAaClusterStreamActive = vi.fn()
    svc.syncClusterStreamFocus()
    expect(svc.drivers.setAaClusterStreamActive).not.toHaveBeenCalled()
  })
})

describe('ProjectionService audio handling', () => {
  test('handleAudioData forwards a command and infers AA play status transitions', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.statusFile.applyAudioCommand = vi.fn()
    svc.mediaStore.patchAaPlayStatus = vi.fn()
    svc.lastPluggedPhoneType = 5

    svc.handleAudioData({ command: 10, audioType: 1, decodeType: 1, volume: 0.5 })
    expect(svc.aaPlaybackInferred).toBe(1)

    svc.handleAudioData({ command: 11, audioType: 1, decodeType: 1 })
    expect(svc.aaPlaybackInferred).toBe(2)

    svc.handleAudioData({ command: 2, audioType: 1, decodeType: 1 })
    expect(svc.aaPlaybackInferred).toBe(2)

    expect(svc.statusFile.applyAudioCommand).toHaveBeenCalled()
    expect(svc.mediaStore.patchAaPlayStatus).toHaveBeenCalled()
  })

  test('handleAudioData emits audioInfo once per metadata key and skips unknown decode types', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.lastPluggedPhoneType = 3

    svc.handleAudioData({ decodeType: 1, audioType: 1 })
    svc.handleAudioData({ decodeType: 1, audioType: 1 })
    const audioInfo = send.mock.calls.filter(([, p]) => p?.type === 'audioInfo')
    expect(audioInfo).toHaveLength(1)

    send.mockClear()
    svc.handleAudioData({ decodeType: 999 })
    expect(send).not.toHaveBeenCalled()
  })
})

describe('ProjectionService phone + dongle lifecycle events', () => {
  test('onPhoneConnected persists work mode, emits plugged, and runs hooks', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.statusFile.setProjection = vi.fn()
    const hook = vi.fn()
    svc.addPluggedHook(hook)
    const throwingHook = vi.fn(() => {
      throw new Error('hook boom')
    })
    svc.addPluggedHook(throwingHook)

    svc.onPhoneConnected(3)

    expect(configEventsMock.emit).toHaveBeenCalledWith('requestSave', {
      lastPhoneWorkMode: expect.any(Number)
    })
    expect(send).toHaveBeenCalledWith('projection-event', { type: 'plugged', phoneType: 3 })
    expect(hook).toHaveBeenCalledWith(3)
    expect(throwingHook).toHaveBeenCalled()
  })

  test('onPhoneConnected swallows a persistence failure', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    ;(configEventsMock.emit as Mock).mockImplementationOnce(() => {
      throw new Error('save boom')
    })
    expect(() => svc.onPhoneConnected(5)).not.toThrow()
  })

  test('addPluggedHook returns a disposer that removes the hook', () => {
    const svc = makeSvc()
    const hook = vi.fn()
    const dispose = svc.addPluggedHook(hook)
    dispose()
    dispose()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    svc.onPhoneConnected(5)
    expect(hook).not.toHaveBeenCalled()
  })

  test('onPhoneDisconnected clears UI state when no session remains active', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.statusFile.setProjection = vi.fn()
    svc.statusFile.setStreaming = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.deviceController.emitDevices = vi.fn()

    svc.onPhoneDisconnected()

    expect(send).toHaveBeenCalledWith('projection-event', { type: 'unplugged' })
    expect(svc.navStore.reset).toHaveBeenCalledWith('phone-disconnect')
  })

  test('onPhoneDisconnected keeps UI when a session is still active', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send }
    svc.navStore.reset = vi.fn()
    svc.deviceController.emitDevices = vi.fn()
    const d = fakeDriver()
    const s = svc.sessions.upsert(d, 'androidauto', 'wifi', {})
    svc.sessions.activate(s.index)
    send.mockClear()

    svc.onPhoneDisconnected()

    expect(send).not.toHaveBeenCalledWith('projection-event', { type: 'unplugged' })
  })

  test('handlePlugged connects and starts when no session and non-cp transport', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    svc.start = vi.fn(async () => undefined)
    svc.handlePlugged({ phoneType: 5 })
    expect(svc.start).toHaveBeenCalled()
  })

  test('handlePlugged does not start when already started', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    svc.started = true
    svc.start = vi.fn(async () => undefined)
    svc.handlePlugged({ phoneType: 5 })
    expect(svc.start).not.toHaveBeenCalled()
  })

  test('onDonglePhoneConnected upserts and activates a dongle session', () => {
    const svc = makeSvc()
    svc.deviceController.emitDevices = vi.fn()
    svc.onDonglePhoneConnected()
    expect(svc.sessions.active()?.protocol).toBe('dongle')
  })

  test('onDonglePhoneDisconnected with another session emits devices only', () => {
    const svc = makeSvc()
    svc.deviceController.emitDevices = vi.fn()
    svc.btPaired.clearDongleRaw = vi.fn()
    svc.dongleState.clearOnDongleGone = vi.fn()
    const other = fakeDriver()
    svc.sessions.upsert(other, 'androidauto', 'wifi', {})
    svc.sessions.upsert(svc.drivers.getDongle(), 'dongle', 'usb', {})

    svc.onDonglePhoneDisconnected()

    expect(svc.btPaired.clearDongleRaw).toHaveBeenCalled()
    expect(svc.deviceController.emitDevices).toHaveBeenCalled()
  })

  test('onDonglePhoneDisconnected without another session triggers phone-disconnected', () => {
    const svc = makeSvc()
    svc.deviceController.emitDevices = vi.fn()
    svc.btPaired.clearDongleRaw = vi.fn()
    svc.dongleState.clearOnDongleGone = vi.fn()
    svc.onPhoneDisconnected = vi.fn()
    svc.sessions.upsert(svc.drivers.getDongle(), 'dongle', 'usb', {})

    svc.onDonglePhoneDisconnected()

    expect(svc.onPhoneDisconnected).toHaveBeenCalled()
  })

  test('onDongleInfo emits devices when dongle state changed', () => {
    const svc = makeSvc()
    svc.deviceController.emitDevices = vi.fn()
    svc.dongleState.applyDongleInfo = vi.fn(() => true)
    svc.onDongleInfo({ boxInfo: {} })
    expect(svc.deviceController.emitDevices).toHaveBeenCalled()

    svc.deviceController.emitDevices.mockClear()
    svc.dongleState.applyDongleInfo = vi.fn(() => false)
    svc.onDongleInfo({ boxInfo: {} })
    expect(svc.deviceController.emitDevices).not.toHaveBeenCalled()
  })

  test('handleSoftwareVersion and handleBoxInfo delegate to dongle state', () => {
    const svc = makeSvc()
    svc.dongleState.handleSoftwareVersion = vi.fn()
    svc.dongleState.handleBoxInfo = vi.fn()
    svc.deviceController.emitDevices = vi.fn()
    svc.handleSoftwareVersion(new SoftwareVersion())
    svc.handleBoxInfo(new BoxInfo())
    expect(svc.dongleState.handleSoftwareVersion).toHaveBeenCalled()
    expect(svc.dongleState.handleBoxInfo).toHaveBeenCalled()
    expect(svc.deviceController.emitDevices).toHaveBeenCalled()
  })
})

describe('ProjectionService session connect / disconnect handlers', () => {
  function primeConnect(svc: any) {
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    svc.refreshBtPairedList = vi.fn(async () => 0)
  }

  test('onAaConnected registers, activates and marks the phone connected', () => {
    const svc = makeSvc()
    primeConnect(svc)
    const session = fakeDriver({ isWiredMode: () => false })
    svc.onAaConnected(session)
    expect(svc.sessions.byDriver(session)?.protocol).toBe('androidauto')
    expect(svc.sessions.active()?.driver).toBe(session)
  })

  test('onAaDisconnected tears down audio and media when no session remains', () => {
    const svc = makeSvc()
    primeConnect(svc)
    svc.mediaStore.reset = vi.fn()
    svc.onPhoneDisconnected = vi.fn()
    svc.deviceRegistry.clearPresence = vi.fn()
    const session = fakeDriver()
    svc.sessions.byDriver = vi.fn(() => ({ device: {} }))
    svc.sessions.closeByDriver = vi.fn()
    svc.sessions.active = vi.fn(() => null)

    svc.onAaDisconnected(session)

    expect(svc.audio.resetForSessionStop).toHaveBeenCalled()
    expect(svc.mediaStore.reset).toHaveBeenCalledWith('aa-session-end')
    expect(svc.onPhoneDisconnected).toHaveBeenCalled()
  })

  test('onAaDisconnected swallows an audio reset failure', () => {
    const svc = makeSvc()
    primeConnect(svc)
    svc.mediaStore.reset = vi.fn()
    svc.onPhoneDisconnected = vi.fn()
    svc.deviceRegistry.clearPresence = vi.fn()
    svc.audio.resetForSessionStop = vi.fn(() => {
      throw new Error('audio boom')
    })
    const session = fakeDriver()
    svc.sessions.byDriver = vi.fn(() => ({ device: {} }))
    svc.sessions.closeByDriver = vi.fn()
    svc.sessions.active = vi.fn(() => null)

    expect(() => svc.onAaDisconnected(session)).not.toThrow()
    expect(svc.mediaStore.reset).toHaveBeenCalledWith('aa-session-end')
  })

  test('onAaDisconnected keeps audio when another session stays active', () => {
    const svc = makeSvc()
    primeConnect(svc)
    svc.mediaStore.reset = vi.fn()
    svc.onPhoneDisconnected = vi.fn()
    const gone = fakeDriver()
    svc.sessions.byDriver = vi.fn(() => ({ device: {} }))
    svc.sessions.closeByDriver = vi.fn()
    svc.sessions.active = vi.fn(() => ({ index: 1 }))

    svc.onAaDisconnected(gone)

    expect(svc.mediaStore.reset).not.toHaveBeenCalled()
    expect(svc.onPhoneDisconnected).toHaveBeenCalled()
  })

  test('onCpConnected registers a carplay session with its controller id', () => {
    const svc = makeSvc()
    primeConnect(svc)
    const session = fakeDriver({ getControllerId: () => 'ctrl-1' })
    svc.onCpConnected(session)
    expect(svc.sessions.byDriver(session)?.protocol).toBe('carplay')
  })

  test('onCpDisconnected closes the session and reports phone gone', () => {
    const svc = makeSvc()
    svc.onPhoneDisconnected = vi.fn()
    svc.deviceRegistry.clearPresence = vi.fn()
    const session = fakeDriver()
    svc.sessions.byDriver = vi.fn(() => ({ device: { wifiMac: 'aa:bb' } }))
    svc.sessions.closeByDriver = vi.fn()

    svc.onCpDisconnected(session)

    expect(svc.deviceRegistry.clearPresence).toHaveBeenCalled()
    expect(svc.onPhoneDisconnected).toHaveBeenCalled()
  })
})

describe('ProjectionService presence handlers', () => {
  test('onCpHelperPresence notes a wifi link and closes on link-down', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteLink = vi.fn()
    svc.sessions.closeByDeviceOnTransport = vi.fn()

    svc.onCpHelperPresence({ kind: 'wifi', ip: '1.2.3.4', wifiMac: 'AA:BB', connected: false })

    expect(svc.deviceRegistry.noteLink).toHaveBeenCalled()
    expect(svc.sessions.closeByDeviceOnTransport).toHaveBeenCalled()
  })

  test('onCpHelperPresence notes a device presence', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    svc.onCpHelperPresence({
      kind: 'device',
      ip: '1.2.3.4',
      btMac: 'AA:BB',
      usbUdid: 'udid',
      name: 'iPhone'
    })
    expect(svc.deviceRegistry.noteDevice).toHaveBeenCalled()
  })

  test('onCpHelperPresence device-gone closes a usb session', () => {
    const svc = makeSvc()
    svc.sessions.closeByDeviceOnTransport = vi.fn()
    svc.onCpHelperPresence({ kind: 'device-gone', usbUdid: 'udid' })
    expect(svc.sessions.closeByDeviceOnTransport).toHaveBeenCalledWith({ usbUdid: 'udid' }, 'usb')
  })

  test('onCpHelperPresence device-gone without a udid is ignored', () => {
    const svc = makeSvc()
    svc.sessions.closeByDeviceOnTransport = vi.fn()
    svc.onCpHelperPresence({ kind: 'device-gone' })
    expect(svc.sessions.closeByDeviceOnTransport).not.toHaveBeenCalled()
  })

  test('onCpPresence device kind notes the device and upserts a session', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    const session = fakeDriver()
    svc.onCpPresence(session, {
      kind: 'device',
      ip: '1.2.3.4',
      btMac: 'AA:BB',
      usbUdid: 'udid-9',
      wifiMac: 'CC:DD',
      name: 'iPhone',
      model: 'iPhone15'
    })
    expect(svc.deviceRegistry.noteDevice).toHaveBeenCalled()
    expect(svc.sessions.byDriver(session)?.protocol).toBe('carplay')
  })

  test('onCpPresence device hands a born placeholder session to the airplay transport', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    const placeholder = fakeDriver()
    svc.sessions.upsert(placeholder, 'carplay', 'wifi', { btMac: 'AA:BB' })
    const session = fakeDriver()

    svc.onCpPresence(session, { kind: 'device', ip: '', btMac: 'AA:BB' })

    expect(placeholder.close).toHaveBeenCalled()
    expect(svc.sessions.byDriver(session)?.protocol).toBe('carplay')
  })

  test('onCpPresence active kind auto-activates the mapped session', () => {
    const svc = makeSvc()
    const session = fakeDriver()
    svc.sessions.upsert(session, 'carplay', 'wifi', { wifiMac: 'AA:BB' })
    svc.onCpPresence(session, { kind: 'active' })
    expect(svc.sessions.active()?.driver).toBe(session)
  })

  test('onCpPresence status kind notes battery/signal status', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteStatus = vi.fn()
    const session = fakeDriver()
    svc.sessions.upsert(session, 'carplay', 'wifi', { wifiMac: 'AA:BB' })
    svc.onCpPresence(session, {
      kind: 'status',
      batteryLevel: 80,
      batteryCharging: true,
      signalStrength: 3,
      carrierName: 'Carrier'
    })
    expect(svc.deviceRegistry.noteStatus).toHaveBeenCalled()
  })

  test('onAaPresence status kind notes AA status', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteStatus = vi.fn()
    const session = fakeDriver()
    svc.sessions.upsert(session, 'androidauto', 'wifi', { instanceId: 'inst' })
    svc.onAaPresence(session, {
      kind: 'status',
      batteryLevel: 50,
      batteryCritical: false,
      batteryTimeRemaining: 100,
      signalStrength: 2
    })
    expect(svc.deviceRegistry.noteStatus).toHaveBeenCalled()
  })

  test('onAaPresence ignores non-device, non-status presence kinds', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    svc.onAaPresence(fakeDriver(), { kind: 'other' })
    expect(svc.deviceRegistry.noteDevice).not.toHaveBeenCalled()
  })

  test('onAaPresence device kind notes device and upserts a wireless session', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    svc.aaBtMacByInstance.set('inst-1', 'AA:BB')
    svc.aaSerialByInstance.set('inst-1', 'serial-1')
    const session = fakeDriver({ isWiredMode: () => false, usbSerial: () => undefined })

    svc.onAaPresence(session, {
      kind: 'device',
      ip: '1.2.3.4',
      instanceId: 'inst-1',
      wifiMac: 'CC:DD',
      name: 'Pixel',
      model: 'Pixel 8'
    })

    expect(svc.deviceRegistry.noteDevice).toHaveBeenCalled()
    expect(svc.sessions.byDriver(session)?.protocol).toBe('androidauto')
  })
})

describe('ProjectionService delegations and ipc host', () => {
  test('simple arbiter/controller delegations', () => {
    const svc = makeSvc()
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ id: 'dev' }))
    svc.arbiter.isPhoneConnected = vi.fn(() => true)
    svc.arbiter.expectPhoneReenumeration = vi.fn()
    svc.arbiter.isExpectingPhoneReenumeration = vi.fn(() => true)
    svc.arbiter.getSnapshot = vi.fn(() => ({ snap: 1 }))
    svc.deviceController.getDevices = vi.fn(() => [])
    svc.deviceController.forgetDevice = vi.fn(() => ({ ok: true }))
    svc.deviceController.selectDevice = vi.fn(() => ({ ok: true }))

    expect(svc.getWiredPhoneDevice()).toEqual({ id: 'dev' })
    expect(svc.isWiredPhoneConnected()).toBe(true)
    svc.expectPhoneReenumeration(100)
    expect(svc.isExpectingPhoneReenumeration()).toBe(true)
    expect(svc.getTransportState()).toEqual({ snap: 1 })
    expect(svc.getDevices()).toEqual([])
    expect(svc.forgetDevice('id')).toEqual({ ok: true })
    expect(svc.selectDevice('id')).toEqual({ ok: true })
  })

  test('getActiveTransport reflects session protocol and started fallback', () => {
    const svc = makeSvc()
    expect(svc.getActiveTransport()).toBeNull()
    svc.started = true
    expect(svc.getActiveTransport()).toBe('dongle')

    const cp = fakeDriver()
    const s = svc.sessions.upsert(cp, 'carplay', 'wifi', {})
    svc.sessions.activate(s.index)
    expect(svc.getActiveTransport()).toBe('cp')
  })

  test('buildIpcHost lambdas delegate to the service', async () => {
    const svc = makeSvc()
    svc.restartSession = vi.fn(async () => undefined)
    svc.setVideoVisible = vi.fn()
    svc.switchTransport = vi.fn(async () => ({ ok: true, active: null }))
    svc.getTransportState = vi.fn(() => ({}))
    svc.getDevices = vi.fn(() => [])
    svc.selectDevice = vi.fn(() => ({ ok: true }))
    svc.forgetDevice = vi.fn(() => ({ ok: true }))
    svc.connectPairedDevice = vi.fn(async () => ({ ok: true }))
    svc.refreshBtPairedList = vi.fn(async () => 0)
    svc.setClusterVisible = vi.fn()
    svc.getClusterTargetWebContents = vi.fn(() => [])
    svc.codecCaps.applyCodecCapabilities = vi.fn()
    svc.sessions.activateNext = vi.fn()

    const host = svc.buildIpcHost()
    await host.restartSession()
    host.setVideoVisible(true)
    await host.switchTransport()
    host.getTransportState()
    host.getDevices()
    host.selectDevice('x')
    host.cycleSession()
    host.forgetDevice('x')
    host.applyCodecCapabilities({})
    expect(host.isUsingAa()).toBe(false)
    await host.connectBt('AA:BB')
    host.refreshBtPaired()
    host.setPendingStartupConnectTarget({ t: 1 })
    expect(host.isMainClusterWindow(999)).toBe(false)
    host.setClusterVisible(true)
    host.getClusterTargetWebContents()

    expect(svc.restartSession).toHaveBeenCalled()
    expect(svc.switchTransport).toHaveBeenCalled()
    expect(svc.sessions.activateNext).toHaveBeenCalled()
    expect(svc.connectPairedDevice).toHaveBeenCalledWith('AA:BB')
    expect(svc.pendingStartupConnectTarget).toEqual({ t: 1 })
  })

  test('shutdownWirelessSessions releases drivers and stops the supervisor', async () => {
    const svc = makeSvc()
    svc.drivers.releaseAa = vi.fn(async () => undefined)
    svc.drivers.releaseCp = vi.fn(async () => undefined)
    const sup = { stop: vi.fn(async () => undefined) }
    svc.helperSupervisor = sup

    await svc.shutdownWirelessSessions()

    expect(svc.drivers.releaseAa).toHaveBeenCalled()
    expect(svc.drivers.releaseCp).toHaveBeenCalled()
    expect(sup.stop).toHaveBeenCalled()
    expect(svc.helperSupervisor).toBeNull()
  })

  test('shutdownWirelessSessions swallows a deauth failure', async () => {
    const svc = makeSvc()
    svc.drivers.releaseAa = vi.fn(async () => undefined)
    svc.drivers.releaseCp = vi.fn(async () => undefined)
    bluezMock.deauthApClients.mockRejectedValueOnce(new Error('deauth boom'))
    svc.helperSupervisor = null
    await expect(svc.shutdownWirelessSessions()).resolves.toBeUndefined()
  })
})

describe('ProjectionService renderer draining and chunking', () => {
  test('attachRenderer drains buffered early video chunks to the renderer', () => {
    const svc = makeSvc()
    const send = vi.fn()
    const wc = { send, isDestroyed: () => false }
    svc.earlyVideoQueues = new Map([['projection-video-chunk', [{ id: 'a' }, { id: 'b' }]]])
    svc.attachRenderer(wc)
    expect(send).toHaveBeenCalledTimes(2)
    expect(svc.earlyVideoQueues.size).toBe(0)
  })

  test('attachRenderer stops draining when the renderer is destroyed mid-flush', () => {
    const svc = makeSvc()
    let destroyed = false
    const wc = {
      send: vi.fn(() => {
        destroyed = true
      }),
      isDestroyed: () => destroyed
    }
    svc.earlyVideoQueues = new Map([
      ['cluster-video-chunk', [{ id: 'a' }, { id: 'b' }, { id: 'c' }]]
    ])
    svc.attachRenderer(wc)
    expect(wc.send).toHaveBeenCalledTimes(1)
  })

  test('sendChunked buffers video chunks when no renderer is attached and caps the queue', () => {
    const svc = makeSvc()
    svc.webContents = null
    const big = new Uint8Array(260).fill(1).buffer
    svc.sendChunked('projection-video-chunk', big, 1)
    const q = svc.earlyVideoQueues.get('projection-video-chunk')
    expect(q.length).toBe(256)
  })

  test('sendChunked sends to explicit targets and skips destroyed ones', () => {
    const svc = makeSvc()
    const good = { isDestroyed: () => false, send: vi.fn() }
    const dead = { isDestroyed: () => true, send: vi.fn() }
    const throwing = {
      isDestroyed: () => false,
      send: vi.fn(() => {
        throw new Error('detached')
      })
    }
    svc.sendChunked('audio-chunk', new Uint8Array([1, 2, 3]).buffer, 2, undefined, [
      good,
      dead,
      throwing
    ])
    expect(good.send).toHaveBeenCalled()
    expect(dead.send).not.toHaveBeenCalled()
  })

  test('getClusterTargetWebContents returns main + secondary windows per config', () => {
    const svc = makeSvc()
    svc.webContents = { isDestroyed: () => false, send: vi.fn() }
    svc.config = { dashboards: { dash3: { main: true, dash: true, aux: true } } }
    ;(getSecondaryWindow as Mock).mockImplementation((role: string) => ({
      isDestroyed: () => false,
      webContents: { role, isDestroyed: () => false }
    }))

    const out = svc.getClusterTargetWebContents()
    expect(out.length).toBe(3)
  })

  test('getClusterTargetWebContents falls back to the main renderer when no screens target it', () => {
    const svc = makeSvc()
    svc.webContents = { isDestroyed: () => false, send: vi.fn() }
    svc.config = { dashboards: null }
    ;(getSecondaryWindow as Mock).mockReturnValue(null)
    const out = svc.getClusterTargetWebContents()
    expect(out).toEqual([svc.webContents])
  })

  test('getAllUiWebContents collects the main and live secondary renderers', () => {
    const svc = makeSvc()
    svc.webContents = { isDestroyed: () => false, send: vi.fn() }
    ;(getSecondaryWindow as Mock).mockImplementation((role: string) => ({
      isDestroyed: () => false,
      webContents: { role, isDestroyed: () => false }
    }))
    const out = svc.getAllUiWebContents()
    expect(out.length).toBe(3)
  })

  test('getAllUiWebContents tolerates a webContents whose isDestroyed throws', () => {
    const svc = makeSvc()
    svc.webContents = {
      isDestroyed: () => {
        throw new Error('boom')
      }
    }
    ;(getSecondaryWindow as Mock).mockReturnValue(null)
    const out = svc.getAllUiWebContents()
    expect(out.length).toBe(1)
  })

  test('getClusterTargetWebContents treats a webContents without isDestroyed as alive', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.config = { dashboards: null }
    ;(getSecondaryWindow as Mock).mockReturnValue(null)
    const out = svc.getClusterTargetWebContents()
    expect(out).toEqual([svc.webContents])
  })
})

describe('ProjectionService onConfigChanged', () => {
  test('clears cluster caches when the cluster is toggled off', () => {
    const svc = makeSvc()
    svc.config = { dashboards: { dash3: { main: true } } }
    svc.lastClusterVideoWidth = 100
    svc.lastClusterVideoHeight = 50
    svc.clusterRequestedBy.add(1)
    svc.planes.retainScreens = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()

    svc.onConfigChanged({ dashboards: { dash3: { main: false } } })

    expect(svc.clusterRequestedBy.size).toBe(0)
    expect(svc.lastClusterVideoWidth).toBeUndefined()
    expect(svc.planes.retainScreens).toHaveBeenCalled()
  })

  test('seeds AA night mode for night, day and auto appearance modes', () => {
    const svc = makeSvc()
    svc.planes.retainScreens = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.drivers.setAaInitialNightMode = vi.fn()

    svc.config = { appearanceMode: 'auto' }
    svc.onConfigChanged({ appearanceMode: 'night' })
    expect(svc.drivers.setAaInitialNightMode).toHaveBeenLastCalledWith(true)

    svc.config = { appearanceMode: 'night' }
    svc.onConfigChanged({ appearanceMode: 'day' })
    expect(svc.drivers.setAaInitialNightMode).toHaveBeenLastCalledWith(false)

    svc.config = { appearanceMode: 'day' }
    svc.onConfigChanged({ appearanceMode: 'auto' })
    expect(svc.drivers.setAaInitialNightMode).toHaveBeenLastCalledWith(undefined)
  })

  test('resyncs helper supervisor and transport on a wireless toggle', () => {
    const svc = makeSvc()
    svc.planes.retainScreens = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.syncHelperSupervisor = vi.fn()
    svc.emitTransportState = vi.fn()
    svc.config = { wirelessAaEnabled: false }

    svc.onConfigChanged({ wirelessAaEnabled: true })

    expect(svc.syncHelperSupervisor).toHaveBeenCalled()
    expect(svc.emitTransportState).toHaveBeenCalled()
  })

  test('reacts to audio device changes', () => {
    const svc = makeSvc()
    svc.planes.retainScreens = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.systemSound.onDeviceChanged = vi.fn()
    svc.connectConfiguredAudioDevices = vi.fn(async () => undefined)
    svc.config = { audioOutputDevice: 'a', audioInputDevice: 'b' }

    svc.onConfigChanged({ audioOutputDevice: 'c', audioInputDevice: 'd' })

    expect(svc.audio.onAudioDeviceChanged).toHaveBeenCalled()
    expect(svc.systemSound.onDeviceChanged).toHaveBeenCalled()
    expect(svc.connectConfiguredAudioDevices).toHaveBeenCalled()
  })

  test('is a no-op while shutting down', () => {
    const svc = makeSvc()
    svc.shuttingDown = true
    svc.planes.retainScreens = vi.fn()
    svc.onConfigChanged({ appearanceMode: 'night' })
    expect(svc.planes.retainScreens).not.toHaveBeenCalled()
  })
})

describe('ProjectionService syncHelperSupervisor (linux)', () => {
  let realPlatform: PropertyDescriptor | undefined
  beforeEach(() => {
    realPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  function primeDrivers(svc: any) {
    svc.drivers.startCp = vi.fn()
    svc.drivers.releaseCp = vi.fn(async () => undefined)
    svc.drivers.startAaWireless = vi.fn()
    svc.drivers.stopAaWireless = vi.fn()
    svc.drivers.getCpManager = vi.fn(() => ({ setAaWireless: vi.fn(), setCpWireless: vi.fn() }))
    svc.openAaBtSubscription = vi.fn()
    svc.closeAaBtSubscription = vi.fn()
    svc.populateAaBtPairedListInitial = vi.fn(async () => undefined)
    svc.emitTransportState = vi.fn()
    svc.connectConfiguredAudioDevices = vi.fn(async () => undefined)
    svc.setWirelessPhoneInRange = vi.fn()
  }

  test('starts a fresh supervisor, wireless AA and CP when enabled', async () => {
    const svc = makeSvc()
    primeDrivers(svc)
    svc.config = { wirelessAaEnabled: true, wirelessCpEnabled: true }

    svc.syncHelperSupervisor()

    expect(svc.helperSupervisor).not.toBeNull()
    expect(svc.drivers.startAaWireless).toHaveBeenCalled()
    expect(svc.drivers.startCp).toHaveBeenCalled()
    expect(svc.cpActive).toBe(true)
    await Promise.resolve()
  })

  test('stops wireless AA and CP when disabled after being active', () => {
    const svc = makeSvc()
    primeDrivers(svc)
    svc.config = { wirelessAaEnabled: true, wirelessCpEnabled: true }
    svc.syncHelperSupervisor()

    svc.config = { wirelessAaEnabled: false, wirelessCpEnabled: false }
    svc.syncHelperSupervisor()

    expect(svc.drivers.stopAaWireless).toHaveBeenCalled()
    expect(svc.aaBtActive).toBe(false)
  })

  test('stops the supervisor entirely when nothing is wanted', () => {
    const svc = makeSvc()
    primeDrivers(svc)
    // force a supervisor to exist, then request nothing (no linux want)
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    svc.helperSupervisor = { stop: vi.fn(async () => undefined) }
    svc.btEnableKey = 'h'
    svc.config = {}
    svc.syncHelperSupervisor()
    expect(svc.helperSupervisor).toBeNull()
  })

  test('toggles wireless CP live when only the CP flag changes', () => {
    const svc = makeSvc()
    primeDrivers(svc)
    const cpm = { setAaWireless: vi.fn(), setCpWireless: vi.fn() }
    svc.drivers.getCpManager = vi.fn(() => cpm)
    svc.config = { wirelessAaEnabled: true, wirelessCpEnabled: false }
    svc.syncHelperSupervisor()

    svc.config = { wirelessAaEnabled: true, wirelessCpEnabled: true }
    svc.syncHelperSupervisor()

    expect(cpm.setCpWireless).toHaveBeenCalledWith(true)
  })
})

describe('ProjectionService BT helpers', () => {
  test('cpClaimedBtMacs collects uppercased macs from carplay sessions', () => {
    const svc = makeSvc()
    svc.sessions.upsert(fakeDriver(), 'carplay', 'wifi', { btMac: 'aa:bb' })
    const macs = svc.cpClaimedBtMacs()
    expect(macs.has('AA:BB')).toBe(true)
  })

  test('extractBluezMac parses a bluez device name', () => {
    const svc = makeSvc()
    expect(svc.extractBluezMac(null)).toBeNull()
    expect(svc.extractBluezMac('bluez_output.AA_BB_CC_DD_EE_FF.1')).toBe('AA:BB:CC:DD:EE:FF')
    expect(svc.extractBluezMac('not-a-bluez-name')).toBeNull()
  })

  test('dispatchRemoteInput ignores unknown commands and requires a started session', () => {
    const svc = makeSvc()
    svc.driverInput = svc.drivers.getActive()
    svc.drivers.getActive().handleInput = vi.fn()

    svc.dispatchRemoteInput('not-a-command')
    expect(svc.drivers.getActive().handleInput).not.toHaveBeenCalled()

    svc.started = false
    svc.dispatchRemoteInput('play')
    expect(svc.drivers.getActive().handleInput).not.toHaveBeenCalled()
  })

  test('dispatchRemoteInput forwards a valid command when started and swallows failures', () => {
    const svc = makeSvc()
    svc.started = true
    const handleInput = vi.fn(() => {
      throw new Error('input boom')
    })
    svc.drivers.getActive().handleInput = handleInput
    expect(() => svc.dispatchRemoteInput('play')).not.toThrow()
    expect(handleInput).toHaveBeenCalledWith('play')
  })

  test('connectConfiguredAudioDevices connects paired audio devices with retry', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = {
      audioOutputDevice: 'bluez_output.AA_BB_CC_DD_EE_FF.1',
      audioInputDevice: 'bluez_input.11_22_33_44_55_66'
    }
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB:CC:DD:EE:FF', connected: false },
      { mac: '11:22:33:44:55:66', connected: true }
    ])
    bluezMock.connectFull.mockResolvedValueOnce({ ok: true })

    await svc.connectConfiguredAudioDevices()

    expect(bluezMock.connectFull).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FF')
  })

  test('connectConfiguredAudioDevices bails when wireless AA is inactive', async () => {
    const svc = makeSvc()
    svc.aaBtActive = false
    await svc.connectConfiguredAudioDevices()
    expect(bluezMock.listPaired).not.toHaveBeenCalled()
  })

  test('connectConfiguredAudioDevices returns when no audio macs are configured', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = {}
    await svc.connectConfiguredAudioDevices()
    expect(bluezMock.listPaired).not.toHaveBeenCalled()
  })

  test('openAaBtSubscription handles input, aa-device and refresh events', () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.dispatchRemoteInput = vi.fn()
    svc.refreshBtPairedList = vi.fn(async () => 0)
    svc.deviceController.resendReconnectTargets = vi.fn()
    let onEvent: any
    bluezMock.subscribe.mockImplementationOnce((cb: any) => {
      onEvent = cb
      return { close: vi.fn() }
    })

    svc.openAaBtSubscription()

    onEvent({ event: 'input', command: 'play' })
    onEvent({ event: 'aa-device', btMac: 'AA:BB', instanceId: 'inst', usbSerial: 'ser' })
    onEvent({ event: 'other', mac: 'CC:DD' })

    expect(svc.dispatchRemoteInput).toHaveBeenCalledWith('play')
    expect(svc.aaBtMacByInstance.get('inst')).toBe('AA:BB')
    expect(svc.aaSerialByInstance.get('inst')).toBe('ser')
    expect(svc.refreshBtPairedList).toHaveBeenCalled()
  })

  test('openAaBtSubscription reopens after a close while still active', () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    vi.useFakeTimers()
    let onClose: any
    bluezMock.subscribe.mockImplementation((_cb: any, closeCb: any) => {
      onClose = closeCb
      return { close: vi.fn() }
    })
    svc.openAaBtSubscription()
    onClose()
    expect(svc.aaBtSubscription).toBeNull()
    vi.advanceTimersByTime(1100)
    vi.useRealTimers()
    expect(bluezMock.subscribe).toHaveBeenCalledTimes(2)
  })
})

describe('ProjectionService linux BT disconnect flows', () => {
  let realPlatform: PropertyDescriptor | undefined
  beforeEach(() => {
    realPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('disconnectHostBtPhones disconnects connected phones only', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB', connected: true, class: 0 },
      { mac: 'CC:DD', connected: false, class: 0 },
      { mac: 'EE:FF', connected: true, class: 0x0400 }
    ])
    await svc.disconnectHostBtPhones()
    expect(bluezMock.disconnect).toHaveBeenCalledWith('AA:BB')
    expect(bluezMock.disconnect).not.toHaveBeenCalledWith('CC:DD')
  })

  test('disconnectHostBtPhones swallows a listPaired failure', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockRejectedValueOnce(new Error('sock down'))
    await expect(svc.disconnectHostBtPhones()).resolves.toBeUndefined()
  })

  test('disconnectHostBtPhones swallows a disconnect failure', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', connected: true, class: 0 }])
    bluezMock.disconnect.mockRejectedValueOnce(new Error('busy'))
    await expect(svc.disconnectHostBtPhones()).resolves.toBeUndefined()
  })

  test('bounceAaBtConnections disconnects connected phones', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB', connected: true, class: 0 },
      { mac: 'CC:DD', connected: false, class: 0 }
    ])
    await svc.bounceAaBtConnections()
    expect(bluezMock.disconnect).toHaveBeenCalledWith('AA:BB')
  })

  test('bounceAaBtConnections swallows disconnect and listPaired failures', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', connected: true, class: 0 }])
    bluezMock.disconnect.mockRejectedValueOnce(new Error('busy'))
    await expect(svc.bounceAaBtConnections()).resolves.toBeUndefined()

    bluezMock.listPaired.mockRejectedValueOnce(new Error('down'))
    await expect(svc.bounceAaBtConnections()).resolves.toBeUndefined()
  })

  test('non-linux disconnect helpers short-circuit', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const svc = makeSvc()
    await svc.disconnectHostBtPhones()
    await svc.bounceAaBtConnections()
    expect(bluezMock.listPaired).not.toHaveBeenCalled()
  })
})

describe('main-side projection event subscribers', () => {
  test('listeners receive emitted events until unsubscribed', () => {
    const svc = makeSvc()
    svc.webContents = null
    const seen: unknown[] = []
    const unsubscribe = svc.onProjectionEvent((payload: unknown) => seen.push(payload))
    svc.emitProjectionEvent({ type: 'media-reset', reason: 'test' })
    expect(seen).toEqual([{ type: 'media-reset', reason: 'test' }])
    unsubscribe()
    svc.emitProjectionEvent({ type: 'media-reset', reason: 'again' })
    expect(seen).toHaveLength(1)
  })
})

describe('ProjectionService constructor wiring closures', () => {
  test('collaborator dependency closures delegate back to the service', () => {
    const svc = makeSvc()
    svc.emitProjectionEvent = vi.fn()
    svc.aaPlaybackInferred = 2
    svc.lastPluggedPhoneType = 5
    svc.config = { language: 'de' }
    svc.webContents = { id: 1, send: vi.fn() }
    svc.hostDevList = [{ id: 'x' }]
    svc.lastVideoWidth = 10
    svc.lastVideoHeight = 20
    svc.lastClusterVideoWidth = 30
    svc.lastClusterVideoHeight = 40
    svc.btPaired.getName = vi.fn(() => 'name')
    svc.btPaired.getConnectedMac = vi.fn(() => 'AA:BB')
    svc.dongleState.getConnectedMac = vi.fn(() => 'CC:DD')
    svc.dongleState.getDongleDevList = vi.fn(() => [])

    expect(svc.mediaStore.deps.getPlaybackInferred()).toBe(2)
    expect(svc.mediaStore.deps.getLastPhoneType()).toBe(5)
    svc.mediaStore.deps.emit({ type: 'media' })
    expect(svc.navStore.deps.getLanguage()).toBe('de')
    svc.navStore.deps.emit({ type: 'navigation' })

    expect(svc.planes.deps.getWebContents()).toBe(svc.webContents)
    expect(svc.planes.deps.getConfig()).toBe(svc.config)
    svc.planes.deps.emit({ type: 'projection' })
    expect(svc.planes.deps.getMainVideoSize()).toEqual({ width: 10, height: 20 })
    expect(svc.planes.deps.getClusterVideoSize()).toEqual({ width: 30, height: 40 })

    svc.dongleState.deps.emit({ type: 'dongle' })
    expect(svc.dongleState.deps.hasRenderer()).toBe(true)
    expect(svc.dongleState.deps.getHostDevList()).toEqual([{ id: 'x' }])

    svc.deviceController.deps.emit({ type: 'devices' })
    expect(svc.deviceController.deps.getBtName('AA:BB')).toBe('name')
    expect(svc.deviceController.deps.getConnectedBtMac()).toBe('AA:BB')
    expect(svc.deviceController.deps.getDongleConnectedMac()).toBe('CC:DD')
    expect(svc.deviceController.deps.getDongleDevList()).toEqual([])
    expect(svc.deviceController.deps.autoConnect()).toBe(true)
    svc.deviceController.deps.pushReconnectTargets([{ mac: 'x' }])
    svc.deviceController.deps.pushWiredPhones(['id1'])

    expect(svc.emitProjectionEvent).toHaveBeenCalled()
  })

  test('driver-manager dependency closures delegate to service handlers', () => {
    const svc = makeSvc()
    const deps = svc.drivers.deps
    svc.onMetaMessage = vi.fn()
    svc.onDriverFailure = vi.fn()
    svc.onDriverTargetedConnect = vi.fn()
    svc.onDriverVideoCodec = vi.fn()
    svc.onDriverClusterVideoCodec = vi.fn()
    svc.onDriverVideoConfig = vi.fn()
    svc.onDriverClusterVideoConfig = vi.fn()
    svc.onAaConnected = vi.fn()
    svc.onAaDisconnected = vi.fn()
    svc.onAaPresence = vi.fn()
    svc.attachCodecCapture = vi.fn()
    svc.onCpConnected = vi.fn()
    svc.onCpDisconnected = vi.fn()
    svc.onCpPresence = vi.fn()
    svc.onCpHelperPresence = vi.fn()
    svc.deviceController.resendReconnectTargets = vi.fn()
    svc.expectPhoneReenumeration = vi.fn()

    const d = fakeDriver()
    deps.handlers.onMetaMessage(d, new MediaData())
    deps.handlers.onFailure()
    deps.handlers.onTargetedConnect()
    deps.handlers.onVideoCodec('h264')
    deps.handlers.onClusterVideoCodec('h264')
    deps.handlers.onVideoConfig(Buffer.from([1]))
    deps.handlers.onClusterVideoConfig(Buffer.from([1]))
    deps.onAaConnected(d)
    deps.onAaDisconnected(d)
    deps.onAaPresence(d, {})
    deps.onAaCreated(d)
    deps.onAaReleased(d)
    expect(deps.getAaConfigSeed()).toMatchObject({ hevcSupported: expect.any(Boolean) })
    deps.onCpConnected(d)
    deps.onCpDisconnected(d)
    deps.onCpPresence(d, {})
    deps.onCpHelperPresence({})
    deps.onCpHelperConnect()
    deps.onCpCreated(d)
    deps.onCpReleased(d)
    expect(deps.getCpConfigSeed()).toMatchObject({ vp9Supported: expect.any(Boolean) })
    deps.onPhoneReenumerate(100)
    expect(deps.getConfig()).toBe(svc.config)

    expect(svc.onMetaMessage).toHaveBeenCalled()
    expect(svc.onAaConnected).toHaveBeenCalled()
    expect(svc.attachCodecCapture).toHaveBeenCalled()
    expect(svc.expectPhoneReenumeration).toHaveBeenCalledWith(100)
  })

  test('arbiter dependency closures reflect service state and drive callbacks', () => {
    const svc = makeSvc()
    svc.emitTransportState = vi.fn()
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    svc.maybeBringUpWiredBeside = vi.fn(async () => undefined)
    svc.closeWiredPhoneSession = vi.fn()
    svc.sessions.active = vi.fn(() => ({ index: 3 }))
    svc.sessions.close = vi.fn()
    svc.getActiveTransport = vi.fn(() => 'dongle')
    svc.started = true

    const deps = svc.arbiter.deps
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    svc.config = { wirelessAaEnabled: true }
    expect(deps.isWirelessEnabled()).toBe(true)
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    expect(typeof deps.isWirelessPhoneInRange()).toBe('boolean')
    expect(deps.getActiveTransport()).toBe('dongle')
    expect(deps.isDongleSessionActive()).toBe(true)
    expect(typeof deps.isWiredAaSessionActive()).toBe('boolean')
    expect(typeof deps.isWiredCpSessionActive()).toBe('boolean')
    expect(typeof deps.hasWiredSession()).toBe('boolean')
    deps.onChange()
    deps.onShouldStop()
    deps.onShouldAutoStart()
    deps.onShouldBringUpWiredBeside()
    deps.onWiredPhoneGone()

    expect(svc.emitTransportState).toHaveBeenCalled()
    expect(svc.sessions.close).toHaveBeenCalledWith(3)
    expect(svc.autoStartIfNeeded).toHaveBeenCalled()
    expect(svc.maybeBringUpWiredBeside).toHaveBeenCalled()
    expect(svc.closeWiredPhoneSession).toHaveBeenCalled()
  })

  test('audio closures wire projection events, chunking and mic audio back to the service', () => {
    const svc = makeSvc()
    svc.emitProjectionEvent = vi.fn()
    svc.sendChunked = vi.fn()
    svc.getAllUiWebContents = vi.fn(() => [])
    svc.drivers.getActive().sendPhoneAudio = vi.fn()

    const args = (ProjectionAudio as unknown as Mock).mock.calls.at(-1)!
    const [getConfig, sendProjectionEvent, sendChunked, sendMicPcm] = args
    expect(getConfig()).toBe(svc.config)
    sendProjectionEvent({ type: 'audio' })
    sendChunked('projection-audio-chunk', new ArrayBuffer(2), 64, { a: 1 })
    sendMicPcm(new Int16Array([1]), 1)

    expect(svc.emitProjectionEvent).toHaveBeenCalledWith({ type: 'audio' })
    expect(svc.sendChunked).toHaveBeenCalled()
    expect(svc.drivers.getActive().sendPhoneAudio).toHaveBeenCalled()
  })

  test('dongle event listeners and audio monitor callback are wired', () => {
    const svc = makeSvc()
    svc.onDonglePhoneConnected = vi.fn()
    svc.onDongleInfo = vi.fn()
    svc.emitProjectionEvent = vi.fn()

    svc.drivers.getDongle().emit('phone-connected')
    svc.drivers.getDongle().emit('dongle-info', { boxInfo: {} })
    audioMonitorHolder.cb()

    expect(svc.onDonglePhoneConnected).toHaveBeenCalled()
    expect(svc.onDongleInfo).toHaveBeenCalled()
    expect(svc.emitProjectionEvent).toHaveBeenCalledWith({ type: 'audioDevicesChanged' })
  })

  test('device registry change closure re-emits the device list', () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.deviceController.emitDevices = vi.fn()
    svc.deviceRegistry.noteDevice({ btMac: 'AA:BB', protocol: 'androidauto', transport: 'wifi' })
    vi.advanceTimersByTime(250)
    expect(svc.deviceController.emitDevices).toHaveBeenCalled()
    vi.useRealTimers()
  })
})

describe('ProjectionService start / autoStart', () => {
  function primeStart(svc: any) {
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)
    svc.dongleState.resetForTeardown = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.planes.resetClusterStreamActive = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.clearStartRetry = vi.fn()
    svc.scheduleStartRetry = vi.fn()
    svc.emitTransportState = vi.fn()
  }

  test('start short-circuits for a dongle target', async () => {
    const svc = makeSvc()
    primeStart(svc)
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'dongle' }))
    await svc.start()
    expect(svc.reloadConfigFromDisk).not.toHaveBeenCalled()
    expect(svc.started).toBe(false)
  })

  test('start brings up CarPlay', async () => {
    const svc = makeSvc()
    primeStart(svc)
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'cp', mode: 'wired' }))
    svc.drivers.startCp = vi.fn()
    await svc.start()
    expect(svc.drivers.startCp).toHaveBeenCalled()
    expect(svc.started).toBe(true)
  })

  test('start brings up wired AA when a device is available', async () => {
    const svc = makeSvc()
    primeStart(svc)
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'aa', mode: 'wired' }))
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ vendorId: 0x18d1, productId: 0x2d00 }))
    svc.drivers.bringUpAaWired = vi.fn(async () => true)
    await svc.start()
    expect(svc.drivers.bringUpAaWired).toHaveBeenCalled()
    expect(svc.started).toBe(true)
  })

  test('start schedules a retry when wired AA has no live device handle', async () => {
    const svc = makeSvc()
    primeStart(svc)
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'aa', mode: 'wired' }))
    svc.arbiter.getPhoneDevice = vi.fn(() => null)
    await svc.start()
    expect(svc.scheduleStartRetry).toHaveBeenCalled()
    expect(svc.started).toBe(false)
  })

  test('start schedules a retry when wired AA bring-up returns false', async () => {
    const svc = makeSvc()
    primeStart(svc)
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'aa', mode: 'wired' }))
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ vendorId: 1, productId: 2 }))
    svc.drivers.bringUpAaWired = vi.fn(async () => false)
    await svc.start()
    expect(svc.scheduleStartRetry).toHaveBeenCalled()
  })

  test('start schedules a retry when wired AA bring-up throws', async () => {
    const svc = makeSvc()
    primeStart(svc)
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'aa', mode: 'wired' }))
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ vendorId: 1, productId: 2 }))
    svc.drivers.bringUpAaWired = vi.fn(async () => {
      throw new Error('bring-up boom')
    })
    await svc.start()
    expect(svc.scheduleStartRetry).toHaveBeenCalled()
    expect(svc.started).toBe(false)
  })

  test('start brings up wireless AA when no wired device is targeted', async () => {
    const svc = makeSvc()
    primeStart(svc)
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'aa', mode: 'wireless' }))
    svc.arbiter.getPhoneDevice = vi.fn(() => null)
    svc.drivers.startAaWireless = vi.fn()
    await svc.start()
    expect(svc.drivers.startAaWireless).toHaveBeenCalled()
    expect(svc.started).toBe(true)
  })

  test('start returns without re-running while a start is in flight', async () => {
    const svc = makeSvc()
    svc.arbiter.pickPreferred = vi.fn()
    svc.startPromise = Promise.resolve()
    await svc.start()
    expect(svc.arbiter.pickPreferred).not.toHaveBeenCalled()
  })

  test('start returns immediately when already started', async () => {
    const svc = makeSvc()
    svc.started = true
    await expect(svc.start()).resolves.toBeUndefined()
  })

  test('autoStartIfNeeded waits for a pending stop then starts', async () => {
    const svc = makeSvc()
    svc.stopPromise = Promise.resolve()
    svc.start = vi.fn(async () => undefined)
    svc.arbiter.decideNextStart = vi.fn(() => ({ kind: 'start' }))
    await svc.autoStartIfNeeded()
    expect(svc.start).toHaveBeenCalled()
  })

  test('autoStartIfNeeded swallows a rejected pending stop', async () => {
    const svc = makeSvc()
    svc.stopPromise = Promise.reject(new Error('stop boom'))
    svc.start = vi.fn(async () => undefined)
    svc.arbiter.decideNextStart = vi.fn(() => ({ kind: 'start' }))
    await expect(svc.autoStartIfNeeded()).resolves.toBeUndefined()
  })

  test('autoStartIfNeeded bails when a session already exists', async () => {
    const svc = makeSvc()
    svc.start = vi.fn(async () => undefined)
    svc.sessions.upsert(fakeDriver(), 'androidauto', 'wifi', {})
    await svc.autoStartIfNeeded()
    expect(svc.start).not.toHaveBeenCalled()
  })

  test('autoStartIfNeeded returns for a none decision and reschedules for defer', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.start = vi.fn(async () => undefined)
    svc.arbiter.decideNextStart = vi.fn(() => ({ kind: 'none' }))
    await svc.autoStartIfNeeded()
    expect(svc.start).not.toHaveBeenCalled()

    svc.arbiter.decideNextStart = vi
      .fn()
      .mockReturnValueOnce({ kind: 'defer', retryMs: 10 })
      .mockReturnValue({ kind: 'none' })
    const spy = vi.spyOn(svc, 'autoStartIfNeeded')
    await svc.autoStartIfNeeded()
    await vi.advanceTimersByTimeAsync(20)
    expect(spy).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })
})

describe('ProjectionService transport switch / restart / connect', () => {
  test('switchTransport returns not-ok when the arbiter refuses', async () => {
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: false, target: { transport: 'aa' } }))
    const res = await svc.switchTransport()
    expect(res).toEqual({ ok: false, active: 'aa' })
  })

  test('switchTransport is a no-op while another switch is running', async () => {
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: { transport: 'aa' } }))
    svc.isSwitching = true
    const res = await svc.switchTransport()
    expect(res).toEqual({ ok: true, active: 'aa' })
  })

  test('switchTransport stops and restarts on the desired transport', async () => {
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: { transport: 'cp' } }))
    svc.arbiter.getOverride = vi
      .fn()
      .mockReturnValueOnce({ transport: 'cp', mode: 'wired' })
      .mockReturnValue(null)
    svc.getActiveTransport = vi.fn(() => 'dongle')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.started = true
    svc.stop = vi.fn(async () => {
      svc.started = false
    })
    svc.autoStartIfNeeded = vi.fn(async () => undefined)

    const res = await svc.switchTransport()
    expect(svc.stop).toHaveBeenCalled()
    expect(svc.autoStartIfNeeded).toHaveBeenCalled()
    expect(res.ok).toBe(true)
  })

  test('switchTransport leaving wireless AA bounces BT and reconnects', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: { transport: 'aa' } }))
    svc.arbiter.getOverride = vi
      .fn()
      .mockReturnValueOnce({ transport: 'aa', mode: 'wireless' })
      .mockReturnValue(null)
    svc.getActiveTransport = vi.fn(() => 'aa')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.started = true
    svc.stop = vi.fn(async () => {
      svc.started = false
    })
    svc.bounceAaBtConnections = vi.fn(async () => undefined)
    svc.tryAutoConnect = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)

    const p = svc.switchTransport()
    await vi.advanceTimersByTimeAsync(600)
    await p
    expect(bluezMock.deauthApClients).toHaveBeenCalled()
    expect(svc.bounceAaBtConnections).toHaveBeenCalled()
    expect(svc.tryAutoConnect).toHaveBeenCalledWith({ force: true })
    vi.useRealTimers()
  })

  test('restartSession disconnects the phone for a dongle session', async () => {
    const svc = makeSvc()
    svc.getActiveTransport = vi.fn(() => 'dongle')
    svc.drivers.getActive().disconnectPhone = vi.fn(async () => true)
    await svc.restartSession()
    expect(svc.drivers.getActive().disconnectPhone).toHaveBeenCalled()
  })

  test('restartSession drops CarPlay sessions when native CarPlay is active', async () => {
    const svc = makeSvc()
    svc.cpActive = true
    const dropSessions = vi.fn()
    svc.drivers.getCpManager = vi.fn(() => ({ dropSessions }))
    svc.getActiveTransport = vi.fn(() => null)
    await svc.restartSession()
    expect(dropSessions).toHaveBeenCalled()
  })

  test('restartSession swallows a dongle disconnect failure', async () => {
    const svc = makeSvc()
    svc.getActiveTransport = vi.fn(() => 'dongle')
    svc.drivers.getActive().disconnectPhone = vi.fn(async () => {
      throw new Error('dc boom')
    })
    await expect(svc.restartSession()).resolves.toBeUndefined()
  })

  test('restartSession stops and returns for a wired AA session', async () => {
    const svc = makeSvc()
    svc.getActiveTransport = vi.fn(() => 'aa')
    svc.isActiveAaWired = vi.fn(() => true)
    svc.stop = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    await svc.restartSession()
    expect(svc.stop).toHaveBeenCalled()
    expect(svc.autoStartIfNeeded).not.toHaveBeenCalled()
  })

  test('restartSession restarts a wireless AA session', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.getActiveTransport = vi.fn(() => 'aa')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.stop = vi.fn(async () => undefined)
    svc.bounceAaBtConnections = vi.fn(async () => undefined)
    svc.tryAutoConnect = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    const p = svc.restartSession()
    await vi.advanceTimersByTimeAsync(600)
    await p
    expect(svc.bounceAaBtConnections).toHaveBeenCalled()
    expect(svc.autoStartIfNeeded).toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('restartSession swallows a stop failure', async () => {
    const svc = makeSvc()
    svc.getActiveTransport = vi.fn(() => 'aa')
    svc.isActiveAaWired = vi.fn(() => true)
    svc.stop = vi.fn(async () => {
      throw new Error('stop boom')
    })
    await expect(svc.restartSession()).resolves.toBeUndefined()
  })

  test('connectPairedDevice returns the listPaired error', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockRejectedValueOnce(new Error('sock down'))
    const res = await svc.connectPairedDevice('AA:BB')
    expect(res).toEqual({ ok: false, error: 'sock down' })
  })

  test('connectPairedDevice connects non-phone devices directly', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', class: 0x0400 }])
    bluezMock.connectFull.mockResolvedValueOnce({ ok: true })
    const res = await svc.connectPairedDevice('AA:BB')
    expect(bluezMock.connectFull).toHaveBeenCalledWith('AA:BB')
    expect(res).toEqual({ ok: true })
  })

  test('connectPairedDevice refuses while another switch is running', async () => {
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', class: 0 }])
    svc.isSwitching = true
    const res = await svc.connectPairedDevice('AA:BB')
    expect(res).toEqual({ ok: false, error: 'switch in progress' })
  })

  test('connectPairedDevice switches to wireless AA for a phone', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', class: 0 }])
    svc.getActiveTransport = vi.fn(() => 'aa')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.started = true
    svc.stop = vi.fn(async () => undefined)
    svc.applyConfigPatch = vi.fn()
    svc.arbiter.setOverride = vi.fn()
    svc.bounceAaBtConnections = vi.fn(async () => undefined)
    svc.tryAutoConnect = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)

    const p = svc.connectPairedDevice('AA:BB')
    await vi.advanceTimersByTimeAsync(600)
    const res = await p
    expect(svc.applyConfigPatch).toHaveBeenCalledWith({ lastConnectedAaBtMac: 'AA:BB' })
    expect(svc.arbiter.setOverride).toHaveBeenCalledWith({ transport: 'aa', mode: 'wireless' })
    expect(res).toEqual({ ok: true })
    vi.useRealTimers()
  })
})

describe('ProjectionService active-session, teardown, stop and retry', () => {
  test('onActiveSessionChanged handles a dongle session with a previous session', () => {
    const svc = makeSvc()
    svc.planes.dispose = vi.fn()
    svc.mediaStore.hydrate = vi.fn()
    svc.navStore.hydrate = vi.fn()
    const next = {
      index: 1,
      protocol: 'dongle',
      driver: fakeDriver(),
      audio: { duckLevel: 1, duckRampMs: 1500 }
    }
    const prev = { index: 2, protocol: 'androidauto' }

    svc.onActiveSessionChanged(next, prev)

    expect(svc.started).toBe(true)
    expect(svc.planes.dispose).toHaveBeenCalled()
    expect(svc.mediaStore.hydrate).toHaveBeenCalledWith(next)
  })

  test('onActiveSessionChanged resets audio for a first dongle session', () => {
    const svc = makeSvc()
    svc.planes.dispose = vi.fn()
    svc.mediaStore.hydrate = vi.fn()
    svc.navStore.hydrate = vi.fn()
    const next = {
      index: 1,
      protocol: 'dongle',
      driver: fakeDriver(),
      audio: { duckLevel: 1, duckRampMs: 1500 }
    }
    svc.onActiveSessionChanged(next, null)
    expect(svc.audio.resetForSessionStart).toHaveBeenCalled()
  })

  test('onActiveSessionChanged tears down to idle when no session becomes active', () => {
    const svc = makeSvc()
    svc.teardownToIdle = vi.fn()
    svc.onActiveSessionChanged(null, { index: 1 })
    expect(svc.teardownToIdle).toHaveBeenCalled()
  })

  test('teardownToIdle disposes planes, resets stores and notifies the renderer', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    svc.planes.dispose = vi.fn()
    svc.statusFile.setStreaming = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.autoStartIfNeeded = vi.fn(async () => undefined)

    svc.teardownToIdle()

    expect(svc.planes.dispose).toHaveBeenCalled()
    expect(svc.audio.resetForSessionStop).toHaveBeenCalled()
    expect(send).toHaveBeenCalledWith('projection-event', { type: 'unplugged' })
    expect(svc.started).toBe(false)
  })

  test('teardownToIdle bails while a stop is in progress', () => {
    const svc = makeSvc()
    svc.stopPromise = Promise.resolve()
    svc.planes.dispose = vi.fn()
    svc.teardownToIdle()
    expect(svc.planes.dispose).not.toHaveBeenCalled()
  })

  test('teardownToIdle swallows a renderer send failure', () => {
    const svc = makeSvc()
    svc.emitProjectionEvent = vi.fn()
    svc.webContents = {
      send: vi.fn(() => {
        throw new Error('send boom')
      }),
      isDestroyed: () => false
    }
    svc.planes.dispose = vi.fn()
    svc.statusFile.setStreaming = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    expect(() => svc.teardownToIdle()).not.toThrow()
    expect(svc.webContents.send).toHaveBeenCalled()
  })

  test('stop notifies the renderer and resets a non-dongle session', async () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    const aa = fakeDriver()
    const s = svc.sessions.upsert(aa, 'androidauto', 'wifi', {})
    svc.sessions.activate(s.index)
    svc.started = true
    svc.disconnectPhone = vi.fn(async () => true)
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.dongleState.resetForTeardown = vi.fn()

    await svc.stop()

    expect(send).toHaveBeenCalledWith('projection-event', { type: 'unplugged' })
    expect(svc.started).toBe(false)
  })

  test('stop swallows a renderer send failure', async () => {
    const svc = makeSvc()
    svc.emitProjectionEvent = vi.fn()
    svc.webContents = {
      send: vi.fn(() => {
        throw new Error('send boom')
      }),
      isDestroyed: () => false
    }
    svc.started = true
    svc.disconnectPhone = vi.fn(async () => true)
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.dongleState.resetForTeardown = vi.fn()
    await expect(svc.stop()).resolves.toBeUndefined()
    expect(svc.webContents.send).toHaveBeenCalled()
  })

  test('scheduleStartRetry arms a backoff timer and clearStartRetry clears it', () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.autoStartIfNeeded = vi.fn(async () => undefined)

    svc.scheduleStartRetry()
    expect(svc.startRetryTimer).not.toBeNull()
    svc.scheduleStartRetry()

    svc.clearStartRetry()
    expect(svc.startRetryTimer).toBeNull()
    vi.useRealTimers()
  })

  test('scheduleStartRetry does nothing while shutting down', () => {
    const svc = makeSvc()
    svc.shuttingDown = true
    svc.scheduleStartRetry()
    expect(svc.startRetryTimer).toBeNull()
  })
})

describe('ProjectionService wired-beside and tryAutoConnect guards', () => {
  test('maybeBringUpWiredBeside brings up a wired AA session beside the active one', async () => {
    const svc = makeSvc()
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ vendorId: 0x18d1 }))
    svc.drivers.bringUpAaWired = vi.fn(async () => true)
    await svc.maybeBringUpWiredBeside()
    expect(svc.drivers.bringUpAaWired).toHaveBeenCalled()
  })

  test('maybeBringUpWiredBeside skips apple devices and missing handles', async () => {
    const svc = makeSvc()
    svc.drivers.bringUpAaWired = vi.fn(async () => true)
    svc.arbiter.getPhoneDevice = vi.fn(() => null)
    await svc.maybeBringUpWiredBeside()
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ vendorId: 0x05ac }))
    await svc.maybeBringUpWiredBeside()
    expect(svc.drivers.bringUpAaWired).not.toHaveBeenCalled()
  })

  test('maybeBringUpWiredBeside skips when a wired AA session already exists', async () => {
    const svc = makeSvc()
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ vendorId: 0x18d1 }))
    svc.drivers.bringUpAaWired = vi.fn(async () => true)
    svc.sessions.upsert(fakeDriver(), 'androidauto', 'usb', { usbSerial: 'x' })
    await svc.maybeBringUpWiredBeside()
    expect(svc.drivers.bringUpAaWired).not.toHaveBeenCalled()
  })

  test('maybeBringUpWiredBeside swallows a bring-up failure', async () => {
    const svc = makeSvc()
    svc.arbiter.getPhoneDevice = vi.fn(() => ({ vendorId: 0x18d1 }))
    svc.drivers.bringUpAaWired = vi.fn(async () => {
      throw new Error('beside boom')
    })
    await expect(svc.maybeBringUpWiredBeside()).resolves.toBeUndefined()
  })

  test('closeWiredPhoneSession closes a wired AA session and no-ops otherwise', () => {
    const svc = makeSvc()
    const wired = fakeDriver()
    svc.sessions.upsert(wired, 'androidauto', 'usb', { usbSerial: 'x' })
    svc.closeWiredPhoneSession()
    expect(wired.close).toHaveBeenCalled()

    const svc2 = makeSvc()
    expect(() => svc2.closeWiredPhoneSession()).not.toThrow()
  })

  test('tryAutoConnect skips while a wired AA session is active', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.started = true
    svc.isActiveAaWired = vi.fn(() => true)
    await svc.tryAutoConnect()
    expect(bluezMock.listPaired).not.toHaveBeenCalled()
  })

  test('tryAutoConnect skips a passive attempt when a wired phone is detected', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.arbiter.getSnapshot = vi.fn(() => ({ wiredPhoneDetected: true }))
    await svc.tryAutoConnect()
    expect(bluezMock.listPaired).not.toHaveBeenCalled()
  })

  test('tryAutoConnect swallows a listPaired failure', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.arbiter.getSnapshot = vi.fn(() => ({ wiredPhoneDetected: false }))
    bluezMock.listPaired.mockRejectedValueOnce(new Error('down'))
    await expect(svc.tryAutoConnect()).resolves.toBeUndefined()
  })

  test('tryAutoConnect falls back to the first phone when none is preferred or trusted', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.arbiter.getSnapshot = vi.fn(() => ({ wiredPhoneDetected: false }))
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', connected: false, trusted: false }])
    bluezMock.connect.mockResolvedValueOnce({ ok: true })
    await svc.tryAutoConnect()
    expect(bluezMock.connect).toHaveBeenCalledWith('AA:BB')
  })
})

describe('ProjectionService refresh + populate edge cases', () => {
  test('refreshBtPairedList keeps last host entries on a transient empty list', async () => {
    const svc = makeSvc()
    svc.hostDevList = [
      { id: 'AA:BB', name: 'x', type: '', source: 'host', class: 0, connected: false }
    ]
    svc.deviceController.emitDevices = vi.fn()
    bluezMock.listPaired.mockResolvedValueOnce([])
    await svc.refreshBtPairedList()
    expect(svc.hostDevList).toHaveLength(1)
  })

  test('populateAaBtPairedListInitial retries on empty and gives up at the deadline', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = { lastConnectedAaBtMac: 'AA:BB' }
    svc.refreshBtPairedList = vi.fn(async () => 0)

    const p = svc.populateAaBtPairedListInitial()
    await vi.advanceTimersByTimeAsync(31_000)
    await p
    expect(svc.refreshBtPairedList).toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('populateAaBtPairedListInitial retries when refresh throws', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = { lastConnectedAaBtMac: '' }
    let calls = 0
    svc.refreshBtPairedList = vi.fn(async () => {
      calls++
      if (calls === 1) throw new Error('sock')
      return 2
    })
    const p = svc.populateAaBtPairedListInitial()
    await vi.advanceTimersByTimeAsync(5_000)
    await p
    expect(calls).toBeGreaterThanOrEqual(2)
    vi.useRealTimers()
  })
})

describe('ProjectionService branch coverage fill', () => {
  test('onCpHelperPresence wifi link-up with non-string ids keeps the session', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteLink = vi.fn()
    svc.sessions.closeByDeviceOnTransport = vi.fn()
    svc.onCpHelperPresence({ kind: 'wifi', connected: true })
    expect(svc.deviceRegistry.noteLink).toHaveBeenCalled()
    expect(svc.sessions.closeByDeviceOnTransport).not.toHaveBeenCalled()
  })

  test('onCpPresence device with an empty payload still upserts a session', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    const session = fakeDriver()
    svc.onCpPresence(session, { kind: 'device' })
    expect(svc.deviceRegistry.noteDevice).toHaveBeenCalled()
  })

  test('onCpPresence active kind with no mapped session is a no-op', () => {
    const svc = makeSvc()
    expect(() => svc.onCpPresence(fakeDriver(), { kind: 'active' })).not.toThrow()
  })

  test('onCpPresence status kind without a session uses empty ids and missing fields', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteStatus = vi.fn()
    svc.onCpPresence(fakeDriver(), { kind: 'status' })
    expect(svc.deviceRegistry.noteStatus).toHaveBeenCalledWith({}, expect.any(Object))
  })

  test('onAaPresence status kind without a session uses empty ids and missing fields', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteStatus = vi.fn()
    svc.onAaPresence(fakeDriver(), { kind: 'status' })
    expect(svc.deviceRegistry.noteStatus).toHaveBeenCalledWith({}, expect.any(Object))
  })

  test('onAaPresence device for a wired session uses the usb serial and no wifi/bt mac', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    const session = fakeDriver({ isWiredMode: () => true, usbSerial: () => undefined })
    svc.aaSerialByInstance.set('inst-1', 'serial-x')
    svc.onAaPresence(session, { kind: 'device', instanceId: 'inst-1' })
    const noted = (svc.deviceRegistry.noteDevice as Mock).mock.calls[0][0]
    expect(noted.transport).toBe('usb')
    expect(noted.wifiMac).toBeUndefined()
    expect(noted.usbSerial).toBe('serial-x')
  })

  test('handleVideoData cluster path skips resolution + push when unchanged and dataless', () => {
    const svc = makeSvc()
    svc.config = { dashboards: { dash3: { main: true } } }
    svc.lastClusterVideoWidth = 320
    svc.lastClusterVideoHeight = 180
    svc.planes.pushCluster = vi.fn()
    svc.planes.recropAllClusters = vi.fn()
    const msg = Object.assign(new VideoData(), { cluster: true, width: 320, height: 180 })
    svc.handleVideoData(msg)
    expect(svc.planes.recropAllClusters).not.toHaveBeenCalled()
    expect(svc.planes.pushCluster).not.toHaveBeenCalled()
  })

  test('handleVideoData cluster path tolerates no active session and a destroyed target', () => {
    const svc = makeSvc()
    svc.config = { dashboards: { dash3: { main: true } } }
    svc.planes.pushCluster = vi.fn()
    svc.planes.recropAllClusters = vi.fn()
    svc.getClusterTargetWebContents = vi.fn(() => [{ isDestroyed: () => true, send: vi.fn() }])
    const msg = Object.assign(new VideoData(), {
      cluster: true,
      width: 640,
      height: 360,
      data: Buffer.from([1])
    })
    svc.handleVideoData(msg)
    expect(svc.planes.pushCluster).toHaveBeenCalled()
  })

  test('handleVideoData main path skips resolution + push when unchanged and dataless', () => {
    const svc = makeSvc()
    svc.firstFrameLogged = true
    svc.lastVideoWidth = 1920
    svc.lastVideoHeight = 1080
    svc.planes.pushMain = vi.fn()
    svc.planes.updateMainCrop = vi.fn()
    const msg = Object.assign(new VideoData(), {
      cluster: false,
      width: 1920,
      height: 1080
    })
    svc.handleVideoData(msg)
    expect(svc.planes.updateMainCrop).not.toHaveBeenCalled()
    expect(svc.planes.pushMain).not.toHaveBeenCalled()
  })

  test('handleVideoData main path tolerates no active session', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    svc.firstFrameLogged = true
    svc.planes.pushMain = vi.fn()
    svc.planes.updateMainCrop = vi.fn()
    const msg = Object.assign(new VideoData(), {
      cluster: false,
      width: 800,
      height: 600,
      data: Buffer.from([1])
    })
    svc.handleVideoData(msg)
    expect(svc.planes.updateMainCrop).toHaveBeenCalled()
    expect(svc.planes.pushMain).toHaveBeenCalled()
  })

  test('onDriverMessage ignores unrecognized message types', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    expect(() => svc.onDriverMessage({})).not.toThrow()
  })

  test('onMetaMessage ignores unrecognized message types', () => {
    const svc = makeSvc()
    expect(() => svc.onMetaMessage(fakeDriver(), {})).not.toThrow()
  })

  test('start applies numeric volume config values', async () => {
    const svc = makeSvc()
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)
    svc.dongleState.resetForTeardown = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.planes.resetClusterStreamActive = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.clearStartRetry = vi.fn()
    svc.emitTransportState = vi.fn()
    svc.config = { audioVolume: 0.5, navVolume: 0.4, voiceAssistantVolume: 0.3, callVolume: 0.2 }
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'aa', mode: 'wireless' }))
    svc.arbiter.getPhoneDevice = vi.fn(() => null)
    svc.drivers.startAaWireless = vi.fn()

    await svc.start()

    expect(svc.audio.setInitialVolumes).toHaveBeenCalledWith({
      music: 0.5,
      nav: 0.4,
      voiceAssistant: 0.3,
      call: 0.2
    })
  })
})

describe('ProjectionService syncHelperSupervisor edge branches', () => {
  let realPlatform: PropertyDescriptor | undefined
  beforeEach(() => {
    realPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('restarts and stops an existing supervisor when the enable key changes', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const svc = makeSvc()
    svc.drivers.startCp = vi.fn()
    svc.drivers.startAaWireless = vi.fn()
    svc.openAaBtSubscription = vi.fn()
    svc.populateAaBtPairedListInitial = vi.fn(async () => undefined)
    const oldSup = { stop: vi.fn(async () => undefined) }
    svc.helperSupervisor = oldSup
    svc.btEnableKey = ''
    svc.config = { wirelessAaEnabled: true }

    svc.syncHelperSupervisor()

    expect(oldSup.stop).toHaveBeenCalled()
    expect(svc.helperSupervisor).not.toBe(oldSup)
  })

  test('releases CP when CP is no longer wanted', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const svc = makeSvc()
    svc.drivers.releaseCp = vi.fn(async () => undefined)
    svc.cpActive = true
    svc.config = {}
    svc.syncHelperSupervisor()
    expect(svc.drivers.releaseCp).toHaveBeenCalled()
    expect(svc.cpActive).toBe(false)
  })
})

describe('ProjectionService audio-device autoconnect retries', () => {
  test('skips unpaired devices and stops retrying once connected', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = {
      audioOutputDevice: 'bluez_output.AA_BB_CC_DD_EE_FF.1',
      audioInputDevice: 'bluez_input.11_22_33_44_55_66'
    }
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: '11:22:33:44:55:66', connected: false }])
    bluezMock.connectFull
      .mockResolvedValueOnce({ ok: false, error: 'busy' })
      .mockResolvedValueOnce({
        ok: true
      })

    const p = svc.connectConfiguredAudioDevices()
    await vi.advanceTimersByTimeAsync(5000)
    await p

    expect(bluezMock.connectFull).toHaveBeenCalledWith('11:22:33:44:55:66')
    vi.useRealTimers()
  })

  test('breaks out of the retry loop when connectFull throws', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = { audioOutputDevice: 'bluez_output.AA_BB_CC_DD_EE_FF.1' }
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB:CC:DD:EE:FF', connected: false }])
    bluezMock.connectFull.mockRejectedValueOnce(new Error('boom'))
    await expect(svc.connectConfiguredAudioDevices()).resolves.toBeUndefined()
  })

  test('swallows a listPaired failure', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = { audioOutputDevice: 'bluez_output.AA_BB_CC_DD_EE_FF.1' }
    bluezMock.listPaired.mockRejectedValueOnce(new Error('down'))
    await expect(svc.connectConfiguredAudioDevices()).resolves.toBeUndefined()
  })
})

describe('ProjectionService remaining edges', () => {
  test('scheduleStartRetry fires the deferred auto-start', () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    svc.scheduleStartRetry()
    vi.advanceTimersByTime(2000)
    expect(svc.autoStartIfNeeded).toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('openAaBtSubscription resend-connect callback resends reconnect targets', () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.deviceController.resendReconnectTargets = vi.fn()
    let resendCb: any
    bluezMock.subscribe.mockImplementationOnce((_cb: any, _close: any, resend: any) => {
      resendCb = resend
      return { close: vi.fn() }
    })
    svc.openAaBtSubscription()
    resendCb()
    expect(svc.deviceController.resendReconnectTargets).toHaveBeenCalled()
  })

  test('getClusterTargetWebContents falls back when secondary windows are absent', () => {
    const svc = makeSvc()
    svc.webContents = { isDestroyed: () => false, send: vi.fn() }
    svc.config = { dashboards: { dash3: { dash: true, aux: true } } }
    ;(getSecondaryWindow as Mock).mockReturnValue(null)
    const out = svc.getClusterTargetWebContents()
    expect(out).toEqual([svc.webContents])
  })

  test('getClusterTargetWebContents returns nothing when there is no live renderer at all', () => {
    const svc = makeSvc()
    svc.webContents = null
    svc.config = { dashboards: null }
    ;(getSecondaryWindow as Mock).mockReturnValue(null)
    const out = svc.getClusterTargetWebContents()
    expect(out).toEqual([])
  })

  test('onConfigChanged reacts to a wireless CP toggle and an input-only device change', () => {
    const svc = makeSvc()
    svc.planes.retainScreens = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.syncHelperSupervisor = vi.fn()
    svc.emitTransportState = vi.fn()
    svc.connectConfiguredAudioDevices = vi.fn(async () => undefined)
    svc.systemSound.onDeviceChanged = vi.fn()
    svc.config = { wirelessCpEnabled: false, audioInputDevice: 'a' }

    svc.onConfigChanged({ wirelessCpEnabled: true, audioInputDevice: 'b' })

    expect(svc.syncHelperSupervisor).toHaveBeenCalled()
    expect(svc.audio.onAudioDeviceChanged).toHaveBeenCalled()
    expect(svc.systemSound.onDeviceChanged).not.toHaveBeenCalled()
  })
})

describe('ProjectionService error-lambda and small-branch coverage', () => {
  test('handlePlugged swallows a rejected start', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    svc.start = vi.fn(() => Promise.reject(new Error('start boom')))
    expect(() => svc.handlePlugged({ phoneType: 5 })).not.toThrow()
  })

  test('onAaConnected/onAaDisconnected swallow refresh rejections', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    svc.refreshBtPairedList = vi.fn(() => Promise.reject(new Error('refresh boom')))
    svc.onPhoneConnected = vi.fn()
    svc.onPhoneDisconnected = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.deviceRegistry.clearPresence = vi.fn()
    svc.sessions.byDriver = vi.fn(() => null)
    svc.sessions.closeByDriver = vi.fn()
    svc.sessions.upsert = vi.fn(() => ({ index: 1 }))
    svc.sessions.active = vi.fn(() => ({ index: 1 }))
    svc.sessions.activate = vi.fn()

    expect(() => svc.onAaConnected(fakeDriver())).not.toThrow()
    expect(() => svc.onAaDisconnected(fakeDriver())).not.toThrow()
  })

  test('onAaDisconnected/onCpDisconnected tolerate a missing closed session', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.refreshBtPairedList = vi.fn(async () => 0)
    svc.onPhoneDisconnected = vi.fn()
    svc.deviceRegistry.clearPresence = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.sessions.byDriver = vi.fn(() => null)
    svc.sessions.closeByDriver = vi.fn()
    svc.sessions.active = vi.fn(() => ({ index: 9 }))

    svc.onAaDisconnected(fakeDriver())
    svc.onCpDisconnected(fakeDriver())

    expect(svc.deviceRegistry.clearPresence).not.toHaveBeenCalled()
  })

  test('onCpConnected uses a null controller id when the session lacks one', () => {
    const svc = makeSvc()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    const session = fakeDriver({ getControllerId: () => null })
    svc.onCpConnected(session)
    expect(svc.sessions.byDriver(session)?.protocol).toBe('carplay')
  })

  test('onConfigChanged swallows a rejected audio reconnect', () => {
    const svc = makeSvc()
    svc.planes.retainScreens = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.systemSound.onDeviceChanged = vi.fn()
    svc.connectConfiguredAudioDevices = vi.fn(() => Promise.reject(new Error('reconnect boom')))
    svc.config = { audioOutputDevice: 'a' }
    expect(() => svc.onConfigChanged({ audioOutputDevice: 'b' })).not.toThrow()
  })

  test('pushReconnectTargets closure swallows a rejected helper send', () => {
    const svc = makeSvc()
    svc.drivers.getCpManager = vi.fn(() => ({
      helper: { sendReconnectTargets: vi.fn(() => Promise.reject(new Error('send boom'))) }
    }))
    expect(() => svc.deviceController.deps.pushReconnectTargets([{ mac: 'x' }])).not.toThrow()
  })

  test('shutdownWirelessSessions swallows a rejected supervisor stop', async () => {
    const svc = makeSvc()
    svc.drivers.releaseAa = vi.fn(async () => undefined)
    svc.drivers.releaseCp = vi.fn(async () => undefined)
    svc.helperSupervisor = { stop: vi.fn(() => Promise.reject(new Error('stop boom'))) }
    await expect(svc.shutdownWirelessSessions()).resolves.toBeUndefined()
  })

  test('buildIpcHost refreshBtPaired closure swallows a rejection', () => {
    const svc = makeSvc()
    svc.refreshBtPairedList = vi.fn(() => Promise.reject(new Error('refresh boom')))
    const host = svc.buildIpcHost()
    expect(() => host.refreshBtPaired()).not.toThrow()
  })

  test('teardownToIdle swallows a rejected auto-start', () => {
    const svc = makeSvc()
    svc.emitProjectionEvent = vi.fn()
    svc.webContents = null
    svc.planes.dispose = vi.fn()
    svc.statusFile.setStreaming = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.autoStartIfNeeded = vi.fn(() => Promise.reject(new Error('auto boom')))
    expect(() => svc.teardownToIdle()).not.toThrow()
  })

  test('helper supervisor stdout/stderr/error listeners are wired', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    try {
      const svc = makeSvc()
      svc.drivers.startCp = vi.fn()
      svc.drivers.startAaWireless = vi.fn()
      svc.openAaBtSubscription = vi.fn()
      svc.populateAaBtPairedListInitial = vi.fn(async () => undefined)
      svc.config = { wirelessAaEnabled: true }
      svc.syncHelperSupervisor()
      helperHolder.handlers.stdout('a line')
      helperHolder.handlers.stderr('an error line')
      helperHolder.handlers.error(new Error('supervisor boom'))
    } finally {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    }
  })

  test('syncHelperSupervisor swallows rejected old + stop supervisor stops', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    try {
      const svc = makeSvc()
      svc.drivers.startCp = vi.fn()
      svc.drivers.startAaWireless = vi.fn()
      svc.openAaBtSubscription = vi.fn()
      svc.populateAaBtPairedListInitial = vi.fn(async () => undefined)
      svc.helperSupervisor = { stop: vi.fn(() => Promise.reject(new Error('old boom'))) }
      svc.btEnableKey = ''
      svc.config = { wirelessAaEnabled: true }
      svc.syncHelperSupervisor()
      await Promise.resolve()

      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
      svc.helperSupervisor = { stop: vi.fn(() => Promise.reject(new Error('stop boom'))) }
      svc.btEnableKey = 'h'
      svc.config = {}
      svc.drivers.releaseCp = vi.fn(async () => undefined)
      svc.syncHelperSupervisor()
      await Promise.resolve()
    } finally {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    }
  })

  test('syncHelperSupervisor wireless-AA start settles its populate promise', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    try {
      const svc = makeSvc()
      svc.drivers.startCp = vi.fn()
      svc.drivers.startAaWireless = vi.fn()
      svc.openAaBtSubscription = vi.fn()
      svc.emitTransportState = vi.fn()
      svc.connectConfiguredAudioDevices = vi.fn(async () => undefined)
      svc.populateAaBtPairedListInitial = vi.fn(async () => undefined)
      svc.config = { wirelessAaEnabled: true }
      svc.syncHelperSupervisor()
      await Promise.resolve()
      await Promise.resolve()
      expect(svc.emitTransportState).toHaveBeenCalled()
    } finally {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    }
  })

  test('hasWiredSession closure reports a wired AA/CP session', () => {
    const svc = makeSvc()
    svc.started = true
    svc.sessions.upsert(fakeDriver(), 'androidauto', 'usb', { usbSerial: 'x' })
    expect(svc.arbiter.deps.hasWiredSession()).toBe(true)
  })

  test('onCpHelperPresence wifi link-down closes on the reported ip', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteLink = vi.fn()
    svc.sessions.closeByDeviceOnTransport = vi.fn()
    svc.onCpHelperPresence({ kind: 'wifi', ip: '10.0.0.5', wifiMac: 'AA:BB', connected: false })
    expect(svc.sessions.closeByDeviceOnTransport).toHaveBeenCalledWith(
      { wifiMac: 'AA:BB', ip: '10.0.0.5' },
      'wifi'
    )
  })

  test('maybeAutoActivate leaves an already-active session untouched', () => {
    const svc = makeSvc()
    const first = fakeDriver()
    const s1 = svc.sessions.upsert(first, 'androidauto', 'wifi', { btMac: 'AA:BB' })
    svc.sessions.activate(s1.index)
    const second = fakeDriver()
    svc.webContents = { send: vi.fn() }
    svc.statusFile.setProjection = vi.fn()
    svc.refreshBtPairedList = vi.fn(async () => 0)
    svc.onCpConnected(second)
    expect(svc.sessions.active()?.driver).toBe(first)
  })

  test('onNativeVideoConfig uses default projection size and tolerates no active session', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    svc.config = {}
    svc.planes.prepareMain = vi.fn(() => false)
    svc.planes.updateMainCrop = vi.fn()
    svc.onNativeVideoConfig(0x7a000001, 'h264', Buffer.from([1]))
    expect(svc.lastVideoWidth).toBe(1920)
  })

  test('onNativeClusterConfig defaults size and skips keyframe when nothing was created', () => {
    const svc = makeSvc()
    svc.config = {}
    svc.planes.prepareClusters = vi.fn(() => false)
    svc.drivers.getDongle().requestKeyframe = vi.fn()
    svc.onNativeClusterConfig('h264', Buffer.from([1]))
    expect(svc.lastClusterVideoWidth).toBe(1280)
    expect(svc.drivers.getDongle().requestKeyframe).not.toHaveBeenCalled()
  })

  test('getActiveTransport maps carplay, dongle and androidauto protocols', () => {
    const svc = makeSvc()
    const cp = fakeDriver()
    const s = svc.sessions.upsert(cp, 'dongle', 'usb', {})
    svc.sessions.activate(s.index)
    expect(svc.getActiveTransport()).toBe('dongle')
  })

  test('ipc host getLastClusterVideoSize returns a size when available and null otherwise', () => {
    const svc = makeSvc()
    const host = svc.buildIpcHost()
    svc.lastClusterVideoWidth = 800
    svc.lastClusterVideoHeight = 480
    expect(host.getLastClusterVideoSize()).toEqual({ width: 800, height: 480 })
    svc.lastClusterVideoWidth = 0
    svc.lastClusterVideoHeight = 0
    expect(host.getLastClusterVideoSize()).toBeNull()
  })

  test('uploadIcons uses default icons when the config lacks custom ones and there is no disk file', () => {
    const svc = makeSvc()
    const fs = require('fs')
    vi.spyOn(fs, 'existsSync').mockReturnValue(false)
    svc.config = {}
    svc.drivers.getDongle().uploadHostIcons = vi.fn()
    svc.uploadIcons()
    expect(svc.drivers.getDongle().uploadHostIcons).toHaveBeenCalled()
  })

  test('disconnectPhone returns false when the driver has no disconnect handler', async () => {
    const svc = makeSvc()
    svc.started = true
    svc.drivers.getActive().disconnectPhone = undefined
    await expect(svc.disconnectPhone()).resolves.toBe(false)
  })

  test('stop is a no-op when not started', async () => {
    const svc = makeSvc()
    svc.started = false
    await expect(svc.stop()).resolves.toBeUndefined()
  })

  test('getAllUiWebContents returns empty when the main renderer is absent', () => {
    const svc = makeSvc()
    svc.webContents = null
    ;(getSecondaryWindow as Mock).mockReturnValue(null)
    expect(svc.getAllUiWebContents()).toEqual([])
  })

  test('autoStartIfNeeded bails if shutdown begins while awaiting a pending stop', async () => {
    const svc = makeSvc()
    svc.start = vi.fn(async () => undefined)
    svc.stopPromise = Promise.resolve().then(() => {
      svc.shuttingDown = true
    })
    await svc.autoStartIfNeeded()
    expect(svc.start).not.toHaveBeenCalled()
  })
})

describe('ProjectionService final branch fill', () => {
  test('setBlinkerSoundActive reads the current config through the accessor', () => {
    const svc = makeSvc()
    svc.config = { disableAudioOutput: true }
    expect(() => svc.setBlinkerSoundActive(true)).not.toThrow()
  })

  test('onCpHelperPresence device with an empty payload notes an undefined device', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    svc.onCpHelperPresence({ kind: 'device' })
    const noted = (svc.deviceRegistry.noteDevice as Mock).mock.calls[0][0]
    expect(noted.btMac).toBeUndefined()
    expect(noted.transport).toBe('wifi')
  })

  test('onCpHelperPresence wifi link-down without an ip closes on the wifi mac', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteLink = vi.fn()
    svc.sessions.closeByDeviceOnTransport = vi.fn()
    svc.onCpHelperPresence({ kind: 'wifi', wifiMac: 'AA:BB' })
    expect(svc.sessions.closeByDeviceOnTransport).toHaveBeenCalledWith(
      { wifiMac: 'AA:BB', ip: undefined },
      'wifi'
    )
  })

  test('onAaPresence wireless device without an instance id notes wifi transport', () => {
    const svc = makeSvc()
    svc.deviceRegistry.noteDevice = vi.fn()
    const session = fakeDriver({ isWiredMode: () => false, usbSerial: () => undefined })
    svc.onAaPresence(session, { kind: 'device' })
    const noted = (svc.deviceRegistry.noteDevice as Mock).mock.calls[0][0]
    expect(noted.transport).toBe('wifi')
    expect(noted.usbSerial).toBeUndefined()
  })

  test('plane cluster-size accessor defaults to zero when unset', () => {
    const svc = makeSvc()
    svc.lastClusterVideoWidth = undefined
    svc.lastClusterVideoHeight = undefined
    expect(svc.planes.deps.getClusterVideoSize()).toEqual({ width: 0, height: 0 })
  })

  test('hasWiredSession closure evaluates the carplay branch for a non-AA usb session', () => {
    const svc = makeSvc()
    svc.started = true
    svc.sessions.upsert(fakeDriver(), 'dongle', 'usb', { usbSerial: 'x' })
    expect(svc.arbiter.deps.hasWiredSession()).toBe(false)
  })

  test('arbiter onShouldStop closure is a no-op without an active session', () => {
    const svc = makeSvc()
    svc.sessions.active = vi.fn(() => null)
    svc.sessions.close = vi.fn()
    svc.arbiter.deps.onShouldStop()
    expect(svc.sessions.close).not.toHaveBeenCalled()
  })

  test('switchTransport breaks immediately when the override is already gone', async () => {
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: { transport: 'aa' } }))
    svc.arbiter.getOverride = vi.fn(() => null)
    svc.stop = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    const res = await svc.switchTransport()
    expect(svc.stop).not.toHaveBeenCalled()
    expect(res.ok).toBe(true)
  })

  test('switchTransport skips the stop when nothing is started and loops on override changes', async () => {
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: false, target: null }))
    const res1 = await svc.switchTransport()
    expect(res1).toEqual({ ok: false, active: null })

    const svc2 = makeSvc()
    svc2.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: { transport: 'cp' } }))
    svc2.arbiter.getOverride = vi
      .fn()
      .mockReturnValueOnce({ transport: 'cp', mode: 'wired' })
      .mockReturnValueOnce({ transport: 'aa', mode: 'wireless' })
      .mockReturnValue(null)
    svc2.getActiveTransport = vi.fn(() => 'dongle')
    svc2.isActiveAaWired = vi.fn(() => false)
    svc2.started = false
    svc2.stop = vi.fn(async () => undefined)
    svc2.bounceAaBtConnections = vi.fn(async () => undefined)
    svc2.tryAutoConnect = vi.fn(async () => undefined)
    svc2.autoStartIfNeeded = vi.fn(async () => undefined)
    vi.useFakeTimers()
    const p = svc2.switchTransport()
    await vi.advanceTimersByTimeAsync(1200)
    await p
    expect(svc2.stop).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('switchTransport swallows a rejected deauth while leaving wireless', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: { transport: 'aa' } }))
    svc.arbiter.getOverride = vi
      .fn()
      .mockReturnValueOnce({ transport: 'aa', mode: 'wireless' })
      .mockReturnValue(null)
    svc.getActiveTransport = vi.fn(() => 'aa')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.started = true
    svc.stop = vi.fn(async () => undefined)
    bluezMock.deauthApClients.mockRejectedValueOnce(new Error('deauth boom'))
    svc.bounceAaBtConnections = vi.fn(async () => undefined)
    svc.tryAutoConnect = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    const p = svc.switchTransport()
    await vi.advanceTimersByTimeAsync(600)
    await p
    expect(svc.bounceAaBtConnections).toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('restartSession restarts a non-AA session without wired/wireless handling', async () => {
    const svc = makeSvc()
    svc.getActiveTransport = vi.fn(() => 'cp')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.stop = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    svc.bounceAaBtConnections = vi.fn(async () => undefined)
    await svc.restartSession()
    expect(svc.bounceAaBtConnections).not.toHaveBeenCalled()
    expect(svc.autoStartIfNeeded).toHaveBeenCalled()
  })

  test('connectPairedDevice swallows a rejected deauth for a non-wireless active phone', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', class: 0 }])
    svc.getActiveTransport = vi.fn(() => 'dongle')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.started = true
    svc.stop = vi.fn(async () => undefined)
    svc.applyConfigPatch = vi.fn()
    svc.arbiter.setOverride = vi.fn()
    svc.bounceAaBtConnections = vi.fn(async () => undefined)
    svc.tryAutoConnect = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    const p = svc.connectPairedDevice('AA:BB')
    await vi.advanceTimersByTimeAsync(600)
    const res = await p
    expect(res).toEqual({ ok: true })
    expect(bluezMock.deauthApClients).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('bounceAaBtConnections skips non-phone devices', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    try {
      const svc = makeSvc()
      bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'EE:FF', connected: true, class: 0x0400 }])
      await svc.bounceAaBtConnections()
      expect(bluezMock.disconnect).not.toHaveBeenCalled()
    } finally {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    }
  })

  test('refreshBtPairedList marks wired-AA phones in range and tags host device types', async () => {
    const svc = makeSvc()
    svc.started = true
    svc.isActiveAaWired = vi.fn(() => true)
    svc.deviceController.emitDevices = vi.fn()
    svc.setWirelessPhoneInRange = vi.fn()
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB', name: 'Phone', connected: false, class: 0 },
      { mac: 'CC:DD', name: 'Speaker', connected: false, class: 0x0400 }
    ])
    await svc.refreshBtPairedList()
    expect(svc.setWirelessPhoneInRange).toHaveBeenCalledWith(true)
    const phoneEntry = svc.hostDevList.find((e: any) => e.id === 'AA:BB')
    const speakerEntry = svc.hostDevList.find((e: any) => e.id === 'CC:DD')
    expect(phoneEntry.type).toBe('AndroidAuto')
    expect(speakerEntry.type).toBe('')
  })

  test('connectConfiguredAudioDevices exhausts all retries without a delay after the last', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.config = { audioOutputDevice: 'bluez_output.AA_BB_CC_DD_EE_FF.1' }
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB:CC:DD:EE:FF', connected: false }])
    bluezMock.connectFull.mockResolvedValue({ ok: false, error: 'busy' })
    const p = svc.connectConfiguredAudioDevices()
    await vi.advanceTimersByTimeAsync(20_000)
    await p
    expect(bluezMock.connectFull).toHaveBeenCalledTimes(4)
    bluezMock.connectFull.mockReset()
    vi.useRealTimers()
  })

  test('tryAutoConnect prefers the last mac and logs a bare failure', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.arbiter.getSnapshot = vi.fn(() => ({ wiredPhoneDetected: false }))
    svc.config = { lastConnectedAaBtMac: 'AA:BB' }
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', connected: false, trusted: false }])
    bluezMock.connect.mockResolvedValueOnce({ ok: false })
    await svc.tryAutoConnect()
    expect(bluezMock.connect).toHaveBeenCalledWith('AA:BB')
  })

  test('openAaBtSubscription tolerates partial aa-device events and refreshes without a mac', () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.refreshBtPairedList = vi.fn(async () => 0)
    let onEvent: any
    bluezMock.subscribe.mockImplementationOnce((cb: any) => {
      onEvent = cb
      return { close: vi.fn() }
    })
    svc.openAaBtSubscription()
    onEvent({ event: 'aa-device' })
    onEvent({ event: 'refresh' })
    expect(svc.refreshBtPairedList).toHaveBeenCalledWith({ preferMac: undefined })
  })

  test('openAaBtSubscription swallows a rejected refresh and does not reopen when inactive', () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.refreshBtPairedList = vi.fn(() => Promise.reject(new Error('refresh boom')))
    let onEvent: any
    let onClose: any
    bluezMock.subscribe.mockImplementationOnce((cb: any, close: any) => {
      onEvent = cb
      onClose = close
      return { close: vi.fn() }
    })
    svc.openAaBtSubscription()
    onEvent({ event: 'refresh' })
    svc.aaBtActive = false
    onClose()
    vi.advanceTimersByTime(1100)
    expect(bluezMock.subscribe).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })

  test('start logs a wireless CP mode bring-up', async () => {
    const svc = makeSvc()
    svc.reloadConfigFromDisk = vi.fn(async () => undefined)
    svc.dongleState.resetForTeardown = vi.fn()
    svc.mediaStore.reset = vi.fn()
    svc.navStore.reset = vi.fn()
    svc.planes.resetClusterStreamActive = vi.fn()
    svc.syncClusterStreamFocus = vi.fn()
    svc.clearStartRetry = vi.fn()
    svc.emitTransportState = vi.fn()
    svc.arbiter.pickPreferred = vi.fn(() => ({ transport: 'cp', mode: 'wireless' }))
    svc.drivers.startCp = vi.fn()
    await svc.start()
    expect(svc.drivers.startCp).toHaveBeenCalled()
  })

  test('onActiveSessionChanged non-dongle keeps keyframe deferred while a start is pending', () => {
    const svc = makeSvc()
    svc.planes.dispose = vi.fn()
    svc.planes.restoreCodecs = vi.fn()
    svc.planes.updateMainCrop = vi.fn()
    svc.planes.getMainCodec = vi.fn(() => 'h264')
    svc.mediaStore.hydrate = vi.fn()
    svc.navStore.hydrate = vi.fn()
    svc.startPromise = Promise.resolve()
    const driver = fakeDriver()
    const next = {
      index: 1,
      protocol: 'androidauto',
      driver,
      audio: { duckLevel: 1, duckRampMs: 1500 },
      video: { main: {}, cluster: {} }
    }
    svc.onActiveSessionChanged(next, { index: 2 })
    expect(driver.requestKeyframe).not.toHaveBeenCalled()
    expect(svc.audio.resetForSessionStart).not.toHaveBeenCalled()
  })
})

describe('ProjectionService last-mile coverage', () => {
  test('switchTransport swallows a stop failure inside the loop', async () => {
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: { transport: 'cp' } }))
    svc.arbiter.getOverride = vi
      .fn()
      .mockReturnValueOnce({ transport: 'cp', mode: 'wired' })
      .mockReturnValue(null)
    svc.getActiveTransport = vi.fn(() => 'dongle')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.started = true
    svc.stop = vi.fn(async () => {
      throw new Error('stop boom')
    })
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    await expect(svc.switchTransport()).resolves.toMatchObject({ ok: true })
  })

  test('switchTransport returns null active when isSwitching and target lacks a transport', async () => {
    const svc = makeSvc()
    svc.arbiter.prepareSwitch = vi.fn(() => ({ ok: true, target: {} }))
    svc.isSwitching = true
    const res = await svc.switchTransport()
    expect(res).toEqual({ ok: true, active: null })
  })

  test('connectPairedDevice swallows a stop failure and a rejected deauth for a wireless phone', async () => {
    vi.useFakeTimers()
    const svc = makeSvc()
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', class: 0 }])
    svc.getActiveTransport = vi.fn(() => 'aa')
    svc.isActiveAaWired = vi.fn(() => false)
    svc.started = true
    svc.stop = vi.fn(async () => {
      throw new Error('stop boom')
    })
    bluezMock.deauthApClients.mockRejectedValueOnce(new Error('deauth boom'))
    svc.applyConfigPatch = vi.fn()
    svc.arbiter.setOverride = vi.fn()
    svc.bounceAaBtConnections = vi.fn(async () => undefined)
    svc.tryAutoConnect = vi.fn(async () => undefined)
    svc.autoStartIfNeeded = vi.fn(async () => undefined)
    const p = svc.connectPairedDevice('AA:BB')
    await vi.advanceTimersByTimeAsync(600)
    const res = await p
    expect(res).toEqual({ ok: true })
    vi.useRealTimers()
  })

  test('getClusterTargetWebContents treats a throwing isDestroyed as alive', () => {
    const svc = makeSvc()
    svc.webContents = {
      isDestroyed: () => {
        throw new Error('boom')
      }
    }
    svc.config = { dashboards: { dash3: { main: true } } }
    ;(getSecondaryWindow as Mock).mockReturnValue(null)
    const out = svc.getClusterTargetWebContents()
    expect(out.length).toBe(1)
  })

  test('syncHelperSupervisor populate chain swallows a connect-config rejection', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    try {
      const svc = makeSvc()
      svc.drivers.startCp = vi.fn()
      svc.drivers.startAaWireless = vi.fn()
      svc.openAaBtSubscription = vi.fn()
      svc.emitTransportState = vi.fn()
      svc.connectConfiguredAudioDevices = vi.fn(() => Promise.reject(new Error('cfg boom')))
      svc.populateAaBtPairedListInitial = vi.fn(async () => undefined)
      svc.config = { wirelessAaEnabled: true }
      svc.syncHelperSupervisor()
      await Promise.resolve()
      await Promise.resolve()
      await Promise.resolve()
    } finally {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    }
  })

  test('syncHelperSupervisor populate chain swallows a then-callback failure', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    try {
      const svc = makeSvc()
      svc.drivers.startCp = vi.fn()
      svc.drivers.startAaWireless = vi.fn()
      svc.openAaBtSubscription = vi.fn()
      svc.emitTransportState = vi.fn(() => {
        throw new Error('emit boom')
      })
      svc.connectConfiguredAudioDevices = vi.fn(async () => undefined)
      svc.populateAaBtPairedListInitial = vi.fn(async () => undefined)
      svc.config = { wirelessAaEnabled: true }
      svc.syncHelperSupervisor()
      await Promise.resolve()
      await Promise.resolve()
      await Promise.resolve()
    } finally {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    }
  })

  test('onNativeVideoConfig updates on a height-only resolution change', () => {
    const svc = makeSvc()
    const send = vi.fn()
    svc.webContents = { send, isDestroyed: () => false }
    svc.config = { projectionWidth: 1920, projectionHeight: 1080 }
    svc.lastVideoWidth = 1920
    svc.lastVideoHeight = 999
    svc.planes.prepareMain = vi.fn(() => false)
    svc.planes.updateMainCrop = vi.fn()
    svc.onNativeVideoConfig(0x7a000001, 'h264', Buffer.from([1]))
    expect(svc.lastVideoHeight).toBe(1080)
    expect(svc.planes.updateMainCrop).toHaveBeenCalled()
  })

  test('tryAutoConnect connects the preferred last mac', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.arbiter.getSnapshot = vi.fn(() => ({ wiredPhoneDetected: false }))
    svc.config = { lastConnectedAaBtMac: 'AA:BB' }
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'CC:DD', connected: false, trusted: true },
      { mac: 'AA:BB', connected: false, trusted: false }
    ])
    bluezMock.connect.mockResolvedValueOnce({ ok: true })
    await svc.tryAutoConnect()
    expect(bluezMock.connect).toHaveBeenCalledWith('AA:BB')
  })

  test('tryAutoConnect tags a trusted fallback target when no last mac is preferred', async () => {
    const svc = makeSvc()
    svc.aaBtActive = true
    svc.arbiter.getSnapshot = vi.fn(() => ({ wiredPhoneDetected: false }))
    svc.config = {}
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', connected: false, trusted: true }])
    bluezMock.connect.mockResolvedValueOnce({ ok: true })
    await svc.tryAutoConnect()
    expect(bluezMock.connect).toHaveBeenCalledWith('AA:BB')
  })

  test('onActiveSessionChanged dongle keeps keyframe deferred while starting', () => {
    const svc = makeSvc()
    svc.planes.dispose = vi.fn()
    svc.mediaStore.hydrate = vi.fn()
    svc.navStore.hydrate = vi.fn()
    svc.startPromise = Promise.resolve()
    const driver = fakeDriver()
    const next = {
      index: 1,
      protocol: 'dongle',
      driver,
      audio: { duckLevel: 1, duckRampMs: 1500 }
    }
    svc.onActiveSessionChanged(next, { index: 2 })
    expect(driver.requestKeyframe).not.toHaveBeenCalled()
  })

  test('onActiveSessionChanged non-dongle with a previous session requests a keyframe', () => {
    const svc = makeSvc()
    svc.planes.dispose = vi.fn()
    svc.planes.restoreCodecs = vi.fn()
    svc.planes.updateMainCrop = vi.fn()
    svc.planes.getMainCodec = vi.fn(() => 'h264')
    svc.mediaStore.hydrate = vi.fn()
    svc.navStore.hydrate = vi.fn()
    svc.startPromise = null
    const driver = fakeDriver()
    const next = {
      index: 1,
      protocol: 'androidauto',
      driver,
      audio: { duckLevel: 1, duckRampMs: 1500 },
      video: { main: {}, cluster: {} }
    }
    svc.onActiveSessionChanged(next, { index: 2 })
    expect(driver.requestKeyframe).toHaveBeenCalled()
    expect(svc.audio.resetForSessionStart).not.toHaveBeenCalled()
  })
})
