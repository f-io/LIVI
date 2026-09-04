import { execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dialog } from 'electron'
import { afterEach, beforeEach, describe, expect, type Mock, test, vi } from 'vitest'

vi.mock('node:child_process', () => ({ spawn: vi.fn(), execFileSync: vi.fn() }))
vi.mock('node:fs', () => ({ existsSync: vi.fn(), readFileSync: vi.fn(), writeFileSync: vi.fn() }))
vi.mock('node:os', () => ({
  default: { userInfo: () => ({ username: 'pi' }) },
  userInfo: () => ({ username: 'pi' })
}))
vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/data') },
  dialog: { showMessageBox: vi.fn(() => Promise.resolve({ response: 0 })) }
}))

import { reconcileWifiAp, releaseWifiApForQuit } from '../wifiApUnit'

const mockedSpawn = spawn as Mock
const mockedExec = execFileSync as Mock
const mockedExists = existsSync as Mock
const mockedRead = readFileSync as Mock
const mockedWrite = writeFileSync as Mock
const mockedDialog = dialog.showMessageBox as Mock
const win = {} as never

// Mirrors the module's unit content so an "installed" read matches.
const UNIT = [
  '[Unit]',
  'Description=LIVI wireless projection AP (early boot)',
  'After=network-pre.target',
  'Wants=network-pre.target',
  'ConditionPathExists=/data/driver/livi-helperd',
  '',
  '[Service]',
  'Type=simple',
  'Environment=SUDO_USER=pi',
  'ExecStart=/data/driver/livi-helperd --wifi-ap',
  'ExecStop=/data/driver/livi-helperd --wifi-ap-teardown',
  'TimeoutStopSec=8',
  'Restart=on-failure',
  'RestartSec=5',
  '',
  '[Install]',
  'WantedBy=multi-user.target',
  ''
].join('\n')

type Cfg = {
  wifiDedicatedInterface: boolean
  wirelessCpEnabled: boolean
  wirelessAaEnabled: boolean
}
const cfg = (o: Partial<Cfg> = {}): never =>
  ({
    wifiDedicatedInterface: false,
    wirelessCpEnabled: false,
    wirelessAaEnabled: false,
    ...o
  }) as never

// Each spawned process closes with the given code.
function autoClose(code = 0): void {
  mockedSpawn.mockImplementation(() => {
    const proc = new EventEmitter()
    queueMicrotask(() => proc.emit('close', code))
    return proc
  })
}

// Each spawned process fails with an error event.
function autoError(): void {
  mockedSpawn.mockImplementation(() => {
    const proc = new EventEmitter()
    queueMicrotask(() => proc.emit('error', new Error('spawn fail')))
    return proc
  })
}

// Unit + marker already present and current.
function installed(): void {
  mockedExists.mockReturnValue(true)
  mockedRead.mockImplementation((p: string) =>
    String(p).includes('.wifi-ap-install') ? '3' : UNIT
  )
}

const spawnCmds = (): string[] => mockedSpawn.mock.calls.map((c) => String(c[0]))

const pkexecScript = (): string =>
  String(mockedSpawn.mock.calls.find((c) => c[0] === 'pkexec')?.[1]?.[2] ?? '')

const sudoLines = (): string[] =>
  mockedSpawn.mock.calls.filter((c) => c[0] === 'sudo').map((c) => (c[1] as string[]).join(' '))

