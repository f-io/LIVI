import { execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { dialog } from 'electron'
import type { Mock } from 'vitest'
import {
  checkAndInstallGvfsGuard,
  startPhoneSuppression,
  stopPhoneSuppression
} from '../services/gvfsPhoneGuard'

vi.mock('node:child_process', () => ({ execFileSync: vi.fn(), spawn: vi.fn() }))
vi.mock('node:fs', () => {
  const __m = {
    existsSync: vi.fn(() => false),
    readFileSync: vi.fn(),
    writeFileSync: vi.fn(),
    mkdtempSync: vi.fn(() => '/tmp/livi-install-x'),
    rmSync: vi.fn()
  }
  return { ...__m, default: __m }
})
vi.mock('node:os', () => {
  const __m = { userInfo: vi.fn(() => ({ username: 'driver' })), tmpdir: () => '/tmp' }
  return { ...__m, default: __m }
})
vi.mock('../services/projection/driver/helper/helperSupervisor', () => ({
  resolveHelperBin: () => '/data/driver/livi-helperd'
}))
vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/tmp') },
  dialog: { showMessageBox: vi.fn(() => Promise.resolve({ response: 0 })) }
}))

const mockedExec = execFileSync as Mock
const mockedSpawn = spawn as Mock
const mockedExists = existsSync as Mock
const mockedWrite = writeFileSync as Mock
const mockedDialog = dialog.showMessageBox as Mock

const GUARD_PATH = '/usr/local/lib/livi/gvfs-phone-guard.sh'
const MONITOR = '/usr/share/gvfs/remote-volume-monitors/afc.monitor'
const win = {} as never

function makeProc(): EventEmitter & { unref: Mock } {
  const proc = new EventEmitter() as EventEmitter & { unref: Mock }
  proc.unref = vi.fn()
  return proc
}

function existing(...paths: string[]): void {
  mockedExists.mockImplementation((p: string) => paths.includes(String(p)))
}

const originalPlatform = process.platform
const originalSudoUser = process.env.SUDO_USER

beforeEach(() => {
  mockedExec.mockReset()
  mockedSpawn.mockReset()
  mockedWrite.mockReset()
  mockedDialog.mockReset()
  mockedDialog.mockResolvedValue({ response: 0 })
  mockedExists.mockReset()
  mockedExists.mockReturnValue(false)
  Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  delete process.env.SUDO_USER
  delete process.env.LIVI_KIOSK
})

afterEach(() => {
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  if (originalSudoUser === undefined) delete process.env.SUDO_USER
  else process.env.SUDO_USER = originalSudoUser
})

