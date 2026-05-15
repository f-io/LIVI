import { EventEmitter } from 'node:events'
import type { Readable } from 'node:stream'

class MockChild extends EventEmitter {
  stdout: EventEmitter & { setEncoding: jest.Mock } = Object.assign(new EventEmitter(), {
    setEncoding: jest.fn()
  })
  stderr: EventEmitter & { setEncoding: jest.Mock } = Object.assign(new EventEmitter(), {
    setEncoding: jest.fn()
  })
  kill = jest.fn()
  killed = false
  exitCode: number | null = null
}

const lastChild: { instance: MockChild | null } = { instance: null }
const spawnMock = jest.fn()
const existsSyncMock = jest.fn()
const readdirSyncMock = jest.fn(() => [] as string[])
const statSyncMock = jest.fn(() => ({
  isDirectory: () => false,
  isFile: () => true,
  size: 0,
  mtimeMs: 0
}))
const readFileSyncMock = jest.fn()
const cpSyncMock = jest.fn()
const mkdirSyncMock = jest.fn()
const writeFileSyncMock = jest.fn()

jest.mock('node:child_process', () => ({
  spawn: (...args: unknown[]) => spawnMock(...args)
}))

jest.mock('node:fs', () => ({
  existsSync: (...a: unknown[]) => existsSyncMock(...a),
  readdirSync: (...a: unknown[]) => readdirSyncMock(...a),
  statSync: (...a: unknown[]) => statSyncMock(...a),
  readFileSync: (...a: unknown[]) => readFileSyncMock(...a),
  cpSync: (...a: unknown[]) => cpSyncMock(...a),
  mkdirSync: (...a: unknown[]) => mkdirSyncMock(...a),
  writeFileSync: (...a: unknown[]) => writeFileSyncMock(...a)
}))

jest.mock('electron', () => ({
  app: { getPath: jest.fn(() => '/tmp/livi-test') }
}))

import type { DongleConfig } from '@shared/types'
import { AaBluetoothSupervisor } from '../aaBluetoothSupervisor'

const baseCfg = (over: Partial<DongleConfig> = {}): DongleConfig =>
  ({
    carName: 'TestHU',
    wifiPassword: 'pw',
    wifiChannel: 36,
    wifiType: '5ghz',
    wifiInterface: 'wlan0',
    btAdapter: 'hci0',
    ...over
  }) as unknown as DongleConfig

