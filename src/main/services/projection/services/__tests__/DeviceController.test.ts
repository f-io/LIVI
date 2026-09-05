import { DeviceController, type DeviceControllerDeps } from '../DeviceController'
import type { DeviceEntry, DeviceRegistry } from '../DeviceRegistry'
import type { ProjectionSession, SessionManager } from '../SessionManager'

const IAP = '00000000-deca-fade-deca-deafdecacafe'
const AAP = '0000111f-0000-1000-8000-00805f9b34fb'
const HSP = '0000111f-0000-1000-8000-00805f9b34fb'

type SessionsApi = {
  all: ReturnType<typeof vi.fn>
  byDevice: ReturnType<typeof vi.fn>
  stateForDevice: ReturnType<typeof vi.fn>
  activate: ReturnType<typeof vi.fn>
}

function mkSession(over: Partial<ProjectionSession> = {}): ProjectionSession {
  return {
    index: 1,
    protocol: 'androidauto',
    transport: 'wifi',
    device: {},
    state: 'held',
    ...over
  } as unknown as ProjectionSession
}

function mkEntry(over: Partial<DeviceEntry> = {}): DeviceEntry {
  return { presence: {}, ...over } as DeviceEntry
}

function mkCtl(over: Partial<Record<keyof DeviceControllerDeps, unknown>> = {}): {
  ctl: DeviceController
  deps: {
    deviceRegistry: {
      forget: ReturnType<typeof vi.fn>
      list: ReturnType<typeof vi.fn>
      deviceId: ReturnType<typeof vi.fn>
    }
    bluez: {
      disconnect: ReturnType<typeof vi.fn>
      remove: ReturnType<typeof vi.fn>
      connect: ReturnType<typeof vi.fn>
    }
    getBtName: ReturnType<typeof vi.fn>
    getConnectedBtMac: ReturnType<typeof vi.fn>
    emit: ReturnType<typeof vi.fn>
    autoConnect: ReturnType<typeof vi.fn>
    pushReconnectTargets: ReturnType<typeof vi.fn>
    pushWiredPhones: ReturnType<typeof vi.fn>
  }
  sessionsApi: SessionsApi
} {
  const sessionsApi: SessionsApi = {
    all: vi.fn(() => []),
    byDevice: vi.fn(() => null),
    stateForDevice: vi.fn(() => null),
    activate: vi.fn(),
    close: vi.fn()
  }
  const deps = {
    deviceRegistry: {
      forget: vi.fn(),
      list: vi.fn(() => []),
      deviceId: vi.fn((e: DeviceEntry) => e.btMac ?? e.usbUdid ?? e.wifiMac ?? e.instanceId ?? '')
    },
    sessions: () => sessionsApi as unknown as SessionManager,
    bluez: {
      disconnect: vi.fn(() => Promise.resolve()),
      remove: vi.fn(() => Promise.resolve()),
      connect: vi.fn(() => Promise.resolve())
    },
    getBtName: vi.fn(() => undefined),
    getConnectedBtMac: vi.fn(() => ''),
    emit: vi.fn(),
    autoConnect: vi.fn(() => true),
    pushReconnectTargets: vi.fn(),
    pushWiredPhones: vi.fn(),
    ...over
  }
  const ctl = new DeviceController(deps as unknown as DeviceControllerDeps)
  return { ctl, deps: deps as never, sessionsApi }
}

