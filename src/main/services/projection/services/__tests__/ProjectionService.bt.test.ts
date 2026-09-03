const bluezMock = {
  listPaired: vi.fn(
    async () => [] as Array<{ mac: string; name?: string; connected?: boolean; trusted?: boolean }>
  ),
  connect: vi.fn(async (_mac: string) => ({ ok: true })),
  connectFull: vi.fn(async (_mac: string) => ({ ok: true })),
  remove: vi.fn(async (_mac: string) => ({ ok: true })),
  subscribe: vi.fn((_onEvent: (e: unknown) => void, _onClose?: () => void) => ({
    close: vi.fn()
  }))
}

vi.mock('../../bt/BluezDeviceClient', () => ({
  BluezDeviceClient: vi.fn().mockImplementation(function () {
    return bluezMock
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
    sendBluetoothPairedList = vi.fn(async () => true)
  }
  class Stub {
    constructor(
      public a?: unknown,
      public b?: unknown
    ) {}
  }
  return {
    DongleDriver: MockDongleDriver,
    Plugged: class {},
    Unplugged: class {},
    PhoneType: { CarPlay: 3, AndroidAuto: 5 },
    BluetoothPairedList: class {},
    AudioData: class {},
    MediaData: class {},
    NavigationData: class {},
    MediaType: { Data: 1 },
    NavigationMetaType: { DashboardInfo: 200 },
    Command: class {},
    BoxInfo: class {},
    SoftwareVersion: class {},
    GnssData: class {},
    SendCommand: Stub,
    SendTouch: Stub,
    SendMultiTouch: Stub,
    SendFile: Stub,
    SendServerCgiScript: Stub,
    SendLiviWeb: Stub,
    SendDisconnectPhone: Stub,
    SendCloseDongle: Stub,
    FileAddress: { ICON_120: '/120', ICON_180: '/180', ICON_256: '/256' },
    BoxUpdateProgress: class {},
    BoxUpdateState: class {},
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

const { configEventsMock } = vi.hoisted(() => ({
  configEventsMock: {
    on: vi.fn(),
    off: vi.fn(),
    emit: vi.fn()
  }
}))
vi.mock('@main/ipc/utils', () => ({
  configEvents: configEventsMock
}))

vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/tmp/appdata') },
  WebContents: class {}
}))

import { ProjectionService } from '../ProjectionService'

function newSvc(): {
  svc: ProjectionService
  setSupervisor: (sup: object | null) => void
} {
  const svc = new ProjectionService()
  const setSupervisor = (sup: object | null): void => {
    ;(svc as unknown as { aaBtActive: boolean }).aaBtActive = sup !== null
  }
  return { svc, setSupervisor }
}

beforeEach(async () => {
  bluezMock.listPaired.mockReset()
  bluezMock.connect.mockReset()
  bluezMock.remove.mockReset()
  bluezMock.subscribe.mockReset()
  configEventsMock.emit.mockReset()
  vi.spyOn(console, 'log').mockImplementation(function () {})
  vi.spyOn(console, 'warn').mockImplementation(function () {})
  vi.spyOn(console, 'error').mockImplementation(function () {})
})
afterEach(async () => vi.restoreAllMocks())

describe('refreshBtPairedList', () => {
  test('still queries BT presence when no AA driver is active', async () => {
    const { svc } = newSvc()
    bluezMock.listPaired.mockResolvedValueOnce([])
    await (svc as unknown as { refreshBtPairedList: () => Promise<void> }).refreshBtPairedList()
    expect(bluezMock.listPaired).toHaveBeenCalled()
  })

  test('listPaired error is swallowed unless throwOnError', async () => {
    const { svc } = newSvc()
    bluezMock.listPaired.mockImplementationOnce(async () => {
      throw new Error('sock down')
    })
    await expect(
      (svc as unknown as { refreshBtPairedList: () => Promise<number> }).refreshBtPairedList()
    ).resolves.toBe(0)
  })

  test('listPaired error rethrows when throwOnError=true', async () => {
    const { svc } = newSvc()
    bluezMock.listPaired.mockImplementationOnce(async () => {
      throw new Error('sock down')
    })
    await expect(
      (
        svc as unknown as {
          refreshBtPairedList: (opts: { throwOnError: boolean }) => Promise<void>
        }
      ).refreshBtPairedList({ throwOnError: true })
    ).rejects.toThrow()
  })

  test('a new connected device is persisted via configEvents', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB', name: 'Phone', connected: true, trusted: true }
    ])
    await (svc as unknown as { refreshBtPairedList: () => Promise<void> }).refreshBtPairedList()
    expect(configEventsMock.emit).toHaveBeenCalledWith(
      'requestSave',
      expect.objectContaining({ lastConnectedAaBtMac: 'AA:BB' })
    )
  })

  test('builds host DevList from paired devices', async () => {
    const { svc } = newSvc()
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB', name: 'P1', connected: false },
      { mac: 'CC:DD', name: 'P2', connected: false }
    ])
    await (svc as unknown as { refreshBtPairedList: () => Promise<void> }).refreshBtPairedList()
    const hostDevList = (svc as unknown as { hostDevList: unknown[] }).hostDevList
    expect(hostDevList).toHaveLength(2)
  })
})

