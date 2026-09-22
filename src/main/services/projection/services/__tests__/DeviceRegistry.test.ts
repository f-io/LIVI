import { promises as fsp } from 'node:fs'
import { DeviceRegistry } from '../DeviceRegistry'

const FILE = '/custom/devices.json'

async function mkLoaded(stored: unknown = []): Promise<{
  reg: DeviceRegistry
  readSpy: ReturnType<typeof vi.spyOn>
  writeSpy: ReturnType<typeof vi.spyOn>
}> {
  const readSpy = vi.spyOn(fsp, 'readFile').mockResolvedValue(JSON.stringify(stored))
  const writeSpy = vi.spyOn(fsp, 'writeFile').mockResolvedValue(undefined)
  const reg = new DeviceRegistry(FILE)
  await reg.load()
  return { reg, readSpy, writeSpy }
}

async function flushPersist(): Promise<void> {
  await vi.advanceTimersByTimeAsync(500)
}

function persisted(writeSpy: ReturnType<typeof vi.spyOn>): unknown[] {
  return JSON.parse(writeSpy.mock.calls[writeSpy.mock.calls.length - 1][1] as string)
}

describe('DeviceRegistry', () => {
  let warnSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    vi.useFakeTimers()
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.spyOn(fsp, 'rename').mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  describe('load', () => {
    test('reads the default userData path when no override is given', async () => {
      const readSpy = vi.spyOn(fsp, 'readFile').mockResolvedValue('[]')
      const reg = new DeviceRegistry()

      await reg.load()

      expect(readSpy).toHaveBeenCalledWith('/tmp/devices.json', 'utf8')
      expect(reg.list()).toEqual([])
    })

    test('normalizes stored macs and keeps odd values lowercased', async () => {
      const { reg } = await mkLoaded([
        { btMac: 'AABBCCDDEEFF', name: 'One' },
        { wifiMac: 'NOT-A-MAC', name: 'Two' },
        { usbUdid: 'udid-3', name: 'Three' }
      ])

      const list = reg.list()
      expect(list[0].btMac).toBe('aa:bb:cc:dd:ee:ff')
      expect(list[1].wifiMac).toBe('not-a-mac')
      expect(list[2].btMac).toBeUndefined()
    })

    test('collapses duplicate stored entries and persists the cleaned file', async () => {
      const { reg, writeSpy } = await mkLoaded([
        { btMac: 'aa:bb:cc:dd:ee:ff', name: 'Primary' },
        { btMac: 'AA:BB:CC:DD:EE:FF', instanceId: 'inst-1', model: 'M' }
      ])

      expect(reg.list()).toHaveLength(1)
      expect(reg.list()[0].instanceId).toBe('inst-1')
      await flushPersist()
      expect(writeSpy).toHaveBeenCalledTimes(1)
      expect(persisted(writeSpy)).toHaveLength(1)
    })

    test('a missing file counts as a clean load', async () => {
      vi.spyOn(fsp, 'readFile').mockRejectedValue(
        Object.assign(new Error('nope'), { code: 'ENOENT' })
      )
      const writeSpy = vi.spyOn(fsp, 'writeFile').mockResolvedValue(undefined)
      const reg = new DeviceRegistry(FILE)

      await reg.load()
      reg.noteDevice({ btMac: 'aa:bb:cc:dd:ee:ff', name: 'N' })
      await flushPersist()

      expect(warnSpy).not.toHaveBeenCalled()
      expect(writeSpy).toHaveBeenCalledTimes(1)
    })

    test('an IO error keeps the file untouched and blocks persisting', async () => {
      vi.spyOn(fsp, 'readFile').mockRejectedValue(
        Object.assign(new Error('denied'), { code: 'EACCES' })
      )
      const writeSpy = vi.spyOn(fsp, 'writeFile').mockResolvedValue(undefined)
      const reg = new DeviceRegistry(FILE)

      await reg.load()
      reg.noteDevice({ btMac: 'aa:bb:cc:dd:ee:ff', name: 'N' })
      await flushPersist()

      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('EACCES'))
      expect(writeSpy).not.toHaveBeenCalled()
    })

    test('a parse error is reported without an errno code', async () => {
      vi.spyOn(fsp, 'readFile').mockResolvedValue('garbage')
      const reg = new DeviceRegistry(FILE)

      await reg.load()

      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('parse error'))
      expect(reg.list()).toEqual([])
    })
  })

  describe('persist', () => {
    test('debounces rapid changes into a single write', async () => {
      const { reg, writeSpy } = await mkLoaded()

      reg.noteDevice({ btMac: 'aa:bb:cc:dd:ee:01', name: 'A' })
      reg.noteDevice({ btMac: 'aa:bb:cc:dd:ee:02', name: 'B' })
      await flushPersist()

      expect(writeSpy).toHaveBeenCalledTimes(1)
      expect(persisted(writeSpy)).toHaveLength(2)
    })

    test('drops entries without a stable key or without protocol and name', async () => {
      const { reg, writeSpy } = await mkLoaded([{ btMac: 'aa:bb:cc:dd:ee:99' }])

      reg.noteDevice({ ip: '10.0.0.9' })
      reg.noteDevice({ usbUdid: 'udid-1', name: 'UsbOnly', protocol: 'androidauto' })
      reg.noteDevice({ usbSerial: 'ser-1', name: 'SerialOnly' })
      reg.noteDevice({ wifiMac: '11:22:33:44:55:66', name: 'WifiOnly' })
      reg.noteDevice({ instanceId: 'inst-1', name: 'InstOnly' })
      await flushPersist()

      const stored = persisted(writeSpy) as Array<Record<string, unknown>>
      expect(stored.map((s) => s.name)).toEqual(['UsbOnly', 'SerialOnly', 'WifiOnly', 'InstOnly'])
    })

    test('write failures are swallowed', async () => {
      const { reg, writeSpy } = await mkLoaded()
      writeSpy.mockRejectedValue(new Error('disk full'))

      reg.noteDevice({ btMac: 'aa:bb:cc:dd:ee:01', name: 'A' })
      await flushPersist()

      expect(writeSpy).toHaveBeenCalledTimes(1)
    })
  })

  describe('noteDevice', () => {
    test('the generic AA device_name "Android" is never stored as a name', async () => {
      const { reg } = await mkLoaded()

      reg.noteDevice({
        usbSerial: 'ser-px',
        instanceId: 'inst-px',
        name: 'Android',
        model: 'Google Pixel 8',
        protocol: 'androidauto',
        transport: 'usb'
      })

      const e = reg.list()[0]
      expect(e.name).toBeUndefined()
      expect(e.model).toBe('Google Pixel 8')
    })

    test('a generic name never clobbers a previously learned personal name', async () => {
      const { reg } = await mkLoaded()

      reg.noteDevice({
        btMac: 'AA:BB:CC:DD:EE:02',
        name: 'LaPixel',
        protocol: 'androidauto',
        transport: 'wifi'
      })
      reg.noteDevice({
        btMac: 'AA:BB:CC:DD:EE:02',
        name: 'android',
        model: 'Google Pixel 8',
        protocol: 'androidauto',
        transport: 'usb'
      })

      expect(reg.list()[0].name).toBe('LaPixel')
    })

    test('a whitespace-only name is treated as absent', async () => {
      const { reg } = await mkLoaded()

      reg.noteDevice({
        usbSerial: 'ser-ws',
        name: '   ',
        model: 'Pixel',
        protocol: 'androidauto',
        transport: 'usb'
      })

      expect(reg.list()[0].name).toBeUndefined()
    })

    test('load scrubs a persisted generic name so the model shows instead', async () => {
      const { reg } = await mkLoaded([
        {
          usbSerial: '39181FDJH00276',
          instanceId: 'dbb43f9e',
          name: 'Android',
          model: 'Google Pixel 8',
          protocol: 'androidauto',
          lastTransport: 'usb'
        }
      ])

      const e = reg.list()[0]
      expect(e.name).toBeUndefined()
      expect(e.model).toBe('Google Pixel 8')
    })

    test('fills every field, marks usb presence and defaults the protocol chain', async () => {
      const { reg } = await mkLoaded()

      reg.noteDevice({
        btMac: 'AA:BB:CC:DD:EE:01',
        wifiMac: '11:22:33:44:55:66',
        ip: '10.0.0.5',
        usbUdid: 'udid-1',
        usbSerial: 'ser-1',
        instanceId: 'inst-1',
        name: 'Phone',
        model: 'Pixel',
        protocol: 'androidauto',
        transport: 'usb'
      })

      const e = reg.list()[0]
      expect(e).toMatchObject({
        btMac: 'aa:bb:cc:dd:ee:01',
        wifiMac: '11:22:33:44:55:66',
        currentIp: '10.0.0.5',
        usbUdid: 'udid-1',
        usbSerial: 'ser-1',
        instanceId: 'inst-1',
        name: 'Phone',
        model: 'Pixel',
        protocol: 'androidauto',
        lastTransport: 'usb',
        presence: { usb: true }
      })
      expect(e.lastSeen).toBeTypeOf('number')

      reg.noteDevice({ btMac: 'aa:bb:cc:dd:ee:01' })
      expect(reg.list()[0].protocol).toBe('androidauto')
      expect(reg.list()[0].presence.wifi).toBe(true)
      expect(reg.list()).toHaveLength(1)
    })

    test('a minimal note creates a carplay entry with wifi presence', async () => {
      const { reg } = await mkLoaded()

      reg.noteDevice({})

      const e = reg.list()[0]
      expect(e.protocol).toBe('carplay')
      expect(e.presence).toEqual({ wifi: true })
      expect(e.btMac).toBeUndefined()
    })

    test('merges two known entries once their ids turn out to be the same phone', async () => {
      const { reg } = await mkLoaded()
      reg.noteDevice({
        btMac: 'aa:bb:cc:dd:ee:01',
        name: 'A',
        model: 'MA',
        protocol: 'carplay',
        transport: 'usb',
        usbUdid: 'udid-a',
        usbSerial: 'ser-a',
        instanceId: 'inst-a',
        wifiMac: '11:22:33:44:55:01',
        ip: '10.0.0.1'
      })
      reg.noteLink({ btMac: 'aa:bb:cc:dd:ee:01' }, 'bt', true)
      reg.noteDevice({ instanceId: 'inst-b', name: 'B', model: 'MB', transport: 'wifi' })

      reg.noteDevice({ btMac: 'aa:bb:cc:dd:ee:01', instanceId: 'inst-b', name: 'A2' })

      expect(reg.list()).toHaveLength(1)
      const e = reg.list()[0]
      expect(e.name).toBe('A2')
      expect(e.instanceId).toBe('inst-b')
      expect(e.model).toBe('MA')
      expect(e.presence).toMatchObject({ bt: true, wifi: true, usb: true })
    })
  })

  describe('presence and status', () => {
    test('noteLink updates presence, ip and lastSeen on link-up and clears on link-down', async () => {
      const { reg } = await mkLoaded([{ wifiMac: '11:22:33:44:55:66', name: 'W' }])

      reg.noteLink({ wifiMac: '11:22:33:44:55:66', ip: '10.0.0.7' }, 'wifi', true)
      expect(reg.list()[0].presence.wifi).toBe(true)
      expect(reg.list()[0].currentIp).toBe('10.0.0.7')
      expect(reg.list()[0].lastSeen).toBeTypeOf('number')

      reg.noteLink({ ip: '10.0.0.7' }, 'wifi', false)
      expect(reg.list()[0].presence.wifi).toBe(false)

      reg.noteLink({ usbSerial: 'ser-x' }, 'usb', true)
      expect(reg.list()).toHaveLength(1)
    })

    test('noteLink without an ip keeps the previous one', async () => {
      const { reg } = await mkLoaded([{ usbSerial: 'ser-1', name: 'S' }])

      reg.noteLink({ usbSerial: 'ser-1' }, 'usb', true)

      expect(reg.list()[0].presence.usb).toBe(true)
      expect(reg.list()[0].currentIp).toBeUndefined()
    })

    test('clearPresence wipes presence and ip for a known device and ignores strangers', async () => {
      const { reg } = await mkLoaded([{ instanceId: 'inst-1', name: 'I' }])
      reg.noteLink({ instanceId: 'inst-1', ip: '10.0.0.8' }, 'wifi', true)

      reg.clearPresence({ instanceId: 'inst-1' })
      expect(reg.list()[0].presence).toEqual({})
      expect(reg.list()[0].currentIp).toBeUndefined()

      reg.clearPresence({ instanceId: 'ghost' })
      expect(reg.list()).toHaveLength(1)
    })

    test('noteStatus applies only the provided fields and ignores strangers', async () => {
      const { reg } = await mkLoaded([{ btMac: 'aa:bb:cc:dd:ee:01', name: 'P' }])

      reg.noteStatus({ btMac: 'AA:BB:CC:DD:EE:01' }, { batteryLevel: 80, signalStrength: 3 })
      reg.noteStatus(
        { btMac: 'aa:bb:cc:dd:ee:01' },
        {
          batteryCritical: false,
          batteryCharging: true,
          batteryTimeRemaining: 90,
          carrierName: 'Carrier'
        }
      )
      reg.noteStatus({ btMac: 'aa:bb:cc:dd:ee:01' }, {})
      reg.noteStatus({ btMac: 'ff:ff:ff:ff:ff:ff' }, { batteryLevel: 1 })

      expect(reg.list()[0]).toMatchObject({
        batteryLevel: 80,
        signalStrength: 3,
        batteryCritical: false,
        batteryCharging: true,
        batteryTimeRemaining: 90,
        carrierName: 'Carrier'
      })
    })

    test('a status lookup matching two entries merges them first', async () => {
      const { reg } = await mkLoaded([
        { usbUdid: 'udid-1', name: 'One' },
        { usbSerial: 'ser-2', wifiMac: '11:22:33:44:55:02', lastSeen: 5 }
      ])

      reg.noteStatus({ usbUdid: 'udid-1', usbSerial: 'ser-2' }, { batteryLevel: 42 })

      expect(reg.list()).toHaveLength(1)
      expect(reg.list()[0]).toMatchObject({
        usbUdid: 'udid-1',
        usbSerial: 'ser-2',
        wifiMac: '11:22:33:44:55:02',
        batteryLevel: 42,
        lastSeen: 5
      })
    })
  })

  describe('identity helpers', () => {
    test('deviceId picks the first stable id', async () => {
      const { reg } = await mkLoaded()

      expect(reg.deviceId({ btMac: 'bt', usbUdid: 'u', presence: {} })).toBe('bt')
      expect(reg.deviceId({ usbUdid: 'u', wifiMac: 'w', presence: {} })).toBe('u')
      expect(reg.deviceId({ wifiMac: 'w', instanceId: 'i', presence: {} })).toBe('w')
      expect(reg.deviceId({ instanceId: 'i', presence: {} })).toBe('i')
      expect(reg.deviceId({ presence: {} })).toBe('')
    })

    test('noteName renames only on a real change', async () => {
      const { reg, writeSpy } = await mkLoaded([{ btMac: 'aa:bb:cc:dd:ee:01', name: 'Old' }])

      reg.noteName('', 'X')
      reg.noteName('aa:bb:cc:dd:ee:01', '')
      reg.noteName('ff:ff:ff:ff:ff:ff', 'X')
      reg.noteName('aa:bb:cc:dd:ee:01', 'Old')
      await flushPersist()
      expect(writeSpy).not.toHaveBeenCalled()

      reg.noteName('AA:BB:CC:DD:EE:01', 'New')
      await flushPersist()
      expect(reg.list()[0].name).toBe('New')
      expect(writeSpy).toHaveBeenCalledTimes(1)
    })

    test('forget removes by btMac, usbUdid or wifiMac and returns the entry', async () => {
      const { reg } = await mkLoaded([
        { btMac: 'aa:bb:cc:dd:ee:01', name: 'Bt' },
        { usbUdid: 'udid-1', name: 'Usb' },
        { wifiMac: '11:22:33:44:55:66', name: 'Wifi' }
      ])

      expect(reg.forget('unknown-id')).toBeUndefined()
      expect(reg.forget('AA:BB:CC:DD:EE:01')?.name).toBe('Bt')
      expect(reg.forget('udid-1')?.name).toBe('Usb')
      expect(reg.forget('11:22:33:44:55:66')?.name).toBe('Wifi')
      expect(reg.list()).toEqual([])
    })

    test('forget also matches the instanceId fallback that deviceId() hands out', async () => {
      const { reg } = await mkLoaded([{ instanceId: 'inst-1', name: 'Wired' }])

      expect(reg.deviceId(reg.list()[0])).toBe('inst-1')
      expect(reg.forget('inst-1')?.name).toBe('Wired')
      expect(reg.list()).toEqual([])
    })
  })

  describe('change notification', () => {
    test('debounces bursts into one callback and stays silent without a listener', async () => {
      const { reg } = await mkLoaded([{ btMac: 'aa:bb:cc:dd:ee:01', name: 'P' }])
      await vi.advanceTimersByTimeAsync(200)

      const cb = vi.fn()
      reg.onChange(cb)
      reg.noteLink({ btMac: 'aa:bb:cc:dd:ee:01' }, 'bt', true)
      reg.noteLink({ btMac: 'aa:bb:cc:dd:ee:01' }, 'bt', false)
      expect(cb).not.toHaveBeenCalled()

      await vi.advanceTimersByTimeAsync(200)
      expect(cb).toHaveBeenCalledTimes(1)
    })
  })
})
