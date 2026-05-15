import EventEmitter from 'events'

// Reuse the same mocking style as the main ProjectionService.test.ts but
// add a stubbed AaBtSockClient so we can exercise the AA-BT helpers
// (refreshAaBtPairedList / populateAaBtPairedListInitial / tryAutoConnect /
// openAaBtSubscription / closeAaBtSubscription) without involving the
// real Python sock.

const aaBtSockMock = {
  listPaired: jest.fn(
    async () => [] as Array<{ mac: string; name?: string; connected?: boolean; trusted?: boolean }>
  ),
  connect: jest.fn(async (_mac: string) => ({ ok: true })),
  remove: jest.fn(async (_mac: string) => ({ ok: true })),
  subscribe: jest.fn((_onEvent: (e: unknown) => void, _onClose?: () => void) => ({
    close: jest.fn()
  }))
}

jest.mock('../../driver/aa/AaBtSockClient', () => ({
  AaBtSockClient: jest.fn().mockImplementation(() => aaBtSockMock)
}))

jest.mock('../../messages', () => {
  class MockDongleDriver extends EventEmitter {
    send = jest.fn(async () => true)
    initialise = jest.fn(async () => undefined)
    start = jest.fn(async () => undefined)
    stop = jest.fn(async () => undefined)
    close = jest.fn(async () => undefined)
    sendBluetoothPairedList = jest.fn(async () => true)
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
    VideoData: class {},
    AudioData: class {},
    MetaData: class {},
    MediaType: { Data: 1 },
    NavigationMetaType: { DashboardInfo: 200 },
    Command: class {},
    BoxInfo: class {},
    SoftwareVersion: class {},
    GnssData: class {},
    SendRawMessage: Stub,
    SendCommand: Stub,
    SendTouch: Stub,
    SendMultiTouch: Stub,
    SendAudio: Stub,
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

const configEventsMock = {
  on: jest.fn(),
  off: jest.fn(),
  emit: jest.fn()
}
jest.mock('@main/ipc/utils', () => ({
  configEvents: configEventsMock
}))

jest.mock('electron', () => ({
  app: { getPath: jest.fn(() => '/tmp/appdata') },
  WebContents: class {}
}))

jest.mock('usb', () => ({
  WebUSBDevice: { createInstance: jest.fn(async () => ({})) },
  usb: { getDeviceList: jest.fn(() => []) }
}))

import { ProjectionService } from '../ProjectionService'

type FakeAa = {
  isWiredMode: jest.Mock
  close: jest.Mock
}

function newSvc(): { svc: ProjectionService; setAa: (aa: FakeAa | null) => void } {
  const svc = new ProjectionService()
  const setAa = (aa: FakeAa | null): void => {
    ;(svc as unknown as { drivers: { aa: FakeAa | null } }).drivers.aa = aa
  }
  return { svc, setAa }
}

function fakeAa(): FakeAa {
  return { isWiredMode: jest.fn(() => false), close: jest.fn() }
}

beforeEach(() => {
  aaBtSockMock.listPaired.mockReset()
  aaBtSockMock.connect.mockReset()
  aaBtSockMock.remove.mockReset()
  aaBtSockMock.subscribe.mockReset()
  configEventsMock.emit.mockReset()
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('refreshAaBtPairedList', () => {
  test('no-op when no AA driver is active', async () => {
    const { svc } = newSvc()
    await (svc as unknown as { refreshAaBtPairedList: () => Promise<void> }).refreshAaBtPairedList()
    expect(aaBtSockMock.listPaired).not.toHaveBeenCalled()
  })

  test('listPaired error is swallowed unless throwOnError', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockImplementationOnce(async () => {
      throw new Error('sock down')
    })
    await expect(
      (svc as unknown as { refreshAaBtPairedList: () => Promise<void> }).refreshAaBtPairedList()
    ).resolves.toBeUndefined()
  })

  test('listPaired error rethrows when throwOnError=true', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockImplementationOnce(async () => {
      throw new Error('sock down')
    })
    await expect(
      (
        svc as unknown as {
          refreshAaBtPairedList: (opts: { throwOnError: boolean }) => Promise<void>
        }
      ).refreshAaBtPairedList({ throwOnError: true })
    ).rejects.toThrow()
  })

  test('a new connected device is persisted via configEvents', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB', name: 'Phone', connected: true, trusted: true }
    ])
    await (svc as unknown as { refreshAaBtPairedList: () => Promise<void> }).refreshAaBtPairedList()
    expect(configEventsMock.emit).toHaveBeenCalledWith(
      'requestSave',
      expect.objectContaining({ lastConnectedAaBtMac: 'AA:BB' })
    )
  })

  test('builds DevList in boxInfo from paired devices', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([
      { mac: 'AA:BB', name: 'P1', connected: false },
      { mac: 'CC:DD', name: 'P2', connected: false }
    ])
    await (svc as unknown as { refreshAaBtPairedList: () => Promise<void> }).refreshAaBtPairedList()
    const box = (svc as unknown as { boxInfo: { DevList?: unknown[] } }).boxInfo
    expect(box.DevList).toHaveLength(2)
  })
})

