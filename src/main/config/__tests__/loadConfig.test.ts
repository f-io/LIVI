import { hostname } from 'node:os'
import { loadConfig } from '@main/config/loadConfig'
import { CAR_NAME_MAX, WIFI_PASSWORD_MAX } from '@shared/types/Config'
import { existsSync, readFileSync, writeFileSync } from 'fs'
import type { Mock } from 'vitest'

const fsMock = vi.hoisted(() => ({
  existsSync: vi.fn(),
  mkdirSync: vi.fn(),
  readFileSync: vi.fn(),
  renameSync: vi.fn(),
  writeFileSync: vi.fn()
}))

vi.mock('fs', () => ({ ...fsMock, default: fsMock }))
vi.mock('node:fs', () => ({ ...fsMock, default: fsMock }))

vi.mock('@main/config/paths', () => ({
  CONFIG_PATH: '/tmp/config.json',
  CONFIG_BACKUP_PATH: '/tmp/backup/config.json'
}))

vi.mock('node:os', () => ({ hostname: vi.fn(() => 'test-host') }))

const sysfsPanelGeometryMock = vi.fn(() => null as unknown)

vi.mock('@main/services/video/panelEdid', () => ({
  sysfsPanelGeometry: () => sysfsPanelGeometryMock()
}))

vi.mock('@shared/types', () => ({
  DEFAULT_CONFIG: {
    width: 800,
    height: 480,
    kiosk: true,
    carName: 'LIVI',
    bindings: {},
    wifiPassword: 'livi-default-pw',
    startPage: '/',
    carplayIcon120: '',
    carplayIcon180: '',
    carplayIcon256: ''
  }
}))