describe('tryAutoConnect', () => {
  test('no-op without active supervisor', async () => {
    const { svc } = newSvc()
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(bluezMock.listPaired).not.toHaveBeenCalled()
  })

  test('bails when something is already connected', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', connected: true }])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(bluezMock.connect).not.toHaveBeenCalled()
  })

  test('logs and bails when paired list is empty', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.listPaired.mockResolvedValueOnce([])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(bluezMock.connect).not.toHaveBeenCalled()
  })

  test('prefers lastConnectedAaBtMac when present', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    ;(svc as unknown as { config: { lastConnectedAaBtMac: string } }).config.lastConnectedAaBtMac =
      'AA:BB'
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'CC:DD', connected: false, trusted: false },
      { mac: 'AA:BB', connected: false, trusted: false }
    ])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(bluezMock.connect).toHaveBeenCalledWith('AA:BB')
  })

  test('falls back to first trusted device', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.listPaired.mockResolvedValueOnce([
      { mac: 'CC:DD', connected: false, trusted: false },
      { mac: 'AA:BB', connected: false, trusted: true }
    ])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(bluezMock.connect).toHaveBeenCalledWith('AA:BB')
  })

  test('connect error is swallowed', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', trusted: true }])
    bluezMock.connect.mockImplementationOnce(async () => {
      throw new Error('busy')
    })
    await expect(
      (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    ).resolves.toBeUndefined()
  })

  test('connect resp.ok=false logs but does not throw', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', trusted: true }])
    bluezMock.connect.mockResolvedValueOnce({ ok: false, error: 'no agent' })
    await expect(
      (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    ).resolves.toBeUndefined()
  })
})

describe('openAaBtSubscription / closeAaBtSubscription', () => {
  test('open is a no-op without an active supervisor', async () => {
    const { svc } = newSvc()
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    expect(bluezMock.subscribe).not.toHaveBeenCalled()
  })

  test('open with an active supervisor creates a subscription', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.subscribe.mockReturnValueOnce({ close: vi.fn() })
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    expect(bluezMock.subscribe).toHaveBeenCalledTimes(1)
  })

  test('open is idempotent', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.subscribe.mockReturnValueOnce({ close: vi.fn() })
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    expect(bluezMock.subscribe).toHaveBeenCalledTimes(1)
  })

  test('close ends the subscription', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    const closeFn = vi.fn()
    bluezMock.subscribe.mockReturnValueOnce({ close: closeFn })
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    ;(svc as unknown as { closeAaBtSubscription: () => void }).closeAaBtSubscription()
    expect(closeFn).toHaveBeenCalled()
  })

  test('close is a no-op when no subscription is open', async () => {
    const { svc } = newSvc()
    expect(() =>
      (svc as unknown as { closeAaBtSubscription: () => void }).closeAaBtSubscription()
    ).not.toThrow()
  })

  test('close swallows a throw from the underlying handle', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.subscribe.mockReturnValueOnce({
      close: () => {
        throw new Error('already closed')
      }
    })
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    expect(() =>
      (svc as unknown as { closeAaBtSubscription: () => void }).closeAaBtSubscription()
    ).not.toThrow()
  })
})

describe('populateAaBtPairedListInitial', () => {
  test('exits immediately on first non-empty list', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor({})
    bluezMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB' }])
    await (
      svc as unknown as { populateAaBtPairedListInitial: () => Promise<void> }
    ).populateAaBtPairedListInitial()
    expect(bluezMock.listPaired).toHaveBeenCalled()
  })

  test('bails fast when supervisor disappears mid-loop', async () => {
    const { svc, setSupervisor } = newSvc()
    setSupervisor(null)
    await (
      svc as unknown as { populateAaBtPairedListInitial: () => Promise<void> }
    ).populateAaBtPairedListInitial()
    expect(bluezMock.listPaired).not.toHaveBeenCalled()
  })
})