describe('reconcileWifiAp — wanted', () => {
  let realPlatform: PropertyDescriptor | undefined
  beforeEach(() => {
    realPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    vi.clearAllMocks()
    mockedExec.mockReturnValue('/usr/bin/systemctl\n')
    autoClose(0)
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('installs unit + sudoers and starts the AP when not installed', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockReturnValue('')
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(mockedDialog).toHaveBeenCalled()
    const script = pkexecScript()
    expect(script).toContain('ExecStop=/data/driver/livi-helperd --wifi-ap-teardown')
    expect(script).toContain('/etc/sudoers.d/99-LIVI-wifi-ap')
    expect(mockedWrite).toHaveBeenCalled()
    expect(sudoLines()).toContain('-n /usr/bin/systemctl enable livi-wifi-ap.service')
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('dedicated off + wireless on: no boot-persist, but the AP is started', async () => {
    installed()
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(mockedDialog).not.toHaveBeenCalled()
    expect(sudoLines()).toContain('-n /usr/bin/systemctl disable livi-wifi-ap.service')
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('installs without a dialog when no window is given', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockReturnValue('')
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(mockedDialog).not.toHaveBeenCalled()
    expect(pkexecScript()).toContain('/etc/sudoers.d/99-LIVI-wifi-ap')
  })

  test('a failed install is caught and the service is not started', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockReturnValue('')
    autoClose(126)
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(pkexecScript()).not.toBe('')
    expect(sudoLines()).toEqual([])
    err.mockRestore()
  })

  test('an unreadable unit file is treated as absent and installs', async () => {
    mockedExists.mockReturnValue(true)
    mockedRead.mockImplementation(() => {
      throw new Error('EACCES')
    })
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(pkexecScript()).not.toBe('')
  })

  test('falls back to /usr/bin/systemctl when `which` fails', async () => {
    installed()
    mockedExec.mockImplementation(() => {
      throw new Error('nope')
    })
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('falls back to /usr/bin/systemctl when `which` returns nothing', async () => {
    installed()
    mockedExec.mockReturnValue('')
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('a start spawn error resolves without throwing', async () => {
    installed()
    autoError()
    await expect(reconcileWifiAp(cfg({ wirelessCpEnabled: true }))).resolves.toBeUndefined()
  })

  test('an install spawn error is caught', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockReturnValue('')
    autoError()
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await expect(
      reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    ).resolves.toBeUndefined()
    err.mockRestore()
  })

  test('declined install does not spawn pkexec', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockReturnValue('')
    mockedDialog.mockResolvedValueOnce({ response: 1 })
    await reconcileWifiAp(cfg({ wirelessAaEnabled: true }), win)
    expect(pkexecScript()).toBe('')
  })

  test('a concurrent reconcile is skipped while installing', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockReturnValue('')
    let release: (v: { response: number }) => void = () => {}
    mockedDialog.mockImplementationOnce(() => new Promise((r) => (release = r)))
    const p1 = reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    const p2 = reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    await p2
    expect(mockedDialog).toHaveBeenCalledTimes(1)
    release({ response: 0 })
    await p1
  })

  test('is a no-op off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(mockedSpawn).not.toHaveBeenCalled()
  })
})

describe('reconcileWifiAp — not wanted', () => {
  let realPlatform: PropertyDescriptor | undefined
  beforeEach(() => {
    realPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    vi.clearAllMocks()
    mockedExec.mockReturnValue('/usr/bin/systemctl\n')
    autoClose(0)
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('returns the interface when the unmanaged conf is present', async () => {
    mockedExists.mockReturnValue(true)
    await reconcileWifiAp(cfg())
    const s = pkexecScript()
    expect(s).toContain('systemctl disable --now livi-wifi-ap.service')
    expect(s).toContain('rm -f /etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf')
    expect(s).toContain('nmcli device set wlan0 managed yes')
  })

  test('returns the interface when the service is still active', async () => {
    mockedExists.mockReturnValue(false)
    autoClose(0) // systemctl is-active → 0
    await reconcileWifiAp(cfg())
    expect(pkexecScript()).toContain('systemctl disable --now livi-wifi-ap.service')
  })

  test('does nothing when the interface was never taken', async () => {
    mockedExists.mockReturnValue(false)
    autoClose(1) // is-active / is-enabled → non-zero
    await reconcileWifiAp(cfg())
    expect(spawnCmds()).not.toContain('pkexec')
  })

  test('returns the interface when only the service is still enabled', async () => {
    mockedExists.mockReturnValue(false)
    let call = 0
    // is-active → inactive, is-enabled → enabled, then the pkexec release.
    mockedSpawn.mockImplementation(() => {
      const code = call++ === 0 ? 1 : 0
      const proc = new EventEmitter()
      queueMicrotask(() => proc.emit('close', code))
      return proc
    })
    await reconcileWifiAp(cfg())
    expect(pkexecScript()).toContain('systemctl disable --now livi-wifi-ap.service')
  })

  test('a wedged release is SIGKILLed after the timeout', async () => {
    vi.useFakeTimers()
    mockedExists.mockReturnValue(true) // conf present → taken, no probe spawn
    const proc = Object.assign(new EventEmitter(), {
      kill: vi.fn(function (this: EventEmitter) {
        this.emit('close', null)
      })
    })
    mockedSpawn.mockReturnValue(proc)
    const p = reconcileWifiAp(cfg())
    await vi.advanceTimersByTimeAsync(12_000)
    await p
    expect(proc.kill).toHaveBeenCalledWith('SIGKILL')
    vi.useRealTimers()
  })

  test('a release spawn error resolves cleanly', async () => {
    mockedExists.mockReturnValue(true)
    autoError()
    await expect(reconcileWifiAp(cfg())).resolves.toBeUndefined()
  })

  test('a probe spawn error counts as not taken', async () => {
    mockedExists.mockReturnValue(false)
    autoError() // is-active / is-enabled error → false
    await reconcileWifiAp(cfg())
    expect(spawnCmds()).not.toContain('pkexec')
  })

  test('releases the configured interface name', async () => {
    mockedExists.mockReturnValue(true)
    await reconcileWifiAp({ ...cfg(), wifiInterface: 'wlan1' } as never)
    expect(pkexecScript()).toContain('nmcli device set wlan1 managed yes')
  })

  test('is a no-op off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    await reconcileWifiAp(cfg())
    expect(mockedSpawn).not.toHaveBeenCalled()
  })
})

describe('releaseWifiApForQuit', () => {
  let realPlatform: PropertyDescriptor | undefined
  beforeEach(() => {
    realPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    vi.clearAllMocks()
    mockedExec.mockReturnValue('/usr/bin/systemctl\n')
    autoClose(0)
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('dedicated keeps the AP: no stop', async () => {
    await releaseWifiApForQuit(cfg({ wifiDedicatedInterface: true }))
    expect(mockedSpawn).not.toHaveBeenCalled()
  })

  test('non-dedicated returns the interface: stops the service', async () => {
    await releaseWifiApForQuit(cfg({ wirelessCpEnabled: true }))
    expect(sudoLines()).toEqual(['-n /usr/bin/systemctl stop livi-wifi-ap.service'])
  })

  test('is a no-op off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    await releaseWifiApForQuit(cfg())
    expect(mockedSpawn).not.toHaveBeenCalled()
  })
})