describe('loadConfig', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  test('restores from the backup mirror when the live config is missing', () => {
    ;(existsSync as Mock).mockImplementation((p: string) => p === '/tmp/backup/config.json')
    ;(readFileSync as Mock).mockReturnValue(JSON.stringify({ carName: 'from-backup' }))

    const result = loadConfig()

    expect(readFileSync).toHaveBeenCalledWith('/tmp/backup/config.json', 'utf8')
    expect(result.carName).toBe('from-backup')
    // The restored config becomes the live one again, and the mirror is refreshed with it.
    expect(writeFileSync).toHaveBeenCalledWith('/tmp/config.json.tmp', expect.any(String))
    expect(writeFileSync).toHaveBeenCalledWith('/tmp/backup/config.json.tmp', expect.any(String))
  })

  test('returns defaults and writes config when file does not exist', () => {
    ;(existsSync as Mock).mockReturnValue(false)

    const result = loadConfig()

    expect(result).toEqual({
      width: 800,
      height: 480,
      kiosk: true,
      carName: 'test-host',
      bindings: {},
      wifiPassword: 'livi-default-pw',
      startPage: '/',
      carplayIcon120: '',
      carplayIcon180: '',
      carplayIcon256: ''
    })
    expect(writeFileSync).toHaveBeenCalledWith(
      '/tmp/config.json.tmp',
      expect.not.stringContaining('carplayIcon')
    )
  })

  test('reads and returns merged config from file', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(
      JSON.stringify({
        width: 1024,
        height: 600,
        kiosk: false,
        carName: 'MyCar',
        bindings: {},
        wifiPassword: 'MyCarPass123',
        startPage: '/'
      })
    )

    const result = loadConfig()

    expect(readFileSync).toHaveBeenCalledWith('/tmp/config.json', 'utf8')
    expect(result).toEqual({
      width: 1024,
      height: 600,
      kiosk: false,
      carName: 'MyCar',
      bindings: {},
      wifiPassword: 'MyCarPass123',
      startPage: '/',
      carplayIcon120: '',
      carplayIcon180: '',
      carplayIcon256: ''
    })
    expect(writeFileSync).not.toHaveBeenCalled()
  })

  test('falls back to defaults and rewrites file when json is invalid', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue('{bad-json')

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    const result = loadConfig()

    expect(result).toEqual({
      width: 800,
      height: 480,
      kiosk: true,
      carName: 'test-host',
      bindings: {},
      wifiPassword: 'livi-default-pw',
      startPage: '/',
      carplayIcon120: '',
      carplayIcon180: '',
      carplayIcon256: ''
    })
    expect(warnSpy).toHaveBeenCalled()
    expect(writeFileSync).toHaveBeenCalledWith(
      '/tmp/config.json.tmp',
      expect.not.stringContaining('carplayIcon')
    )

    warnSpy.mockRestore()
  })

  test('projection and cluster defaults come from the panel EDID when unset', () => {
    ;(existsSync as Mock).mockReturnValue(false)
    sysfsPanelGeometryMock.mockReturnValueOnce({
      widthMm: 400,
      heightMm: 234,
      widthPx: 400,
      heightPx: 234
    })

    const result = loadConfig() as Record<string, unknown>

    expect(result.projectionWidth).toBe(400)
    expect(result.projectionHeight).toBe(234)
    expect(result.clusterWidth).toBe(400)
    expect(result.clusterHeight).toBe(234)
  })

  test('a panel above 720p scales into 1280x720 keeping its aspect', () => {
    ;(existsSync as Mock).mockReturnValue(false)
    sysfsPanelGeometryMock.mockReturnValueOnce({
      widthMm: 940,
      heightMm: 529,
      widthPx: 3840,
      heightPx: 2160
    })

    const result = loadConfig() as Record<string, unknown>

    expect(result.projectionWidth).toBe(1280)
    expect(result.projectionHeight).toBe(720)
    expect(result.clusterWidth).toBe(1280)
    expect(result.clusterHeight).toBe(720)
  })

  test('a 16:10 panel above 720p scales to even dimensions inside the box', () => {
    ;(existsSync as Mock).mockReturnValue(false)
    sysfsPanelGeometryMock.mockReturnValueOnce({
      widthMm: 520,
      heightMm: 325,
      widthPx: 1920,
      heightPx: 1200
    })

    const result = loadConfig() as Record<string, unknown>

    expect(result.projectionWidth).toBe(1152)
    expect(result.projectionHeight).toBe(720)
  })

  test('a configured projection size skips the panel EDID lookup', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(JSON.stringify({ projectionWidth: 1280 }))

    loadConfig()

    expect(sysfsPanelGeometryMock).not.toHaveBeenCalled()
  })

  test('an existing carName is never replaced by the hostname', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(
      JSON.stringify({
        width: 800,
        height: 480,
        kiosk: true,
        carName: 'Wohnmobil',
        bindings: {},
        wifiPassword: 'MyCarPass123',
        startPage: '/'
      })
    )

    const result = loadConfig()

    expect(result.carName).toBe('Wohnmobil')
    expect(writeFileSync).not.toHaveBeenCalled()
  })

  test('an empty carName is kept, only a missing one is derived', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(
      JSON.stringify({ width: 800, height: 480, kiosk: true, carName: '', bindings: {} })
    )

    expect(loadConfig().carName).toBe('')
  })

  test('a localhost hostname falls back to the default car name', () => {
    ;(existsSync as Mock).mockReturnValue(false)
    ;(hostname as Mock).mockReturnValueOnce('LocalHost.localdomain')
    expect(loadConfig().carName).toBe('LIVI')
  })

  test('an empty hostname falls back to the default car name', () => {
    ;(existsSync as Mock).mockReturnValue(false)
    ;(hostname as Mock).mockReturnValueOnce('')
    expect(loadConfig().carName).toBe('LIVI')
  })

  test('a long hostname is truncated to the car name limit', () => {
    ;(existsSync as Mock).mockReturnValue(false)
    ;(hostname as Mock).mockReturnValueOnce('x'.repeat(CAR_NAME_MAX + 10))
    expect(loadConfig().carName).toBe('x'.repeat(CAR_NAME_MAX))
  })

  test('an unknown startPage falls back to the default', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(JSON.stringify({ startPage: '/nonexistent' }))
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(loadConfig().startPage).toBe('/')

    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('is no page'))
    warnSpy.mockRestore()
  })

  test('a known startPage survives', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(JSON.stringify({ startPage: '/media' }))

    expect(loadConfig().startPage).toBe('/media')
  })

  test('a valid wifiPassword survives', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(
      JSON.stringify({
        width: 800,
        height: 480,
        kiosk: true,
        carName: 'Car',
        bindings: {},
        wifiPassword: 'supersecret'
      })
    )
    expect(loadConfig().wifiPassword).toBe('supersecret')
  })

  test('a default logo stays out of the file, a custom one goes in', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(JSON.stringify({ carName: 'Car', bindings: {} }))
    ;(writeFileSync as Mock).mockClear()

    loadConfig()

    const written = (writeFileSync as Mock).mock.calls[0]?.[1] as string
    expect(written).not.toContain('carplayIcon120')

    ;(readFileSync as Mock).mockReturnValue(
      JSON.stringify({ carName: 'Car', bindings: {}, carplayIcon120: 'b64' })
    )
    ;(writeFileSync as Mock).mockClear()

    expect(loadConfig().carplayIcon120).toBe('b64')
    const second = (writeFileSync as Mock).mock.calls[0]?.[1] as string | undefined
    if (second !== undefined) expect(second).toContain('carplayIcon120')
  })

  test('empty logo keys already in the file are cleaned out on the next write', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(
      JSON.stringify({ carName: 'Car', bindings: {}, carplayIcon120: '', carplayIcon180: '  ' })
    )
    ;(writeFileSync as Mock).mockClear()

    loadConfig()

    expect(writeFileSync).toHaveBeenCalledWith(
      '/tmp/config.json.tmp',
      expect.not.stringContaining('carplayIcon')
    )
  })

  test('an overlong wifiPassword falls back to the default', () => {
    ;(existsSync as Mock).mockReturnValue(true)
    ;(readFileSync as Mock).mockReturnValue(
      JSON.stringify({
        width: 800,
        height: 480,
        kiosk: true,
        carName: 'Car',
        bindings: {},
        wifiPassword: 'p'.repeat(WIFI_PASSWORD_MAX + 1)
      })
    )
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    expect(loadConfig().wifiPassword).toBe('livi-default-pw')
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('falling back'))
    warnSpy.mockRestore()
  })
})
