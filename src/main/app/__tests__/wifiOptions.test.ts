import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import type { Mock } from 'vitest'
import {
  listBtAdapters,
  listWifiChannels,
  listWifiCountryCodes,
  listWifiInterfaces
} from '../wifiOptions'

vi.mock('node:child_process', () => ({ execFileSync: vi.fn() }))
vi.mock('@main/services/projection/driver/helper/helperSupervisor', () => ({
  resolveHelperBin: () => '/data/driver/livi-helperd'
}))
vi.mock('node:fs', () => {
  const __m = { existsSync: vi.fn(), readdirSync: vi.fn(), readFileSync: vi.fn() }
  return { ...__m, default: __m }
})

const mockedExec = execFileSync as Mock
const mockedExists = existsSync as Mock
const mockedReaddir = readdirSync as Mock

const TWO_RADIOS = [
  'country DE',
  'phy phy0',
  'chan 36 5180 ok 20',
  'chan 149 5745 disabled 20',
  'phy phy1',
  'chan 36 5180 ok 20',
  'chan 149 5745 ok 20',
  ''
].join('\n')

const HELPER_LIST = [
  'country DE',
  'phy phy0',
  'chan 1 2412 ok 20',
  'chan 2 2417 ok 20',
  'chan 14 2484 disabled 20',
  'chan 36 5180 ok 20',
  'chan 40 5200 disabled 20',
  'chan 52 5260 radar 20',
  'chan 149 5745 ok 20',
  'bogus line',
  ''
].join('\n')

// What DE really answers: the 5.8 band is there, but only at short range device power.
const SHORT_RANGE_58 = [
  'country DE',
  'phy phy0',
  'chan 36 5180 ok 20',
  'chan 149 5745 ok 13',
  'chan 165 5825 ok 13',
  ''
].join('\n')