beforeEach(() => {
  spawnMock.mockReset()
  existsSyncMock.mockReset()
  readFileSyncMock.mockReset()
  cpSyncMock.mockReset()
  mkdirSyncMock.mockReset()
  writeFileSyncMock.mockReset()
  lastChild.instance = null
  spawnMock.mockImplementation(() => {
    const child = new MockChild()
    lastChild.instance = child
    return child
  })
  // Default: script exists on disk
  existsSyncMock.mockReturnValue(true)
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('AaBluetoothSupervisor.start', () => {
  test('spawns python3 with the script path and config-derived env', () => {
    const sup = new AaBluetoothSupervisor({ python: 'python3' })
    sup.start(baseCfg())
    expect(spawnMock).toHaveBeenCalledTimes(1)
    const [, , opts] = spawnMock.mock.calls[0]
    const env = (opts as { env: Record<string, string> }).env
    expect(env.LIVI_SSID).toBe('TestHU')
    expect(env.LIVI_BTNAME).toBe('TestHU')
    expect(env.LIVI_PASSPHRASE).toBe('pw')
    expect(env.LIVI_WIFI_IFACE).toBe('wlan0')
    expect(env.LIVI_BT_ADAPTER).toBe('hci0')
  })

  test('falls back to LIVI when carName is whitespace', () => {
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg({ carName: '   ' }))
    const env = (spawnMock.mock.calls[0][2] as { env: Record<string, string> }).env
    expect(env.LIVI_SSID).toBe('LIVI')
  })

  test('defaults wifiPassword to "12345678" when empty', () => {
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg({ wifiPassword: '' }))
    const env = (spawnMock.mock.calls[0][2] as { env: Record<string, string> }).env
    expect(env.LIVI_PASSPHRASE).toBe('12345678')
  })

  test('5ghz defaults to channel 36 when wifiChannel is missing', () => {
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg({ wifiChannel: 0, wifiType: '5ghz' }))
    const env = (spawnMock.mock.calls[0][2] as { env: Record<string, string> }).env
    expect(env.LIVI_CHANNEL).toBe('36')
  })

  test('2.4ghz defaults to channel 6 when wifiChannel is missing', () => {
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg({ wifiChannel: 0, wifiType: '2.4ghz' } as Partial<DongleConfig>))
    const env = (spawnMock.mock.calls[0][2] as { env: Record<string, string> }).env
    expect(env.LIVI_CHANNEL).toBe('6')
  })

  test('emits "error" when the script is not on disk', () => {
    existsSyncMock.mockReturnValue(false)
    const sup = new AaBluetoothSupervisor()
    const err = jest.fn()
    sup.on('error', err)
    sup.start(baseCfg())
    expect(err).toHaveBeenCalled()
  })

  test('falls through to process.resourcesPath when dev tree is missing', () => {
    Object.defineProperty(process, 'resourcesPath', {
      value: '/opt/livi/resources',
      configurable: true
    })
    // 1st call (devPath aa-bluetooth.py) → false, 2nd call (resPath) → true
    // 3rd call inside _spawn() (existsSync(script)) → true
    existsSyncMock
      .mockReturnValueOnce(false) // dev probe
      .mockReturnValueOnce(true) // resourcesPath probe
      .mockReturnValueOnce(true) // _spawn existsSync(script)
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    expect(spawnMock).toHaveBeenCalled()
  })

  test('AppImage mount path triggers stageBtDir (signature cache hit)', () => {
    process.env.APPIMAGE = '/tmp/livi.AppImage'
    process.env.APPDIR = '/tmp/.mount_livixyz'
    Object.defineProperty(process, 'resourcesPath', {
      value: '/tmp/.mount_livixyz/resources',
      configurable: true
    })
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

    // dev probe → false, resourcesPath probe → true, stageBtDir signature → match
    existsSyncMock
      .mockReturnValueOnce(false) // dev probe
      .mockReturnValueOnce(true) // resourcesPath probe
      .mockReturnValueOnce(true) // sig file exists
      .mockReturnValueOnce(true) // _spawn existsSync(script)
    readFileSyncMock.mockReturnValueOnce('signature-match')
    // signTree writes an empty hash for an empty tree
    readdirSyncMock.mockReturnValue([])

    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    expect(spawnMock).toHaveBeenCalled()
    delete process.env.APPIMAGE
    delete process.env.APPDIR
  })

  test('AppImage mount path triggers stageBtDir (signature mismatch → cpSync)', () => {
    process.env.APPIMAGE = '/tmp/livi.AppImage'
    process.env.APPDIR = '/tmp/.mount_livixyz'
    Object.defineProperty(process, 'resourcesPath', {
      value: '/tmp/.mount_livixyz/resources',
      configurable: true
    })
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

    existsSyncMock
      .mockReturnValueOnce(false) // dev
      .mockReturnValueOnce(true) // resourcesPath
      .mockReturnValueOnce(false) // sig file missing
      .mockReturnValueOnce(true) // _spawn existsSync(script)
    readdirSyncMock.mockReturnValue([])

    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    expect(cpSyncMock).toHaveBeenCalled()
    expect(writeFileSyncMock).toHaveBeenCalled()
    delete process.env.APPIMAGE
    delete process.env.APPDIR
  })

  test('signTree walks nested directories recursively', () => {
    process.env.APPIMAGE = '/tmp/livi.AppImage'
    process.env.APPDIR = '/tmp/.mount_livixyz'
    Object.defineProperty(process, 'resourcesPath', {
      value: '/tmp/.mount_livixyz/resources',
      configurable: true
    })
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

    // Return one file and one directory at the root; the dir contains one file.
    readdirSyncMock
      .mockReturnValueOnce(['file.txt', 'subdir']) // root
      .mockReturnValueOnce(['nested.py']) // subdir
    statSyncMock
      .mockReturnValueOnce({ isDirectory: () => false, isFile: () => true, size: 10, mtimeMs: 100 })
      .mockReturnValueOnce({ isDirectory: () => true, isFile: () => false, size: 0, mtimeMs: 0 })
      .mockReturnValueOnce({ isDirectory: () => false, isFile: () => true, size: 20, mtimeMs: 200 })

    existsSyncMock
      .mockReturnValueOnce(false) // dev probe
      .mockReturnValueOnce(true) // resourcesPath probe
      .mockReturnValueOnce(false) // sig file missing
      .mockReturnValueOnce(true) // _spawn existsSync(script)

    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    expect(cpSyncMock).toHaveBeenCalled()
    delete process.env.APPIMAGE
    delete process.env.APPDIR
  })

  test('stage failure falls back to using the mount path directly', () => {
    process.env.APPIMAGE = '/tmp/livi.AppImage'
    process.env.APPDIR = '/tmp/.mount_livixyz'
    Object.defineProperty(process, 'resourcesPath', {
      value: '/tmp/.mount_livixyz/resources',
      configurable: true
    })
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

    existsSyncMock
      .mockReturnValueOnce(false) // dev probe
      .mockReturnValueOnce(true) // resourcesPath probe
      .mockReturnValueOnce(false) // sig file missing
      .mockReturnValueOnce(true) // _spawn existsSync(script)
    readdirSyncMock.mockReturnValue([])
    cpSyncMock.mockImplementationOnce(() => {
      throw new Error('EACCES')
    })

    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    expect(spawnMock).toHaveBeenCalled()
    delete process.env.APPIMAGE
    delete process.env.APPDIR
  })
})

