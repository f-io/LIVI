import { execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dialog } from 'electron'
import { afterEach, beforeEach, describe, expect, type Mock, test, vi } from 'vitest'

vi.mock('node:child_process', () => ({ spawn: vi.fn(), execFileSync: vi.fn() }))
vi.mock('../staged', () => ({
  helperRestaged: vi.fn(() => false),
  markHelperRestaged: vi.fn(),
  clearHelperRestaged: vi.fn()
}))
vi.mock('../helperSupervisor', () => ({ resolveHelperBin: () => '/data/driver/livi-helperd' }))
vi.mock('node:fs', () => ({
  existsSync: vi.fn(),
  readFileSync: vi.fn(),
  writeFileSync: vi.fn(),
  mkdtempSync: vi.fn(() => '/tmp/livi-ap-test'),
  rmSync: vi.fn()
}))
vi.mock('node:os', () => ({
  default: { userInfo: () => ({ username: 'pi' }), tmpdir: () => '/tmp' },
  userInfo: () => ({ username: 'pi' }),
  tmpdir: () => '/tmp'
}))
vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/data'), getAppPath: vi.fn(() => '/app') },
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

// Stands in for the shipped templates, so the test exercises the substitution.
const UNIT_TPL = [
  '[Unit]',
  'ConditionPathExists=__HELPER__',
  '',
  '[Service]',
  'Environment=SUDO_USER=__USERNAME__',
  'ExecStart=__HELPER__ --wifi-ap',
  'ExecStop=__HELPER__ --wifi-ap-teardown',
  ''
].join('\n')
const SUDOERS_TPL = [
  'Cmnd_Alias LIVI_WIFI_AP = __SYSTEMCTL__ restart livi-wifi-ap.service, __HELPER__ --wifi-ap-teardown',
  '__USERNAME__ ALL=(root) NOPASSWD: LIVI_WIFI_AP',
  ''
].join('\n')
const UNIT = UNIT_TPL.replace(/__HELPER__/g, '/data/driver/livi-helperd').replace(
  /__USERNAME__/g,
  'pi'
)

// What `sudo -n -l` answers. Only a rule naming the service counts as installed.
const NO_AP_RULE = 'User pi may run the following commands:\n    (root) NOPASSWD: /usr/bin/true\n'
const AP_RULE =
  'User pi may run the following commands:\n    (root) NOPASSWD: /usr/bin/systemctl restart livi-wifi-ap.service\n'

// helperRoot: whether `sudo -n livi-helperd` is permitted.
const exec = {
  which: '/usr/bin/systemctl\n',
  sudoList: NO_AP_RULE,
  helper: '',
  helperRoot: false,
  id: 'pi\n'
}

// Nothing installed yet, but the templates that ship with the app are there.
function templatesOnly(p: string): string {
  const path = String(p)
  if (path.endsWith('livi-wifi-ap.service.template')) return UNIT_TPL
  if (path.endsWith('99-LIVI-wifi-ap.sudoers.template')) return SUDOERS_TPL
  return ''
}

function execDispatch(cmd: string, args: string[] = []): string {
  if (cmd === 'which') return exec.which
  if (cmd === 'sudo' && args.includes('--install-wifi-ap')) {
    if (!exec.helperRoot) throw new Error('sudo: a password is required')
    return ''
  }
  if (cmd === 'sudo') return exec.sudoList
  if (cmd === 'id') return exec.id
  if (args.includes('--wifi-ap-status')) return exec.helper
  return ''
}