describe('checkAndInstallGvfsGuard', () => {
  test('does nothing off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    await checkAndInstallGvfsGuard(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('skips when the sudo rule is already active', async () => {
    existing(GUARD_PATH)
    mockedExec.mockReturnValue('Cmnd_Alias LIVI_GVFS = ...')
    await checkAndInstallGvfsGuard(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('recognizes the rule by the guard path too', async () => {
    existing(GUARD_PATH)
    mockedExec.mockReturnValue(`(root) NOPASSWD: ${GUARD_PATH} disable`)
    await checkAndInstallGvfsGuard(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('skips via the marker when sudo -l is unavailable', async () => {
    // Install once so the marker holds what this build writes, then read it back.
    existing(MONITOR)
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)
    const install = checkAndInstallGvfsGuard(win)
    await new Promise((r) => setTimeout(r, 0))
    proc.emit('close', 0)
    await install
    const written = mockedWrite.mock.calls[0][1] as string
    mockedDialog.mockClear()

    existing(GUARD_PATH)
    ;(readFileSync as Mock).mockImplementation((p: string) =>
      String(p) === '/tmp/gvfs-guard-v1.installed' ? written : ''
    )
    mockedExec.mockImplementation(() => {
      throw new Error('sudo: a password is required')
    })
    await checkAndInstallGvfsGuard(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('installs through the helper without a prompt when the rule allows it', async () => {
    existing(MONITOR)
    mockedExec.mockImplementation((cmd: string, args: string[] = []) => {
      if (cmd === 'sudo' && args.includes('-l')) throw new Error('no rule listed')
      return ''
    })
    await checkAndInstallGvfsGuard(win)
    expect(mockedDialog).not.toHaveBeenCalled()
    expect(mockedSpawn).not.toHaveBeenCalled()
    const args = mockedExec.mock.calls.find((c) => c[0] === 'sudo' && !c[1].includes('-l'))?.[1]
    expect(args.slice(0, 3)).toEqual(['-n', '/data/driver/livi-helperd', '--install-gvfs-guard'])
    expect(mockedWrite).toHaveBeenCalledWith(
      '/tmp/gvfs-guard-v1.installed',
      expect.stringMatching(/^[0-9a-f]{16}$/),
      { mode: 0o644 }
    )
  })

  test('never prompts in kiosk mode', async () => {
    process.env.LIVI_KIOSK = '1'
    existing(MONITOR)
    await checkAndInstallGvfsGuard(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('skips when no phone volume monitors exist', async () => {
    await checkAndInstallGvfsGuard(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('detects a disabled monitor left behind by the guard', async () => {
    existing('/usr/share/gvfs/remote-volume-monitors/mtp.livi-off')
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'which') throw new Error('not found')
      throw new Error('no sudo')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await checkAndInstallGvfsGuard(win)
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('pkexec not available'))
    expect(mockedDialog).not.toHaveBeenCalled()
    warnSpy.mockRestore()
  })

  test('does not install when the user skips the dialog', async () => {
    existing(MONITOR)
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    mockedDialog.mockResolvedValueOnce({ response: 1 })
    await checkAndInstallGvfsGuard(win)
    expect(mockedSpawn).not.toHaveBeenCalled()
  })

  test('installs via pkexec and writes the sentinel', async () => {
    process.env.SUDO_USER = 'sudo-driver'
    existing(MONITOR)
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)

    const done = checkAndInstallGvfsGuard(win)
    await new Promise((r) => setImmediate(r))
    proc.emit('close', 0)
    await done

    const [cmd, args] = mockedSpawn.mock.calls[0]
    expect(cmd).toBe('pkexec')
    expect(args[0]).toBe('bash')
    expect(args[2]).toContain('sudo-driver ALL=(root) NOPASSWD: LIVI_GVFS')
    expect(mockedWrite).toHaveBeenCalledWith(
      '/tmp/gvfs-guard-v1.installed',
      expect.stringMatching(/^[0-9a-f]{16}$/),
      { mode: 0o644 }
    )
  })

  test('survives a failing sentinel write', async () => {
    existing(MONITOR)
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    mockedWrite.mockImplementation(() => {
      throw new Error('read-only')
    })
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)

    const done = checkAndInstallGvfsGuard(win)
    await new Promise((r) => setImmediate(r))
    proc.emit('close', 0)
    await expect(done).resolves.toBeUndefined()
    expect(mockedSpawn.mock.calls[0][1][2]).toContain('driver ALL=(root) NOPASSWD: LIVI_GVFS')
  })

  test('logs when pkexec exits non-zero', async () => {
    existing(MONITOR)
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const done = checkAndInstallGvfsGuard(win)
    await new Promise((r) => setImmediate(r))
    proc.emit('close', 126)
    await done

    expect(errSpy).toHaveBeenCalledWith('[gvfsGuard] install failed:', expect.any(Error))
    expect(mockedWrite).not.toHaveBeenCalledWith(
      '/tmp/gvfs-guard-v1.installed',
      expect.anything(),
      expect.anything()
    )
    errSpy.mockRestore()
  })

  test('logs when pkexec cannot be spawned', async () => {
    existing(MONITOR)
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const done = checkAndInstallGvfsGuard(win)
    await new Promise((r) => setImmediate(r))
    proc.emit('error', new Error('ENOENT'))
    await done

    expect(errSpy).toHaveBeenCalledWith('[gvfsGuard] install failed:', expect.any(Error))
    errSpy.mockRestore()
  })
})

describe('startPhoneSuppression', () => {
  test('does nothing off linux or without the guard script', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    startPhoneSuppression()
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    startPhoneSuppression()
    expect(mockedExec).not.toHaveBeenCalled()
  })

  test('heals, disables and spawns the detached restore watcher', () => {
    existing(GUARD_PATH)
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)

    startPhoneSuppression()

    expect(mockedExec).toHaveBeenNthCalledWith(1, 'sudo', ['-n', GUARD_PATH, 'restore'], {
      stdio: 'ignore'
    })
    expect(mockedExec).toHaveBeenNthCalledWith(2, 'sudo', ['-n', GUARD_PATH, 'disable'], {
      stdio: 'ignore'
    })
    const [cmd, args, opts] = mockedSpawn.mock.calls[0]
    expect(cmd).toBe('bash')
    expect(args[1]).toContain(`kill -0 ${process.pid}`)
    expect(args[1]).toContain(`${GUARD_PATH} restore`)
    expect(opts).toMatchObject({ detached: true })
    expect(proc.unref).toHaveBeenCalledTimes(1)
  })

  test('warns and skips the watcher when sudo refuses', () => {
    existing(GUARD_PATH)
    mockedExec.mockImplementation(() => {
      throw new Error('password required')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    startPhoneSuppression()

    expect(warnSpy).toHaveBeenCalledWith(
      '[gvfsGuard] could not disable phone monitors:',
      'password required'
    )
    expect(mockedSpawn).not.toHaveBeenCalled()
    warnSpy.mockRestore()
  })
})

describe('stopPhoneSuppression', () => {
  test('does nothing off linux or without the guard script', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    stopPhoneSuppression()
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    stopPhoneSuppression()
    expect(mockedExec).not.toHaveBeenCalled()
  })

  test('restores the monitors', () => {
    existing(GUARD_PATH)
    stopPhoneSuppression()
    expect(mockedExec).toHaveBeenCalledWith('sudo', ['-n', GUARD_PATH, 'restore'], {
      stdio: 'ignore'
    })
  })

  test('warns when the restore fails', () => {
    existing(GUARD_PATH)
    mockedExec.mockImplementation(() => {
      throw new Error('nope')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    stopPhoneSuppression()
    expect(warnSpy).toHaveBeenCalledWith('[gvfsGuard] could not restore phone monitors:', 'nope')
    warnSpy.mockRestore()
  })
})