describe('AaBluetoothSupervisor — stdout / stderr line buffering', () => {
  test('emits each non-empty line on stdout', () => {
    const sup = new AaBluetoothSupervisor()
    const lines: string[] = []
    sup.on('stdout', (l) => lines.push(l))
    sup.start(baseCfg())
    const child = lastChild.instance!
    child.stdout.emit('data', 'hello\nworld\n')
    expect(lines).toEqual(['hello', 'world'])
  })

  test('handles \\r\\n line endings', () => {
    const sup = new AaBluetoothSupervisor()
    const lines: string[] = []
    sup.on('stdout', (l) => lines.push(l))
    sup.start(baseCfg())
    lastChild.instance!.stdout.emit('data', 'win\r\n')
    expect(lines).toEqual(['win'])
  })

  test('buffers partial lines across chunks', () => {
    const sup = new AaBluetoothSupervisor()
    const lines: string[] = []
    sup.on('stdout', (l) => lines.push(l))
    sup.start(baseCfg())
    const child = lastChild.instance!
    child.stdout.emit('data', 'hel')
    child.stdout.emit('data', 'lo\n')
    expect(lines).toEqual(['hello'])
  })

  test('emits stderr lines on the "stderr" event', () => {
    const sup = new AaBluetoothSupervisor()
    const lines: string[] = []
    sup.on('stderr', (l) => lines.push(l))
    sup.start(baseCfg())
    lastChild.instance!.stderr.emit('data', 'oops\n')
    expect(lines).toEqual(['oops'])
  })
})

describe('AaBluetoothSupervisor — child lifecycle', () => {
  test('forwards child error events', () => {
    const sup = new AaBluetoothSupervisor()
    const err = jest.fn()
    sup.on('error', err)
    sup.start(baseCfg())
    lastChild.instance!.emit('error', new Error('spawn EACCES'))
    expect(err).toHaveBeenCalled()
  })

  test('emits exit when the child exits', () => {
    const sup = new AaBluetoothSupervisor()
    const exit = jest.fn()
    sup.on('exit', exit)
    sup.start(baseCfg())
    lastChild.instance!.emit('exit', 1, null)
    expect(exit).toHaveBeenCalledWith(1, null)
  })

  test('respawns after the configured delay', () => {
    jest.useFakeTimers()
    const sup = new AaBluetoothSupervisor({ restartDelayMs: 100 })
    sup.start(baseCfg())
    lastChild.instance!.emit('exit', 1, null)
    spawnMock.mockClear()
    jest.advanceTimersByTime(100)
    expect(spawnMock).toHaveBeenCalledTimes(1)
    jest.useRealTimers()
  })

  test('stops respawning after maxRestarts is exceeded', () => {
    jest.useFakeTimers()
    const sup = new AaBluetoothSupervisor({ restartDelayMs: 0, maxRestarts: 1 })
    const err = jest.fn()
    sup.on('error', err)
    sup.start(baseCfg())
    // 1st exit → respawn allowed
    lastChild.instance!.emit('exit', 1, null)
    jest.advanceTimersByTime(0)
    // 2nd exit → exceeds budget
    lastChild.instance!.emit('exit', 1, null)
    expect(err).toHaveBeenCalled()
    jest.useRealTimers()
  })

  test('"running" reflects the child state', () => {
    const sup = new AaBluetoothSupervisor()
    expect(sup.running).toBe(false)
    sup.start(baseCfg())
    expect(sup.running).toBe(true)
    lastChild.instance!.exitCode = 0
    expect(sup.running).toBe(false)
  })
})

describe('AaBluetoothSupervisor.stop', () => {
  test('is idempotent when nothing was started', async () => {
    const sup = new AaBluetoothSupervisor()
    await expect(sup.stop()).resolves.toBeUndefined()
  })

  test('sends SIGTERM and resolves when the child exits', async () => {
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    const child = lastChild.instance!
    const p = sup.stop()
    expect(child.kill).toHaveBeenCalledWith('SIGTERM')
    child.emit('exit', 0, 'SIGTERM')
    await p
  })

  test('SIGKILL grace period escalates when child does not exit on SIGTERM', async () => {
    jest.useFakeTimers()
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    const child = lastChild.instance!
    const stopPromise = sup.stop()
    expect(child.kill).toHaveBeenCalledWith('SIGTERM')
    // Don't exit — let the 3s grace period run
    jest.advanceTimersByTime(3_001)
    expect(child.kill).toHaveBeenCalledWith('SIGKILL')
    // Now resolve the stop by emitting exit
    child.emit('exit', 0, 'SIGKILL')
    await stopPromise
    jest.useRealTimers()
  })

  test('stop after child has already exited returns immediately', async () => {
    const sup = new AaBluetoothSupervisor()
    sup.start(baseCfg())
    const child = lastChild.instance!
    child.exitCode = 0
    await expect(sup.stop()).resolves.toBeUndefined()
  })

  test('prevents further respawns', () => {
    jest.useFakeTimers()
    const sup = new AaBluetoothSupervisor({ restartDelayMs: 100 })
    sup.start(baseCfg())
    const stopP = sup.stop()
    lastChild.instance!.emit('exit', 0, 'SIGTERM')
    void stopP
    spawnMock.mockClear()
    jest.advanceTimersByTime(500)
    expect(spawnMock).not.toHaveBeenCalled()
    jest.useRealTimers()
  })
})
