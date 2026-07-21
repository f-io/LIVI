import type { IPhoneDriver } from '../../driver/IPhoneDriver'
import { SessionManager } from '../SessionManager'

function mkDriver(): IPhoneDriver {
  return {} as unknown as IPhoneDriver
}

function mkManager(): SessionManager {
  return new SessionManager({ route: () => {} })
}

describe('SessionManager', () => {
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
  })
})