async function flush(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

describe('DeviceController', () => {
  let logSpy: ReturnType<typeof vi.spyOn>
  let warnSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    logSpy = vi.spyOn(console, 'log').mockImplementation(() => undefined)
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  })

  afterEach(() => {
    logSpy.mockRestore()
    warnSpy.mockRestore()
  })

  describe('forgetDevice', () => {
    test('fails when the registry knows nothing', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.forget.mockReturnValue(undefined)

      expect(ctl.forgetDevice('x')).toEqual({ ok: false })
    })

    test('succeeds without touching bluez when the entry has no bt mac', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.forget.mockReturnValue(mkEntry({ usbUdid: 'u1' }))

      expect(ctl.forgetDevice('u1')).toEqual({ ok: true })
      expect(deps.bluez.disconnect).not.toHaveBeenCalled()
    })

    test('unpairs even when the disconnect fails', async () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.forget.mockReturnValue(mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff' }))
      deps.bluez.disconnect.mockRejectedValue(new Error('gone'))

      expect(ctl.forgetDevice('aa:bb:cc:dd:ee:ff')).toEqual({ ok: true })
      await flush()

      expect(deps.bluez.remove).toHaveBeenCalledWith('aa:bb:cc:dd:ee:ff')
      expect(warnSpy).not.toHaveBeenCalled()
    })

    test('sends the goodbye to a running session of the forgotten device', () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      deps.deviceRegistry.forget.mockReturnValue(mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff' }))
      const disconnectPhone = vi.fn(() => Promise.resolve())
      ;(sessionsApi.byDevice as ReturnType<typeof vi.fn>).mockReturnValue({
        index: 3,
        driver: { disconnectPhone }
      })

      expect(ctl.forgetDevice('aa:bb:cc:dd:ee:ff')).toEqual({ ok: true })
      expect(disconnectPhone).toHaveBeenCalled()
    })

    test('closes the session once the goodbye had its window', () => {
      vi.useFakeTimers()
      try {
        const { ctl, deps, sessionsApi } = mkCtl()
        deps.deviceRegistry.forget.mockReturnValue(mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff' }))
        ;(sessionsApi.byDevice as ReturnType<typeof vi.fn>).mockReturnValue({
          index: 3,
          driver: { disconnectPhone: vi.fn(() => Promise.resolve()) }
        })

        ctl.forgetDevice('aa:bb:cc:dd:ee:ff')
        expect(sessionsApi.close).not.toHaveBeenCalled()

        vi.advanceTimersByTime(1500)
        expect(sessionsApi.close).toHaveBeenCalledWith(3)
      } finally {
        vi.useRealTimers()
      }
    })

    test('falls back to SendDisconnectPhone and logs a failing goodbye', async () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      deps.deviceRegistry.forget.mockReturnValue(mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff' }))
      const send = vi.fn(() => Promise.reject(new Error('gone')))
      ;(sessionsApi.byDevice as ReturnType<typeof vi.fn>).mockReturnValue({
        index: 4,
        driver: { send }
      })

      expect(ctl.forgetDevice('aa:bb:cc:dd:ee:ff')).toEqual({ ok: true })
      await flush()

      expect(send).toHaveBeenCalled()
      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('goodbye failed: gone'))
    })

    test('logs when the unpair itself fails', async () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.forget.mockReturnValue(mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff' }))
      deps.bluez.remove.mockRejectedValue(new Error('busy'))

      ctl.forgetDevice('aa:bb:cc:dd:ee:ff')
      await flush()

      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('unpair failed: busy'))
    })
  })

  describe('selectDevice', () => {
    test('activates the session of a known registry device', () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff', currentIp: '10.0.0.2', name: 'P' })
      ])
      sessionsApi.byDevice.mockReturnValue(mkSession({ index: 3 }))

      expect(ctl.selectDevice('aa:bb:cc:dd:ee:ff')).toEqual({ ok: true })
      expect(sessionsApi.byDevice).toHaveBeenCalledWith({
        btMac: 'aa:bb:cc:dd:ee:ff',
        wifiMac: undefined,
        usbUdid: undefined,
        instanceId: undefined,
        ip: '10.0.0.2'
      })
      expect(sessionsApi.activate).toHaveBeenCalledWith(3)
    })

    test('wakes a known carplay phone on the iAP profile', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff', protocol: 'carplay', name: 'P' })
      ])

      expect(ctl.selectDevice('aa:bb:cc:dd:ee:ff')).toEqual({ ok: true })
      expect(deps.bluez.connect).toHaveBeenCalledWith('aa:bb:cc:dd:ee:ff', undefined, IAP)
    })

    test('wakes a known android phone on the HFP AG profile', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:ff', protocol: 'androidauto', name: 'P' })
      ])

      expect(ctl.selectDevice('aa:bb:cc:dd:ee:ff')).toEqual({ ok: true })
      expect(deps.bluez.connect).toHaveBeenCalledWith('aa:bb:cc:dd:ee:ff', undefined, HSP)
    })

    test('wakes an unknown mac-like id without a profile and survives connect errors', async () => {
      const { ctl, deps } = mkCtl()
      deps.bluez.connect.mockRejectedValue(new Error('unreachable'))

      expect(ctl.selectDevice('aa:bb:cc:dd:ee:ff')).toEqual({ ok: true })
      expect(deps.bluez.connect).toHaveBeenCalledWith('aa:bb:cc:dd:ee:ff', undefined, undefined)
      await flush()
      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('wake'))
    })

    test('gives up on an unknown id that is no mac', () => {
      const { ctl, deps } = mkCtl()

      expect(ctl.selectDevice('not-a-mac')).toEqual({ ok: false })
      expect(deps.bluez.connect).not.toHaveBeenCalled()
    })

    test('gives up on a known entry without a bt mac and no session', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([mkEntry({ usbUdid: 'udid-1', name: 'P' })])

      expect(ctl.selectDevice('udid-1')).toEqual({ ok: false })
    })
  })

  describe('emitDevices and reconciliation', () => {
    test('emits device views only when they changed', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', name: 'P' })])

      ctl.emitDevices()
      ctl.emitDevices()
      expect(deps.emit).toHaveBeenCalledTimes(1)

      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', name: 'P2' })
      ])
      ctl.emitDevices()
      expect(deps.emit).toHaveBeenCalledTimes(2)
    })

    test('pushes reconnect targets for idle known phones, honoring the wake profile', () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      const withSession = mkEntry({ btMac: 'aa:bb:cc:dd:ee:05', protocol: 'carplay', name: 'S' })
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', protocol: 'carplay', name: 'CP' }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:02', name: 'NameOnly' }),
        mkEntry({ wifiMac: '11:22:33:44:55:66', name: 'NoBt' }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:04' }),
        withSession
      ])
      sessionsApi.byDevice.mockImplementation((ids: { btMac?: string }) =>
        ids.btMac === 'aa:bb:cc:dd:ee:05' ? mkSession() : null
      )

      ctl.emitDevices()

      expect(deps.pushReconnectTargets).toHaveBeenCalledTimes(1)
      expect(deps.pushReconnectTargets).toHaveBeenCalledWith([
        ['AA:BB:CC:DD:EE:01', IAP],
        ['AA:BB:CC:DD:EE:02', null]
      ])

      ctl.emitDevices()
      expect(deps.pushReconnectTargets).toHaveBeenCalledTimes(1)

      ctl.resendReconnectTargets()
      expect(deps.pushReconnectTargets).toHaveBeenCalledTimes(2)
      expect(deps.pushWiredPhones).toHaveBeenCalledTimes(1)
    })

    test('pages the most recently seen phone first', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', protocol: 'androidauto', lastSeen: 100 }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:02', protocol: 'carplay', lastSeen: 900 }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:03', protocol: 'carplay' })
      ])

      ctl.resendReconnectTargets()

      expect(deps.pushReconnectTargets).toHaveBeenCalledWith([
        ['AA:BB:CC:DD:EE:02', IAP],
        ['AA:BB:CC:DD:EE:01', AAP],
        ['AA:BB:CC:DD:EE:03', IAP]
      ])
    })

    test('pushes no reconnect targets with autoconnect off', () => {
      const { ctl, deps } = mkCtl({ autoConnect: vi.fn(() => false) })
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', protocol: 'carplay', name: 'CP' })
      ])

      ctl.resendReconnectTargets()

      expect(deps.pushReconnectTargets).toHaveBeenCalledWith([])
    })

    test('reports wired android phones by every known id, skipping wireless and carplay', () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      sessionsApi.all.mockReturnValue([
        mkSession({
          index: 1,
          protocol: 'androidauto',
          transport: 'usb',
          device: { instanceId: 'inst-a', btMac: 'aa:bb:cc:dd:ee:01', usbSerial: 'ser-a' }
        }),
        mkSession({ index: 2, protocol: 'androidauto', transport: 'wifi', device: {} }),
        mkSession({ index: 3, protocol: 'carplay', transport: 'usb', device: {} }),
        mkSession({ index: 4, protocol: 'androidauto', transport: 'usb', device: {} })
      ])

      ctl.emitDevices()

      expect(deps.pushWiredPhones).toHaveBeenCalledWith(['AA:BB:CC:DD:EE:01', 'INST-A', 'SER-A'])
      ctl.emitDevices()
      expect(deps.pushWiredPhones).toHaveBeenCalledTimes(1)
    })
  })

  describe('getDevices', () => {
    test('skips entries without an id or without protocol and name', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry(),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01' }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:02', protocol: 'carplay' })
      ])

      const views = ctl.getDevices()

      expect(views).toHaveLength(1)
      expect(views[0].id).toBe('aa:bb:cc:dd:ee:02')
    })

    test('derives status from session state and wifi presence', () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', name: 'Active' }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:02', name: 'Held' }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:03', name: 'Wifi', presence: { wifi: true } }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:04', name: 'Off' })
      ])
      sessionsApi.stateForDevice.mockImplementation((ids: { btMac?: string }) =>
        ids.btMac === 'aa:bb:cc:dd:ee:01'
          ? 'active'
          : ids.btMac === 'aa:bb:cc:dd:ee:02'
            ? 'held'
            : null
      )

      const byName = new Map(ctl.getDevices().map((v) => [v.name, v.status]))

      expect(byName.get('Active')).toBe('active')
      expect(byName.get('Held')).toBe('available')
      expect(byName.get('Wifi')).toBe('available')
      expect(byName.get('Off')).toBe('offline')
    })

    test('prefers the live bt name and falls back to the registry name', () => {
      const { ctl, deps } = mkCtl()
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', name: 'Stored' }),
        mkEntry({ usbUdid: 'udid-1', name: 'UsbOnly' })
      ])
      deps.getBtName.mockImplementation((mac: string) =>
        mac === 'AA:BB:CC:DD:EE:01' ? 'LiveName' : undefined
      )

      const names = ctl.getDevices().map((v) => v.name)

      expect(names).toContain('LiveName')
      expect(names).toContain('UsbOnly')

      deps.getBtName.mockReturnValue(undefined)
      expect(ctl.getDevices().map((v) => v.name)).toContain('Stored')
    })

    test('rebinds the shown bt mac for an android phone whose mac a carplay session owns', () => {
      const cp = mkSession({
        index: 1,
        protocol: 'carplay',
        device: { btMac: 'cc:cc:cc:cc:cc:01' },
        state: 'active'
      })
      const { ctl, deps, sessionsApi } = mkCtl()
      sessionsApi.all.mockReturnValue([
        cp,
        mkSession({ index: 2, protocol: 'carplay', device: {} }),
        mkSession({ index: 3, protocol: 'androidauto', device: { btMac: 'dd:dd:dd:dd:dd:01' } })
      ])
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'cc:cc:cc:cc:cc:01', protocol: 'androidauto', name: 'AA' })
      ])
      deps.getConnectedBtMac.mockReturnValue('ee:ee:ee:ee:ee:01')
      deps.getBtName.mockImplementation((mac: string) =>
        mac === 'EE:EE:EE:EE:EE:01' ? 'RealAndroid' : undefined
      )

      expect(ctl.getDevices()[0].name).toBe('RealAndroid')

      deps.getConnectedBtMac.mockReturnValue('')
      expect(ctl.getDevices()[0].name).toBe('AA')

      deps.getConnectedBtMac.mockReturnValue('cc:cc:cc:cc:cc:01')
      expect(ctl.getDevices()[0].name).toBe('AA')
    })

    test('numbers sessions by their order and hides numbers for foreign sessions', () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      const s1 = mkSession({ index: 5 })
      const s2 = mkSession({ index: 9 })
      sessionsApi.all.mockReturnValue([s2, s1])
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', name: 'One' }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:02', name: 'Two' })
      ])
      sessionsApi.byDevice.mockImplementation((ids: { btMac?: string }) =>
        ids.btMac === 'aa:bb:cc:dd:ee:01' ? s1 : mkSession({ index: 99 })
      )

      const views = ctl.getDevices()

      expect(views.find((v) => v.name === 'One')?.session).toBe(1)
      expect(views.find((v) => v.name === 'Two')?.session).toBeUndefined()
    })

    test('sorts session views first, then by status rank, then by recency', () => {
      const { ctl, deps, sessionsApi } = mkCtl()
      const s1 = mkSession({ index: 1 })
      const s2 = mkSession({ index: 2 })
      sessionsApi.all.mockReturnValue([s1, s2])
      deps.deviceRegistry.list.mockReturnValue([
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:03', name: 'OffOld', lastSeen: 100 }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:01', name: 'SessTwo' }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:04', name: 'OffNew', lastSeen: 200 }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:05', name: 'Avail', presence: { wifi: true } }),
        mkEntry({ btMac: 'aa:bb:cc:dd:ee:02', name: 'SessOne' })
      ])
      sessionsApi.byDevice.mockImplementation((ids: { btMac?: string }) =>
        ids.btMac === 'aa:bb:cc:dd:ee:01' ? s2 : ids.btMac === 'aa:bb:cc:dd:ee:02' ? s1 : null
      )
      sessionsApi.stateForDevice.mockImplementation((ids: { btMac?: string }) =>
        ids.btMac === 'aa:bb:cc:dd:ee:01' || ids.btMac === 'aa:bb:cc:dd:ee:02' ? 'active' : null
      )

      const names = ctl.getDevices().map((v) => v.name)

      expect(names).toEqual(['SessOne', 'SessTwo', 'Avail', 'OffNew', 'OffOld'])
    })
  })
})
