import { createClockTrust, isClockPlausible } from '../clockTrust'

const NOW = Date.UTC(2026, 5, 21, 12, 0, 0)
const secs = (ms: number) => String(Math.floor(ms / 1000))

const linux = (over: Parameters<typeof createClockTrust>[0] = {}) =>
  createClockTrust({
    platform: 'linux',
    now: () => NOW,
    readFile: () => null,
    fileExists: () => false,
    chronyTracking: () => null,
    ...over
  })

describe('clockTrust', () => {
  beforeEach(() => {
    vi.spyOn(console, 'log').mockImplementation(() => {})
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })

  test('isClockPlausible rejects pre-2026 clocks', () => {
    expect(isClockPlausible(0)).toBe(false)
    expect(isClockPlausible(NOW)).toBe(true)
  })

  test('non-linux hosts are always trusted', () => {
    expect(createClockTrust({ platform: 'darwin' }).check()).toBe('host')
  })

  test('untrusted on linux without RTC or NTP (Pi 4 offline)', () => {
    expect(linux().check()).toBeNull()
  })

  test('implausible system clock is never trusted', () => {
    expect(linux({ now: () => 0, fileExists: () => true }).check()).toBeNull()
  })

  test('RTC with plausible time close to system clock is trusted (Pi 5 + battery)', () => {
    expect(linux({ readFile: () => `${secs(NOW + 30_000)}\n` }).check()).toBe('rtc')
  })

  test('RTC reset to 1970 (Pi 5 without battery) is not trusted', () => {
    expect(linux({ readFile: () => '12\n' }).check()).toBeNull()
  })

  test('RTC far from system clock is not trusted', () => {
    expect(linux({ readFile: () => secs(NOW - 3_600_000) }).check()).toBeNull()
  })

  test('systemd-timesyncd marker means NTP synced', () => {
    expect(linux({ fileExists: (p) => p === '/run/systemd/timesync/synchronized' }).check()).toBe(
      'ntp'
    )
  })

  test('chrony tracking with normal leap status means NTP synced', () => {
    const ok =
      'C0A80001,192.168.0.1,3,1782043200.000000000,0.000001,0.0,0.0,1.0,0.0,0.0,0.01,0.001,64.0,Normal\n'
    expect(linux({ chronyTracking: () => ok }).check()).toBe('ntp')

    const unsync = '00000000,,0,0.000000000,0,0,0,0,0,0,0,0,0,Not synchronised\n'
    expect(linux({ chronyTracking: () => unsync }).check()).toBeNull()
  })

  test('stays trusted once confirmed', () => {
    let synced = true
    const t = linux({ fileExists: () => synced })
    expect(t.check()).toBe('ntp')
    synced = false
    expect(t.check()).toBe('ntp')
  })
})