describe('wifiOptions', () => {
  const originalPlatform = process.platform

  beforeEach(() => {
    vi.clearAllMocks()
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  })

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  })

  describe('listWifiInterfaces', () => {
    test('returns sorted net devices with a wireless sysfs dir', () => {
      mockedReaddir.mockReturnValue(['wlan1', 'eth0', 'wlan0'])
      mockedExists.mockImplementation((p: string) => String(p).includes('wlan'))
      expect(listWifiInterfaces()).toEqual(['wlan0', 'wlan1'])
    })

    test('returns [] off linux', () => {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
      expect(listWifiInterfaces()).toEqual([])
      expect(mockedReaddir).not.toHaveBeenCalled()
    })

    test('returns [] when sysfs is unreadable', () => {
      mockedReaddir.mockImplementation(() => {
        throw new Error('ENOENT')
      })
      expect(listWifiInterfaces()).toEqual([])
    })
  })

  describe('listBtAdapters', () => {
    test('returns sorted hciN adapters', () => {
      mockedReaddir.mockReturnValue(['hci1', 'hci0', 'usb1', 'hciX'])
      expect(listBtAdapters()).toEqual(['hci0', 'hci1'])
    })

    test('returns [] off linux', () => {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
      expect(listBtAdapters()).toEqual([])
    })

    test('returns [] when sysfs is unreadable', () => {
      mockedReaddir.mockImplementation(() => {
        throw new Error('ENOENT')
      })
      expect(listBtAdapters()).toEqual([])
    })
  })

  describe('listWifiChannels', () => {
    test('parses allowed 2.4 GHz channels from iw list', () => {
      mockedExec.mockReturnValue(HELPER_LIST)
      expect(listWifiChannels('2.4ghz')).toEqual([1, 2])
    })

    test('parses allowed 5 GHz channels skipping disabled and radar entries', () => {
      mockedExec.mockReturnValue(HELPER_LIST)
      expect(listWifiChannels('5ghz')).toEqual([36, 149])
    })

    test('reads only the radio the chosen interface sits on', async () => {
      const { readFileSync } = await import('node:fs')
      ;(readFileSync as Mock).mockReturnValue('phy0\n')
      mockedExec.mockReturnValue(TWO_RADIOS)
      expect(listWifiChannels('5ghz', 'DE', 'wlan0')).toEqual([36])
      ;(readFileSync as Mock).mockReturnValue('phy1\n')
      expect(listWifiChannels('5ghz', 'DE', 'wlan1')).toEqual([36, 149])
    })

    test('drops a band the domain only allows for short range devices', () => {
      mockedExec.mockReturnValue(SHORT_RANGE_58)
      expect(listWifiChannels('5ghz')).toEqual([36])
    })

    test('keeps a channel when the helper says nothing about its power', () => {
      mockedExec.mockReturnValue('country DE\nphy phy0\nchan 149 5745 ok\n')
      expect(listWifiChannels('5ghz')).toEqual([149])
    })

    test('an unreadable phy link falls back to every radio the helper lists', async () => {
      const { readFileSync } = await import('node:fs')
      ;(readFileSync as Mock).mockImplementation(() => {
        throw new Error('ENOENT')
      })
      mockedExec.mockReturnValue(TWO_RADIOS)
      expect(listWifiChannels('5ghz', 'DE', 'wlan0')).toEqual([36, 149])
    })

    test('a country that is not the one the driver is on gets the safe list', () => {
      mockedExec.mockReturnValue(HELPER_LIST)
      expect(listWifiChannels('5ghz', 'US')).toEqual([36, 40, 44, 48])
      expect(listWifiChannels('5ghz', 'DE')).toEqual([36, 149])
    })

    test('asks the helper, which asks the driver', () => {
      mockedExec.mockReturnValue(HELPER_LIST)
      listWifiChannels('5ghz')
      expect(mockedExec).toHaveBeenCalledWith(
        '/data/driver/livi-helperd',
        ['--wifi-channels'],
        expect.anything()
      )
    })

    test('falls back to standard channels when the helper fails', () => {
      mockedExec.mockImplementation(() => {
        throw new Error('no helper')
      })
      expect(listWifiChannels('2.4ghz')).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11])
      expect(listWifiChannels('5ghz')).toEqual([36, 40, 44, 48])
    })

    test('falls back when the driver lists no usable channels', () => {
      mockedExec.mockReturnValue('country DE\nchan 40 5200 disabled\n')
      expect(listWifiChannels('5ghz')).toEqual([36, 40, 44, 48])
    })

    test('a driver that lists no frequency at all gets the safe list', () => {
      mockedExec.mockReturnValue('country DE\nphy phy0\n')
      expect(listWifiChannels('5ghz')).toEqual([36, 40, 44, 48])
    })

    test('falls back off linux without asking anyone', () => {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
      expect(listWifiChannels('2.4ghz')).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11])
      expect(mockedExec).not.toHaveBeenCalled()
    })
  })

  describe('listWifiCountryCodes', () => {
    test('parses countries from regdbdump excluding the world domain', () => {
      mockedExec.mockReturnValue(
        ['country 00: DFS-UNSET', 'country DE: DFS-ETSI', 'country AT: DFS-ETSI', 'junk'].join('\n')
      )
      expect(listWifiCountryCodes()).toEqual(['AT', 'DE'])
    })

    test('finds regdbdump in sbin, which a desktop session does not carry in its PATH', () => {
      mockedExists.mockImplementation((p: string) => String(p) === '/usr/sbin/regdbdump')
      mockedExec.mockReturnValue('country DE: DFS-ETSI\n')
      listWifiCountryCodes()
      expect(mockedExec).toHaveBeenCalledWith(
        '/usr/sbin/regdbdump',
        ['/lib/firmware/regulatory.db'],
        expect.anything()
      )
    })

    test('calls regdbdump by name when it is in none of the system directories', () => {
      mockedExists.mockReturnValue(false)
      mockedExec.mockReturnValue('country DE: DFS-ETSI\n')
      listWifiCountryCodes()
      expect(mockedExec).toHaveBeenCalledWith(
        'regdbdump',
        ['/lib/firmware/regulatory.db'],
        expect.anything()
      )
    })

    test('falls back to the static list when regdbdump fails', () => {
      mockedExec.mockImplementation(() => {
        throw new Error('regdbdump missing')
      })
      const codes = listWifiCountryCodes()
      expect(codes).toContain('DE')
      expect(codes).toContain('US')
      expect(codes).toEqual([...codes].sort())
    })

    test('falls back when the dump contains no countries', () => {
      mockedExec.mockReturnValue('country 00: DFS-UNSET\n')
      expect(listWifiCountryCodes()).toContain('DE')
    })
  })
})