describe('tryAutoConnect', () => {
  test('no-op when no AA driver', async () => {
    const { svc } = newSvc()
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(aaBtSockMock.listPaired).not.toHaveBeenCalled()
  })

  test('bails when something is already connected', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', connected: true }])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(aaBtSockMock.connect).not.toHaveBeenCalled()
  })

  test('logs and bails when paired list is empty', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(aaBtSockMock.connect).not.toHaveBeenCalled()
  })

  test('prefers lastConnectedAaBtMac when present', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    ;(svc as unknown as { config: { lastConnectedAaBtMac: string } }).config.lastConnectedAaBtMac =
      'AA:BB'
    aaBtSockMock.listPaired.mockResolvedValueOnce([
      { mac: 'CC:DD', connected: false, trusted: false },
      { mac: 'AA:BB', connected: false, trusted: false }
    ])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(aaBtSockMock.connect).toHaveBeenCalledWith('AA:BB')
  })

  test('falls back to first trusted device', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([
      { mac: 'CC:DD', connected: false, trusted: false },
      { mac: 'AA:BB', connected: false, trusted: true }
    ])
    await (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    expect(aaBtSockMock.connect).toHaveBeenCalledWith('AA:BB')
  })

  test('connect error is swallowed', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', trusted: true }])
    aaBtSockMock.connect.mockImplementationOnce(async () => {
      throw new Error('busy')
    })
    await expect(
      (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    ).resolves.toBeUndefined()
  })

  test('connect resp.ok=false logs but does not throw', async () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB', trusted: true }])
    aaBtSockMock.connect.mockResolvedValueOnce({ ok: false, error: 'no agent' })
    await expect(
      (svc as unknown as { tryAutoConnect: () => Promise<void> }).tryAutoConnect()
    ).resolves.toBeUndefined()
  })
})

describe('openAaBtSubscription / closeAaBtSubscription', () => {
  test('open is a no-op without an AA driver', () => {
    const { svc } = newSvc()
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    expect(aaBtSockMock.subscribe).not.toHaveBeenCalled()
  })

  test('open with an AA driver creates a subscription', () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.subscribe.mockReturnValueOnce({ close: jest.fn() })
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    expect(aaBtSockMock.subscribe).toHaveBeenCalledTimes(1)
  })

  test('open is idempotent', () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.subscribe.mockReturnValueOnce({ close: jest.fn() })
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    expect(aaBtSockMock.subscribe).toHaveBeenCalledTimes(1)
  })

  test('close ends the subscription', () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    const closeFn = jest.fn()
    aaBtSockMock.subscribe.mockReturnValueOnce({ close: closeFn })
    ;(svc as unknown as { openAaBtSubscription: () => void }).openAaBtSubscription()
    ;(svc as unknown as { closeAaBtSubscription: () => void }).closeAaBtSubscription()
    expect(closeFn).toHaveBeenCalled()
  })

  test('close is a no-op when no subscription is open', () => {
    const { svc } = newSvc()
    expect(() =>
      (svc as unknown as { closeAaBtSubscription: () => void }).closeAaBtSubscription()
    ).not.toThrow()
  })

  test('close swallows a throw from the underlying handle', () => {
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.subscribe.mockReturnValueOnce({
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
    const { svc, setAa } = newSvc()
    setAa(fakeAa())
    aaBtSockMock.listPaired.mockResolvedValueOnce([{ mac: 'AA:BB' }])
    await (
      svc as unknown as { populateAaBtPairedListInitial: () => Promise<void> }
    ).populateAaBtPairedListInitial()
    expect(aaBtSockMock.listPaired).toHaveBeenCalled()
  })

  test('bails fast when AA driver disappears mid-loop', async () => {
    const { svc, setAa } = newSvc()
    setAa(null)
    await (
      svc as unknown as { populateAaBtPairedListInitial: () => Promise<void> }
    ).populateAaBtPairedListInitial()
    expect(aaBtSockMock.listPaired).not.toHaveBeenCalled()
  })
})