type Cfg = {
  wifiDedicatedInterface: boolean
  wirelessCpEnabled: boolean
  wirelessAaEnabled: boolean
  wifiInterface: string
  wifiChannel: number
  wifiChannelWidth: number
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

// Unit in place and the sudoers rule in force.
function installed(): void {
  mockedExists.mockReturnValue(true)
  mockedRead.mockImplementation((p: string) => {
    const path = String(p)
    if (path.endsWith('livi-wifi-ap.service.template')) return UNIT_TPL
    if (path.endsWith('99-LIVI-wifi-ap.sudoers.template')) return SUDOERS_TPL
    return UNIT
  })
  exec.sudoList = AP_RULE
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
    exec.which = '/usr/bin/systemctl\n'
    exec.sudoList = NO_AP_RULE
    exec.helper = ''
    exec.helperRoot = false
    exec.id = 'pi\n'
    mockedExec.mockImplementation(execDispatch)
    autoClose(0)
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('installs unit + sudoers and starts the AP when not installed', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(mockedDialog).toHaveBeenCalled()
    const script = pkexecScript()
    expect(script).toContain('ExecStop=/data/driver/livi-helperd --wifi-ap-teardown')
    expect(script).toContain('/etc/sudoers.d/99-LIVI-wifi-ap')
    expect(mockedWrite).toHaveBeenCalled()
    expect(sudoLines()).toContain('-n /usr/bin/systemctl enable livi-wifi-ap.service')
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('restartWifiAp hands the settings to the running service', async () => {
    const { restartWifiAp } = await import('../wifiApUnit')
    installed()
    await restartWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl restart livi-wifi-ap.service')
  })

  test('a refused channel is written back from what the service ended up on', async () => {
    const { restartWifiAp, setWifiApReport } = await import('../wifiApUnit')
    const patches: Record<string, unknown>[] = []
    setWifiApReport((p) => patches.push(p))
    installed()
    exec.helper = 'running true\nssid LIVI\nchannel 36\nwidth 20\n'
    await restartWifiAp(cfg({ wirelessCpEnabled: true, wifiChannel: 149, wifiChannelWidth: 80 }))
    await new Promise((done) => setTimeout(done, 0))
    expect(patches).toEqual([{ wifiChannel: 36, wifiChannelWidth: 20 }])
    setWifiApReport(() => {})
  })

  test('the boot fallback is written back once the settle is asked for', async () => {
    const { settleWifiAp, setWifiApReport } = await import('../wifiApUnit')
    const patches: Record<string, unknown>[] = []
    setWifiApReport((p) => patches.push(p))
    installed()
    exec.helper = 'running true\nssid LIVI\nchannel 44\nwidth 20\n'
    await settleWifiAp(cfg({ wirelessCpEnabled: true, wifiChannel: 149, wifiChannelWidth: 20 }))
    expect(patches).toEqual([{ wifiChannel: 44 }])
    setWifiApReport(() => {})
  })

  test('a settings change alone writes nothing back, the service still runs the old one', async () => {
    const { setWifiApReport } = await import('../wifiApUnit')
    const patches: Record<string, unknown>[] = []
    setWifiApReport((p) => patches.push(p))
    installed()
    exec.helper = 'running true\nssid LIVI\nchannel 44\nwidth 20\n'
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true, wifiChannel: 36, wifiChannelWidth: 20 }))
    await new Promise((done) => setTimeout(done, 0))
    expect(patches).toEqual([])
    setWifiApReport(() => {})
  })

  test('nothing is written back while the service runs what was asked for', async () => {
    const { settleWifiAp, setWifiApReport } = await import('../wifiApUnit')
    const patches: Record<string, unknown>[] = []
    setWifiApReport((p) => patches.push(p))
    installed()
    exec.helper = 'running true\nssid LIVI\nchannel 44\nwidth 20\n'
    await settleWifiAp(cfg({ wirelessCpEnabled: true, wifiChannel: 44, wifiChannelWidth: 20 }))
    expect(patches).toEqual([])
    setWifiApReport(() => {})
  })

  test('restartWifiAp says nothing while no access point is wanted', async () => {
    const { restartWifiAp } = await import('../wifiApUnit')
    installed()
    await restartWifiAp(cfg({}))
    expect(sudoLines()).toHaveLength(0)
  })

  test('restarts the service when a new helper binary was staged', async () => {
    const { helperRestaged } = await import('../staged')
    vi.mocked(helperRestaged).mockReturnValueOnce(true)
    installed()
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl restart livi-wifi-ap.service')
    expect(sudoLines()).not.toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
    const { clearHelperRestaged } = await import('../staged')
    expect(clearHelperRestaged).toHaveBeenCalled()
  })

  test('dedicated off + wireless on: no boot-persist, but the AP is started', async () => {
    installed()
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(mockedDialog).not.toHaveBeenCalled()
    expect(sudoLines()).toContain('-n /usr/bin/systemctl disable livi-wifi-ap.service')
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('installs through the helper when it may run as root, without pkexec', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    exec.helperRoot = true
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(mockedDialog).not.toHaveBeenCalled()
    expect(spawnCmds()).not.toContain('pkexec')
    const call = mockedExec.mock.calls.find((c) =>
      (c[1] as string[])?.includes('--install-wifi-ap')
    )
    expect(call?.[1]).toEqual([
      '-n',
      '/data/driver/livi-helperd',
      '--install-wifi-ap',
      '/tmp/livi-ap-test/unit',
      '/tmp/livi-ap-test/rule'
    ])
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('falls back to pkexec when the helper cannot run as root', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(spawnCmds()).toContain('pkexec')
  })

  test('installs without a dialog when no window is given', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(mockedDialog).not.toHaveBeenCalled()
    expect(pkexecScript()).toContain('/etc/sudoers.d/99-LIVI-wifi-ap')
  })

  test('a failed install is caught and the service is not started', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    autoClose(126)
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(pkexecScript()).not.toBe('')
    expect(sudoLines()).toEqual([])
    err.mockRestore()
  })

  test('an unreadable unit file is treated as absent and installs', async () => {
    installed()
    const readable = mockedRead.getMockImplementation() as (p: string) => string
    mockedRead.mockImplementation((p: string) => {
      if (String(p).startsWith('/etc/')) throw new Error('EACCES')
      return readable(p)
    })
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(pkexecScript()).not.toBe('')
  })

  test('falls back to /usr/bin/systemctl when `which` fails', async () => {
    installed()
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'which' && args[0] === 'systemctl') throw new Error('nope')
      return execDispatch(cmd, args)
    })
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('falls back to /usr/bin/systemctl when `which` returns nothing', async () => {
    installed()
    exec.which = ''
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
    mockedRead.mockImplementation(templatesOnly)
    autoError()
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await expect(
      reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    ).resolves.toBeUndefined()
    err.mockRestore()
  })

  test('declined install does not spawn pkexec', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    mockedDialog.mockResolvedValueOnce({ response: 1 })
    await reconcileWifiAp(cfg({ wirelessAaEnabled: true }), win)
    expect(pkexecScript()).toBe('')
  })

  test('a concurrent reconcile is skipped while installing', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    let release: (v: { response: number }) => void = () => {}
    mockedDialog.mockImplementationOnce(() => new Promise((r) => (release = r)))
    const p1 = reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    const p2 = reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    await p2
    expect(mockedDialog).toHaveBeenCalledTimes(1)
    release({ response: 0 })
    await p1
  })

  test('reads the template packaged next to the app when it is there', async () => {
    const real = Object.getOwnPropertyDescriptor(process, 'resourcesPath')
    Object.defineProperty(process, 'resourcesPath', { value: '/res', configurable: true })
    mockedExists.mockImplementation((p: string) => String(p).startsWith('/res'))
    mockedRead.mockImplementation(templatesOnly)
    exec.helperRoot = true
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(mockedRead).toHaveBeenCalledWith('/res/livi-wifi-ap.service.template', 'utf8')
    if (real) Object.defineProperty(process, 'resourcesPath', real)
  })

  test('the rule names the user behind pkexec, then the one behind sudo', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    process.env.PKEXEC_UID = '1000'
    exec.id = 'desktop-user\n'
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(pkexecScript()).toContain('SUDO_USER=desktop-user')

    vi.clearAllMocks()
    mockedExec.mockImplementation(execDispatch)
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    autoClose(0)
    process.env.PKEXEC_UID = ''
    process.env.SUDO_USER = 'sudo-user'
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(pkexecScript()).toContain('SUDO_USER=sudo-user')
    process.env.SUDO_USER = ''
  })

  test('a sudo that cannot list the rules installs rather than assume', async () => {
    installed()
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'sudo' && args.includes('-l')) throw new Error('a password is required')
      return execDispatch(cmd, args)
    })
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(pkexecScript()).not.toBe('')
  })

  test('templates that cannot be read stop the reconcile', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(() => {
      throw new Error('EACCES')
    })
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(err).toHaveBeenCalled()
    expect(sudoLines()).toEqual([])
    err.mockRestore()
  })

  test('no helper and no pkexec says what to run instead', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'which' && args[0] === 'pkexec') throw new Error('not found')
      return execDispatch(cmd, args)
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(warn.mock.calls[0]?.[0]).toContain('Run the LIVI install script')
    expect(spawnCmds()).not.toContain('pkexec')
    expect(sudoLines()).toEqual([])
    warn.mockRestore()
  })

  test('the dongle as the interface releases the local AP', async () => {
    installed()
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true, wifiInterface: 'livi-link' }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl stop livi-wifi-ap.service')
    expect(sudoLines()).not.toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('a status readback that fails writes nothing back', async () => {
    vi.useFakeTimers()
    const { restartWifiAp, setWifiApReport } = await import('../wifiApUnit')
    const patches: Record<string, unknown>[] = []
    setWifiApReport((p) => patches.push(p))
    installed()
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (args.includes('--wifi-ap-status')) throw new Error('no helper')
      return execDispatch(cmd, args)
    })
    await restartWifiAp(cfg({ wirelessCpEnabled: true }))
    await vi.advanceTimersByTimeAsync(40_000)
    expect(patches).toEqual([])
    setWifiApReport(() => {})
    vi.useRealTimers()
  })

  test('a host that grants passwordless sudo for everything needs no install', async () => {
    mockedExists.mockReturnValue(true)
    mockedRead.mockImplementation((p: string) => {
      const path = String(p)
      if (path.endsWith('livi-wifi-ap.service.template')) return UNIT_TPL
      if (path.endsWith('99-LIVI-wifi-ap.sudoers.template')) return SUDOERS_TPL
      return UNIT
    })
    exec.sudoList = 'User pi may run the following commands:\n    (ALL : ALL) NOPASSWD: ALL\n'
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(pkexecScript()).toBe('')
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('a temp directory that cannot be made falls back to pkexec', async () => {
    const { mkdtempSync } = await import('node:fs')
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    exec.helperRoot = true
    ;(mkdtempSync as Mock).mockImplementationOnce(() => {
      throw new Error('EROFS')
    })
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(spawnCmds()).toContain('pkexec')
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
    exec.which = '/usr/bin/systemctl\n'
    exec.sudoList = NO_AP_RULE
    exec.helper = ''
    exec.helperRoot = false
    exec.id = 'pi\n'
    mockedExec.mockImplementation(execDispatch)
    autoClose(0)
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('returns the interface when the unmanaged conf is present', async () => {
    mockedExists.mockReturnValue(true)
    autoClose(0)
    await reconcileWifiAp(cfg())
    const lines = sudoLines()
    expect(lines.some((l) => l.includes('stop livi-wifi-ap.service'))).toBe(true)
    expect(lines.some((l) => l.includes('disable livi-wifi-ap.service'))).toBe(true)
    expect(lines.some((l) => l.includes('--wifi-ap-teardown'))).toBe(true)
    expect(spawnCmds()).not.toContain('pkexec')
  })

  test('returns the interface when the service is still active', async () => {
    mockedExists.mockReturnValue(false)
    autoClose(0) // systemctl is-active → 0
    await reconcileWifiAp(cfg())
    expect(sudoLines().some((l) => l.includes('--wifi-ap-teardown'))).toBe(true)
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
    expect(sudoLines().some((l) => l.includes('--wifi-ap-teardown'))).toBe(true)
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
    for (let i = 0; i < 3; i++) await vi.advanceTimersByTimeAsync(12_000)
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

  test('leaves it to the helper which interface goes back', async () => {
    mockedExists.mockReturnValue(true)
    autoClose(0)
    await reconcileWifiAp({ ...cfg(), wifiInterface: 'wlan1' } as never)
    const teardown = sudoLines().find((l) => l.includes('--wifi-ap-teardown'))
    expect(teardown).toBeDefined()
    expect(teardown).not.toContain('wlan1')
  })

  test('reads the template packaged next to the app when it is there', async () => {
    const real = Object.getOwnPropertyDescriptor(process, 'resourcesPath')
    Object.defineProperty(process, 'resourcesPath', { value: '/res', configurable: true })
    mockedExists.mockImplementation((p: string) => String(p).startsWith('/res'))
    mockedRead.mockImplementation(templatesOnly)
    exec.helperRoot = true
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(mockedRead).toHaveBeenCalledWith('/res/livi-wifi-ap.service.template', 'utf8')
    if (real) Object.defineProperty(process, 'resourcesPath', real)
  })

  test('the rule names the user behind pkexec, then the one behind sudo', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    process.env.PKEXEC_UID = '1000'
    exec.id = 'desktop-user\n'
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(pkexecScript()).toContain('SUDO_USER=desktop-user')

    vi.clearAllMocks()
    mockedExec.mockImplementation(execDispatch)
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    autoClose(0)
    process.env.PKEXEC_UID = ''
    process.env.SUDO_USER = 'sudo-user'
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(pkexecScript()).toContain('SUDO_USER=sudo-user')
    process.env.SUDO_USER = ''
  })

  test('a sudo that cannot list the rules installs rather than assume', async () => {
    installed()
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'sudo' && args.includes('-l')) throw new Error('a password is required')
      return execDispatch(cmd, args)
    })
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(pkexecScript()).not.toBe('')
  })

  test('templates that cannot be read stop the reconcile', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(() => {
      throw new Error('EACCES')
    })
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(err).toHaveBeenCalled()
    expect(sudoLines()).toEqual([])
    err.mockRestore()
  })

  test('no helper and no pkexec says what to run instead', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'which' && args[0] === 'pkexec') throw new Error('not found')
      return execDispatch(cmd, args)
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(warn.mock.calls[0]?.[0]).toContain('Run the LIVI install script')
    expect(spawnCmds()).not.toContain('pkexec')
    expect(sudoLines()).toEqual([])
    warn.mockRestore()
  })

  test('the dongle as the interface releases the local AP', async () => {
    installed()
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true, wifiInterface: 'livi-link' }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl stop livi-wifi-ap.service')
    expect(sudoLines()).not.toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('a status readback that fails writes nothing back', async () => {
    vi.useFakeTimers()
    const { restartWifiAp, setWifiApReport } = await import('../wifiApUnit')
    const patches: Record<string, unknown>[] = []
    setWifiApReport((p) => patches.push(p))
    installed()
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (args.includes('--wifi-ap-status')) throw new Error('no helper')
      return execDispatch(cmd, args)
    })
    await restartWifiAp(cfg({ wirelessCpEnabled: true }))
    await vi.advanceTimersByTimeAsync(40_000)
    expect(patches).toEqual([])
    setWifiApReport(() => {})
    vi.useRealTimers()
  })

  test('a host that grants passwordless sudo for everything needs no install', async () => {
    mockedExists.mockReturnValue(true)
    mockedRead.mockImplementation((p: string) => {
      const path = String(p)
      if (path.endsWith('livi-wifi-ap.service.template')) return UNIT_TPL
      if (path.endsWith('99-LIVI-wifi-ap.sudoers.template')) return SUDOERS_TPL
      return UNIT
    })
    exec.sudoList = 'User pi may run the following commands:\n    (ALL : ALL) NOPASSWD: ALL\n'
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(pkexecScript()).toBe('')
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('a temp directory that cannot be made falls back to pkexec', async () => {
    const { mkdtempSync } = await import('node:fs')
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    exec.helperRoot = true
    ;(mkdtempSync as Mock).mockImplementationOnce(() => {
      throw new Error('EROFS')
    })
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(spawnCmds()).toContain('pkexec')
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
    exec.which = '/usr/bin/systemctl\n'
    exec.sudoList = NO_AP_RULE
    exec.helper = ''
    exec.helperRoot = false
    exec.id = 'pi\n'
    mockedExec.mockImplementation(execDispatch)
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

  test('reads the template packaged next to the app when it is there', async () => {
    const real = Object.getOwnPropertyDescriptor(process, 'resourcesPath')
    Object.defineProperty(process, 'resourcesPath', { value: '/res', configurable: true })
    mockedExists.mockImplementation((p: string) => String(p).startsWith('/res'))
    mockedRead.mockImplementation(templatesOnly)
    exec.helperRoot = true
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(mockedRead).toHaveBeenCalledWith('/res/livi-wifi-ap.service.template', 'utf8')
    if (real) Object.defineProperty(process, 'resourcesPath', real)
  })

  test('the rule names the user behind pkexec, then the one behind sudo', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    process.env.PKEXEC_UID = '1000'
    exec.id = 'desktop-user\n'
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(pkexecScript()).toContain('SUDO_USER=desktop-user')

    vi.clearAllMocks()
    mockedExec.mockImplementation(execDispatch)
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    autoClose(0)
    process.env.PKEXEC_UID = ''
    process.env.SUDO_USER = 'sudo-user'
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }))
    expect(pkexecScript()).toContain('SUDO_USER=sudo-user')
    process.env.SUDO_USER = ''
  })

  test('a sudo that cannot list the rules installs rather than assume', async () => {
    installed()
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'sudo' && args.includes('-l')) throw new Error('a password is required')
      return execDispatch(cmd, args)
    })
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(pkexecScript()).not.toBe('')
  })

  test('templates that cannot be read stop the reconcile', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(() => {
      throw new Error('EACCES')
    })
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }))
    expect(err).toHaveBeenCalled()
    expect(sudoLines()).toEqual([])
    err.mockRestore()
  })

  test('no helper and no pkexec says what to run instead', async () => {
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'which' && args[0] === 'pkexec') throw new Error('not found')
      return execDispatch(cmd, args)
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(warn.mock.calls[0]?.[0]).toContain('Run the LIVI install script')
    expect(spawnCmds()).not.toContain('pkexec')
    expect(sudoLines()).toEqual([])
    warn.mockRestore()
  })

  test('the dongle as the interface releases the local AP', async () => {
    installed()
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true, wifiInterface: 'livi-link' }))
    expect(sudoLines()).toContain('-n /usr/bin/systemctl stop livi-wifi-ap.service')
    expect(sudoLines()).not.toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('a status readback that fails writes nothing back', async () => {
    vi.useFakeTimers()
    const { restartWifiAp, setWifiApReport } = await import('../wifiApUnit')
    const patches: Record<string, unknown>[] = []
    setWifiApReport((p) => patches.push(p))
    installed()
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (args.includes('--wifi-ap-status')) throw new Error('no helper')
      return execDispatch(cmd, args)
    })
    await restartWifiAp(cfg({ wirelessCpEnabled: true }))
    await vi.advanceTimersByTimeAsync(40_000)
    expect(patches).toEqual([])
    setWifiApReport(() => {})
    vi.useRealTimers()
  })

  test('a host that grants passwordless sudo for everything needs no install', async () => {
    mockedExists.mockReturnValue(true)
    mockedRead.mockImplementation((p: string) => {
      const path = String(p)
      if (path.endsWith('livi-wifi-ap.service.template')) return UNIT_TPL
      if (path.endsWith('99-LIVI-wifi-ap.sudoers.template')) return SUDOERS_TPL
      return UNIT
    })
    exec.sudoList = 'User pi may run the following commands:\n    (ALL : ALL) NOPASSWD: ALL\n'
    await reconcileWifiAp(cfg({ wirelessCpEnabled: true }), win)
    expect(pkexecScript()).toBe('')
    expect(sudoLines()).toContain('-n /usr/bin/systemctl start livi-wifi-ap.service')
  })

  test('a temp directory that cannot be made falls back to pkexec', async () => {
    const { mkdtempSync } = await import('node:fs')
    mockedExists.mockReturnValue(false)
    mockedRead.mockImplementation(templatesOnly)
    exec.helperRoot = true
    ;(mkdtempSync as Mock).mockImplementationOnce(() => {
      throw new Error('EROFS')
    })
    await reconcileWifiAp(cfg({ wifiDedicatedInterface: true }), win)
    expect(spawnCmds()).toContain('pkexec')
  })

  test('is a no-op off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    await releaseWifiApForQuit(cfg())
    expect(mockedSpawn).not.toHaveBeenCalled()
  })
})

describe('shipped templates', () => {
  test('carry what the app and the installer substitute', async () => {
    const fs = await vi.importActual<typeof import('node:fs')>('node:fs')
    const { join } = await vi.importActual<typeof import('node:path')>('node:path')
    const dir = join(process.cwd(), 'assets', 'linux')
    const unit = fs.readFileSync(join(dir, 'livi-wifi-ap.service.template'), 'utf8')
    const rule = fs.readFileSync(join(dir, '99-LIVI-wifi-ap.sudoers.template'), 'utf8')

    expect(unit).toContain('ConditionPathExists=__HELPER__')
    expect(unit).toContain('Environment=SUDO_USER=__USERNAME__')
    expect(unit).toContain('ExecStop=__HELPER__ --wifi-ap-teardown')
    expect(unit).not.toContain('__SYSTEMCTL__')
    // sudoersActive() recognises an installed rule by this command.
    expect(rule).toContain('__SYSTEMCTL__ restart livi-wifi-ap.service')
    expect(rule).toContain('__USERNAME__ ALL=(root) NOPASSWD: LIVI_WIFI_AP')
    expect(unit.endsWith('\n')).toBe(true)
    expect(rule.endsWith('\n')).toBe(true)
  })
})
