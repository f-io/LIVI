import type { IPhoneDriver } from '../../driver/IPhoneDriver'
import { SessionManager } from '../SessionManager'

function mkDriver(): IPhoneDriver {
  return {} as unknown as IPhoneDriver
}

function mkManager(): SessionManager {
  return new SessionManager({ route: () => {} })
}

describe('SessionManager', () => {
  it('numbers from 1 again once no session is left', () => {
    const mgr = mkManager()
    const first = mkDriver()
    expect(mgr.upsert(first, 'carplay', 'wifi', { btMac: 'aa:aa' }).index).toBe(1)
    const second = mkDriver()
    expect(mgr.upsert(second, 'carplay', 'wifi', { btMac: 'bb:bb' }).index).toBe(2)
    mgr.closeByDriver(first)
    expect(mgr.upsert(mkDriver(), 'carplay', 'wifi', { btMac: 'cc:cc' }).index).toBe(3)
    mgr.clear()
    expect(mgr.upsert(mkDriver(), 'carplay', 'wifi', { btMac: 'dd:dd' }).index).toBe(1)
  })

  describe('carplay transport derivation', () => {
    it('is wifi with no udid, becomes usb once a udid lands, then stays usb + keeps the udid on a later partial upsert', () => {
      const mgr = mkManager()
      const driver = mkDriver()

      const s0 = mgr.upsert(driver, 'carplay', 'wifi', { btMac: 'AA:BB:CC:DD:EE:FF' })
      expect(s0.transport).toBe('wifi')
      expect(s0.device.usbUdid).toBeUndefined()

      const s1 = mgr.upsert(driver, 'carplay', 'wifi', { usbUdid: '00008120-DEADBEEF' })
      expect(s1).toBe(s0)
      expect(s1.transport).toBe('usb')
      expect(s1.device.usbUdid).toBe('00008120-DEADBEEF')

      const s2 = mgr.upsert(driver, 'carplay', 'wifi', {
        btMac: 'AA:BB:CC:DD:EE:FF',
        wifiMac: '11:22:33:44:55:66',
        usbUdid: undefined
      })
      expect(s2).toBe(s0)
      expect(s2.transport).toBe('usb')
      expect(s2.device.usbUdid).toBe('00008120-DEADBEEF')
      expect(s2.device.wifiMac).toBe('11:22:33:44:55:66')

      const s3 = mgr.upsert(driver, 'carplay', 'wifi', { controllerId: 'ctrl-1' })
      expect(s3).toBe(s0)
      expect(s3.transport).toBe('usb')
      expect(s3.device.usbUdid).toBe('00008120-DEADBEEF')
      expect(s3.device.controllerId).toBe('ctrl-1')

      const s4 = mgr.upsert(driver, 'carplay', 'wifi', {
        btMac: 'AA:BB:CC:DD:EE:FF',
        usbUdid: '',
        ip: '172.20.10.1'
      })
      expect(s4).toBe(s0)
      expect(s4.transport).toBe('usb')
      expect(s4.device.usbUdid).toBe('00008120-DEADBEEF')
      expect(s4.device.ip).toBe('172.20.10.1')
    })
  })

  describe('sticky identity merge', () => {
    it('never erases a known id when a later upsert passes it as undefined', () => {
      const mgr = mkManager()
      const driver = mkDriver()

      const s = mgr.upsert(driver, 'androidauto', 'wifi', {
        btMac: 'AA:BB:CC:DD:EE:FF',
        wifiMac: '11:22:33:44:55:66'
      })
      mgr.upsert(driver, 'androidauto', 'wifi', {
        btMac: undefined,
        wifiMac: undefined,
        instanceId: 'inst-1'
      })

      expect(s.device.btMac).toBe('aa:bb:cc:dd:ee:ff')
      expect(s.device.wifiMac).toBe('11:22:33:44:55:66')
      expect(s.device.instanceId).toBe('inst-1')
    })
  })

  describe('mac case-insensitive identity', () => {
    it('matches one session across mixed btMac casing instead of forking a twin', () => {
      const mgr = mkManager()
      const btDriver = mkDriver()
      const wifiDriver = mkDriver()

      const s1 = mgr.upsert(btDriver, 'carplay', 'bt', { btMac: '0C:6A:C4:4E:F3:2A' })
      const s2 = mgr.upsert(wifiDriver, 'carplay', 'wifi', {
        btMac: '0c:6a:c4:4e:f3:2a',
        wifiMac: 'f2:83:07:13:fb:88'
      })

      expect(s2).toBe(s1)
      expect(mgr.all()).toHaveLength(1)
    })

    it('resolves byDevice when the picker id is upper and the session stored lower', () => {
      const mgr = mkManager()
      const s = mgr.upsert(mkDriver(), 'carplay', 'wifi', { btMac: '0c:6a:c4:4e:f3:2a' })

      expect(mgr.byDevice({ btMac: '0C:6A:C4:4E:F3:2A' })).toBe(s)
    })
  })

  describe('non-carplay transport stays caller-driven', () => {
    it('keeps the passed transport for androidauto and ignores a udid', () => {
      const mgr = mkManager()
      const driver = mkDriver()

      const s = mgr.upsert(driver, 'androidauto', 'usb', { instanceId: 'x' })
      expect(s.transport).toBe('usb')

      const s2 = mgr.upsert(driver, 'androidauto', 'wifi', { usbUdid: 'should-be-ignored' })
      expect(s2).toBe(s)
      expect(s2.transport).toBe('wifi')
    })
  })

  describe('wireless → wired handover', () => {
    it('hands the entry to the wired driver, keeps its index, and retires the wireless driver', () => {
      const mgr = mkManager()
      const closed: string[] = []
      const wireless = { close: () => closed.push('wireless') } as unknown as IPhoneDriver
      const wired = { close: () => closed.push('wired') } as unknown as IPhoneDriver

      const s1 = mgr.upsert(wireless, 'androidauto', 'wifi', {
        instanceId: 'inst-1',
        usbSerial: 'SER123'
      })
      const s2 = mgr.upsert(wired, 'androidauto', 'usb', {
        instanceId: 'inst-1',
        usbSerial: 'SER123'
      })

      expect(s2).toBe(s1)
      expect(s2.index).toBe(s1.index)
      expect(s2.driver).toBe(wired)
      expect(s2.transport).toBe('usb')
      expect(mgr.all()).toHaveLength(1)
      expect(closed).toEqual(['wireless'])
    })

    it('does not let a wireless arrival steal a wired session', () => {
      const mgr = mkManager()
      const wired = { close: () => {} } as unknown as IPhoneDriver
      const wireless = { close: () => {} } as unknown as IPhoneDriver

      const s1 = mgr.upsert(wired, 'androidauto', 'usb', { usbSerial: 'SER123' })
      const s2 = mgr.upsert(wireless, 'androidauto', 'wifi', { usbSerial: 'SER123' })

      expect(s2).not.toBe(s1)
      expect(s1.driver).toBe(wired)
      expect(mgr.all()).toHaveLength(2)
    })

    it('routes to the wired driver when the adopted session is active', () => {
      const route = vi.fn()
      const mgr = new SessionManager({ route })
      const wireless = { close: vi.fn() } as unknown as IPhoneDriver
      const wired = { close: vi.fn() } as unknown as IPhoneDriver

      const s = mgr.upsert(wireless, 'androidauto', 'wifi', { instanceId: 'inst-1' })
      mgr.activate(s.index)
      route.mockClear()

      mgr.upsert(wired, 'androidauto', 'usb', { instanceId: 'inst-1' })

      expect(route).toHaveBeenCalledWith(wired)
    })
  })

  describe('lookups', () => {
    it('finds sessions by every identity key and misses on unknown ids', () => {
      const mgr = mkManager()
      const s = mgr.upsert(mkDriver(), 'androidauto', 'wifi', {
        wifiMac: '11:22:33:44:55:66',
        usbUdid: 'UDID-1',
        usbSerial: 'SER-1',
        instanceId: 'inst-1',
        controllerId: 'ctrl-1',
        ip: '10.0.0.2'
      })

      expect(mgr.byDevice({ wifiMac: '11:22:33:44:55:66' })).toBe(s)
      expect(mgr.byDevice({ usbUdid: 'UDID-1' })).toBe(s)
      expect(mgr.byDevice({ usbSerial: 'SER-1' })).toBe(s)
      expect(mgr.byDevice({ instanceId: 'inst-1' })).toBe(s)
      expect(mgr.byDevice({ controllerId: 'ctrl-1' })).toBe(s)
      expect(mgr.byDevice({ ip: '10.0.0.2' })).toBe(s)
      expect(mgr.byDevice({ btMac: 'no:pe:no:pe:no:pe' })).toBeNull()
      expect(mgr.byDevice({})).toBeNull()
    })

    it('byIdentity requires an id and a matching protocol', () => {
      const mgr = mkManager()
      const s = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'inst-1' })

      expect(mgr.byIdentity('androidauto', {})).toBeNull()
      expect(mgr.byIdentity('androidauto', { ip: '1.2.3.4' })).toBeNull()
      expect(mgr.byIdentity('androidauto', { controllerId: 'ctrl-x' })).toBeNull()
      expect(mgr.byIdentity('androidauto', { usbSerial: 'ser-x' })).toBeNull()
      expect(mgr.byIdentity('carplay', { instanceId: 'inst-1' })).toBeNull()
      expect(mgr.byIdentity('androidauto', { instanceId: 'inst-1' })).toBe(s)
    })

    it('byIndex, stateForDevice, active and held reflect the session states', () => {
      const mgr = mkManager()
      expect(mgr.active()).toBeNull()

      const s1 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      const s2 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'b' })
      mgr.activate(s1.index)

      expect(mgr.byIndex(s1.index)).toBe(s1)
      expect(mgr.byIndex(999)).toBeNull()
      expect(mgr.active()).toBe(s1)
      expect(mgr.held()).toEqual([s2])
      expect(mgr.stateForDevice({ instanceId: 'a' })).toBe('active')
      expect(mgr.stateForDevice({ instanceId: 'b' })).toBe('held')
      expect(mgr.stateForDevice({ instanceId: 'c' })).toBeNull()
    })

    it('dump logs without mutating', () => {
      const logSpy = vi.spyOn(console, 'log').mockImplementation(() => undefined)
      const mgr = mkManager()
      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })

      mgr.dump('why-not')

      expect(logSpy).toHaveBeenCalled()
      expect(mgr.all()).toHaveLength(1)
      logSpy.mockRestore()
    })
  })

  describe('change notification', () => {
    it('emitChange invokes onChange when provided', () => {
      const onChange = vi.fn()
      const mgr = new SessionManager({ route: () => {}, onChange })

      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })

      expect(onChange).toHaveBeenCalledTimes(1)
    })
  })

  describe('reassignDriver', () => {
    it('returns null for an unknown driver and the session for a no-op reassign', () => {
      const mgr = mkManager()
      const d = mkDriver()
      const s = mgr.upsert(d, 'carplay', 'wifi', { btMac: 'aa:bb:cc:dd:ee:01' })

      expect(mgr.reassignDriver(mkDriver(), mkDriver())).toBeNull()
      expect(mgr.reassignDriver(d, d)).toBe(s)
      expect(s.driver).toBe(d)
    })

    it('hands a held session to the new driver without routing', () => {
      const route = vi.fn()
      const mgr = new SessionManager({ route })
      const from = mkDriver()
      const to = mkDriver()
      const s = mgr.upsert(from, 'carplay', 'wifi', { btMac: 'aa:bb:cc:dd:ee:01' })
      route.mockClear()

      expect(mgr.reassignDriver(from, to)).toBe(s)
      expect(s.driver).toBe(to)
      expect(route).not.toHaveBeenCalled()
    })

    it('routes to the new driver when the session is active', () => {
      const route = vi.fn()
      const mgr = new SessionManager({ route })
      const from = mkDriver()
      const to = mkDriver()
      const s = mgr.upsert(from, 'carplay', 'wifi', { btMac: 'aa:bb:cc:dd:ee:01' })
      mgr.activate(s.index)
      route.mockClear()

      mgr.reassignDriver(from, to)

      expect(route).toHaveBeenCalledWith(to)
    })
  })

  describe('activation', () => {
    it('activate returns null for an unknown index and short-circuits when already active', () => {
      const route = vi.fn()
      const onActiveChanged = vi.fn()
      const mgr = new SessionManager({ route, onActiveChanged })
      const s = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })

      expect(mgr.activate(999)).toBeNull()

      expect(mgr.activate(s.index)).toBe(s)
      expect(onActiveChanged).toHaveBeenCalledWith(s, null)

      route.mockClear()
      onActiveChanged.mockClear()
      expect(mgr.activate(s.index)).toBe(s)
      expect(route).not.toHaveBeenCalled()
      expect(onActiveChanged).not.toHaveBeenCalled()
    })

    it('activate holds the previous session and notifies with it', () => {
      const onActiveChanged = vi.fn()
      const mgr = new SessionManager({ route: () => {}, onActiveChanged })
      const s1 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      const s2 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'b' })
      mgr.activate(s1.index)

      mgr.activate(s2.index)

      expect(s1.state).toBe('held')
      expect(s2.state).toBe('active')
      expect(onActiveChanged).toHaveBeenLastCalledWith(s2, s1)
    })

    it('activateNext does nothing with fewer than two sessions', () => {
      const route = vi.fn()
      const mgr = new SessionManager({ route })
      mgr.activateNext()
      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      route.mockClear()

      mgr.activateNext()

      expect(route).not.toHaveBeenCalled()
    })

    it('activateNext cycles through sessions and wraps around', () => {
      const mgr = mkManager()
      const s1 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      const s2 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'b' })
      mgr.activate(s1.index)

      mgr.activateNext()
      expect(mgr.active()).toBe(s2)

      mgr.activateNext()
      expect(mgr.active()).toBe(s1)
    })

    it('activateNext starts at the first session when none is active', () => {
      const mgr = mkManager()
      const s1 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'b' })

      mgr.activateNext()

      expect(mgr.active()).toBe(s1)
    })
  })

  describe('closing', () => {
    it('close promotes the first held session when the active one goes away', () => {
      const route = vi.fn()
      const onActiveChanged = vi.fn()
      const mgr = new SessionManager({ route, onActiveChanged })
      const s1 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      const s2 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'b' })
      mgr.activate(s1.index)
      route.mockClear()
      onActiveChanged.mockClear()

      mgr.close(s1.index)

      expect(mgr.all()).toEqual([s2])
      expect(s2.state).toBe('active')
      expect(route).toHaveBeenCalledWith(s2.driver)
      expect(onActiveChanged).toHaveBeenCalledWith(s2, s1)
    })

    it('close goes idle when the last active session goes away', () => {
      const onActiveChanged = vi.fn()
      const mgr = new SessionManager({ route: () => {}, onActiveChanged })
      const s = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      mgr.activate(s.index)
      onActiveChanged.mockClear()

      mgr.close(s.index)

      expect(mgr.all()).toHaveLength(0)
      expect(onActiveChanged).toHaveBeenCalledWith(null, s)
    })

    it('close removes a held session without touching the active one', () => {
      const onActiveChanged = vi.fn()
      const mgr = new SessionManager({ route: () => {}, onActiveChanged })
      const s1 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      const s2 = mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'b' })
      mgr.activate(s1.index)
      onActiveChanged.mockClear()

      mgr.close(s2.index)

      expect(mgr.all()).toEqual([s1])
      expect(s1.state).toBe('active')
      expect(onActiveChanged).not.toHaveBeenCalled()
    })

    it('close ignores an unknown index', () => {
      const mgr = mkManager()
      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })

      mgr.close(999)

      expect(mgr.all()).toHaveLength(1)
    })

    it('closeByDriver removes the matching session and logs a miss otherwise', () => {
      const logSpy = vi.spyOn(console, 'log').mockImplementation(() => undefined)
      const mgr = mkManager()
      const d = mkDriver()
      mgr.upsert(d, 'androidauto', 'wifi', { instanceId: 'a' })

      mgr.closeByDriver(mkDriver())
      expect(mgr.all()).toHaveLength(1)
      expect(logSpy).toHaveBeenCalledWith('[SESSIONS] closeByDriver → NO matching session')

      mgr.closeByDriver(d)
      expect(mgr.all()).toHaveLength(0)
      logSpy.mockRestore()
    })

    it('closeByDevice removes the matching session and ignores unknown ids', () => {
      const mgr = mkManager()
      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })

      mgr.closeByDevice({ instanceId: 'zzz' })
      expect(mgr.all()).toHaveLength(1)

      mgr.closeByDevice({ instanceId: 'a' })
      expect(mgr.all()).toHaveLength(0)
    })

    it('closeByDeviceOnTransport closes the driver only on a transport match', () => {
      const mgr = mkManager()
      const close = vi.fn()
      const d = { close } as unknown as IPhoneDriver
      mgr.upsert(d, 'androidauto', 'usb', { instanceId: 'a' })

      mgr.closeByDeviceOnTransport({ instanceId: 'zzz' }, 'usb')
      expect(close).not.toHaveBeenCalled()

      mgr.closeByDeviceOnTransport({ instanceId: 'a' }, 'wifi')
      expect(close).not.toHaveBeenCalled()

      mgr.closeByDeviceOnTransport({ instanceId: 'a' }, 'usb')
      expect(close).toHaveBeenCalledTimes(1)
    })

    it('clear drops every session', () => {
      const mgr = mkManager()
      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'a' })
      mgr.upsert(mkDriver(), 'androidauto', 'wifi', { instanceId: 'b' })

      mgr.clear()

      expect(mgr.all()).toHaveLength(0)
    })
  })
})
