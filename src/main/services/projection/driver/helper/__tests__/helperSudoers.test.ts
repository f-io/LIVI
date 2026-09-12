import { execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { stamp } from '@main/services/privileged'
import { dialog } from 'electron'
import type { Mock } from 'vitest'
import { checkAndInstallHelperSudoers, helperSudoersExists } from '../helperSudoers'

vi.mock('node:child_process', () => ({ execFileSync: vi.fn(), spawn: vi.fn() }))
vi.mock('node:fs', () => {
  const __m = { existsSync: vi.fn(() => false), readFileSync: vi.fn(), writeFileSync: vi.fn() }
  return { ...__m, default: __m }
})
vi.mock('node:os', () => {
  const __m = { userInfo: vi.fn(() => ({ username: 'driver' })) }
  return { ...__m, default: __m }
})
vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/tmp'), getAppPath: vi.fn(() => '/app') },
  dialog: { showMessageBox: vi.fn(() => Promise.resolve({ response: 0 })) }
}))

const mockedExec = execFileSync as Mock
const mockedSpawn = spawn as Mock
const mockedExists = existsSync as Mock
const mockedRead = readFileSync as Mock
const mockedWrite = writeFileSync as Mock
const mockedDialog = dialog.showMessageBox as Mock

const TEMPLATE =
  'Cmnd_Alias LIVI_HELPER = */livi-helperd\n__USERNAME__ ALL=(root) NOPASSWD: LIVI_HELPER\n'
const SENTINEL = '/tmp/helper-sudoers-v1.installed'
const win = {} as never

function makeProc(): EventEmitter {
  return new EventEmitter()
}

function noSudo(): void {
  mockedExec.mockImplementation((cmd: string) => {
    if (cmd === 'which') return ''
    throw new Error('no sudo')
  })
}

const originalPlatform = process.platform
const originalResources = process.resourcesPath
const originalPkexecUid = process.env.PKEXEC_UID
const originalSudoUser = process.env.SUDO_USER

beforeEach(() => {
  mockedExec.mockReset()
  mockedSpawn.mockReset()
  mockedExists.mockReset()
  mockedExists.mockReturnValue(false)
  mockedRead.mockReset()
  mockedRead.mockReturnValue(TEMPLATE)
  mockedWrite.mockReset()
  mockedDialog.mockReset()
  mockedDialog.mockResolvedValue({ response: 0 })
  Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = undefined
  delete process.env.PKEXEC_UID
  delete process.env.SUDO_USER
})

afterEach(() => {
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = originalResources
  if (originalPkexecUid === undefined) delete process.env.PKEXEC_UID
  else process.env.PKEXEC_UID = originalPkexecUid
  if (originalSudoUser === undefined) delete process.env.SUDO_USER
  else process.env.SUDO_USER = originalSudoUser
})

async function install(): Promise<string> {
  const proc = makeProc()
  mockedSpawn.mockReturnValue(proc)
  const done = checkAndInstallHelperSudoers(win)
  await new Promise((r) => setImmediate(r))
  proc.emit('close', 0)
  await done
  return mockedSpawn.mock.calls[0][1][2] as string
}

describe('helperSudoersExists', () => {
  test('true when sudo -n -l lists the alias or the helper', () => {
    mockedExec.mockReturnValueOnce('(root) SETENV: NOPASSWD: /opt/livi/driver/livi-helperd')
    expect(helperSudoersExists()).toBe(true)
    mockedExec.mockReturnValueOnce('(root) NOPASSWD: /opt/livi/driver/livi-helperd')
    expect(helperSudoersExists()).toBe(true)
  })

  test('a python-era rule does not count, so an updated install gets the new one', () => {
    mockedExec.mockReturnValueOnce(
      'User pi may run the following commands:\n    (root) SETENV: NOPASSWD: /usr/bin/python3 *livi-helper.py'
    )
    mockedExists.mockImplementation((p: string) => String(p) === '/tmp/bt-sudoers-v1.installed')
    expect(helperSudoersExists()).toBe(false)
  })

  test('true when the host already grants passwordless sudo for everything', () => {
    mockedExec.mockReturnValueOnce(
      'User pi may run the following commands:\n    (ALL) NOPASSWD: ALL'
    )
    expect(helperSudoersExists()).toBe(true)
  })

  test('falls back to the marker when sudo refuses', () => {
    noSudo()
    const rendered = TEMPLATE.replace(/__USERNAME__/g, 'driver')
    mockedRead.mockImplementation((p: string) =>
      String(p) === SENTINEL ? `${stamp(rendered)}\n` : TEMPLATE
    )
    expect(helperSudoersExists()).toBe(true)
  })

  test('a marker from a different rule text does not count', () => {
    noSudo()
    mockedRead.mockImplementation((p: string) => (String(p) === SENTINEL ? stamp('old') : TEMPLATE))
    expect(helperSudoersExists()).toBe(false)
  })

  test('false when the sentinel check throws', () => {
    noSudo()
    mockedExists.mockImplementation(() => {
      throw new Error('EACCES')
    })
    expect(helperSudoersExists()).toBe(false)
  })
})

describe('checkAndInstallHelperSudoers', () => {
  test('does nothing off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    await checkAndInstallHelperSudoers(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('skips when the rule is already active', async () => {
    mockedExec.mockReturnValue('livi-helperd')
    await checkAndInstallHelperSudoers(win)
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('warns when pkexec is missing', async () => {
    mockedExec.mockImplementation(() => {
      throw new Error('nothing works')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await checkAndInstallHelperSudoers(win)
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('pkexec not available'))
    expect(mockedDialog).not.toHaveBeenCalled()
    warnSpy.mockRestore()
  })

  test('does nothing when the user skips', async () => {
    noSudo()
    mockedDialog.mockResolvedValueOnce({ response: 1 })
    await checkAndInstallHelperSudoers(win)
    expect(mockedSpawn).not.toHaveBeenCalled()
  })

  test('installs the rendered rule, writes the sentinel and confirms', async () => {
    noSudo()
    process.env.SUDO_USER = 'sudo-driver'

    const script = await install()

    expect(script).toContain('sudo-driver ALL=(root) NOPASSWD: LIVI_HELPER')
    expect(script).toContain('Cmnd_Alias LIVI_HELPER = */livi-helperd')
    expect(script).toContain('visudo -c -f /etc/sudoers.d/99-LIVI-helper.livi-tmp')
    // The same grant used to live under a name that only said Bluetooth.
    expect(script).toContain('rm -f /etc/sudoers.d/99-LIVI-bt')
    expect(mockedWrite).toHaveBeenCalledWith(SENTINEL, expect.stringMatching(/^[0-9a-f]{16}$/), {
      mode: 0o644
    })
    expect(mockedDialog).toHaveBeenLastCalledWith(win, expect.objectContaining({ type: 'info' }))
  })

  test('renders the template from the packaged resources when present', async () => {
    noSudo()
    ;(process as { resourcesPath?: string }).resourcesPath = '/res'
    mockedExists.mockImplementation(
      (p: string) => String(p) === '/res/99-LIVI-helper.sudoers.template'
    )

    await install()

    expect(mockedRead).toHaveBeenCalledWith('/res/99-LIVI-helper.sudoers.template', 'utf8')
    expect(mockedSpawn.mock.calls[0][1][2]).toContain('*/livi-helperd')
  })

  test('falls back to the app template when the packaged one is missing', async () => {
    noSudo()
    ;(process as { resourcesPath?: string }).resourcesPath = '/res'

    await install()

    expect(mockedRead).toHaveBeenCalledWith(
      '/app/assets/linux/99-LIVI-helper.sudoers.template',
      'utf8'
    )
  })

  test('resolves the username from PKEXEC_UID and falls back on id errors', async () => {
    process.env.PKEXEC_UID = '1000'
    mockedExec.mockImplementation((cmd: string, args: string[]) => {
      if (cmd === 'id') return 'pkexec-user\n'
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    const script = await install()
    expect(script).toContain('pkexec-user ALL=(root)')

    mockedSpawn.mockReset()
    mockedDialog.mockClear()
    mockedExec.mockImplementation((cmd: string) => {
      if (cmd === 'id') throw new Error('no such uid')
      if (cmd === 'which') return ''
      throw new Error('no sudo')
    })
    const second = await install()
    expect(second).toContain('driver ALL=(root)')
  })

  test('keeps going when the sentinel write fails', async () => {
    noSudo()
    mockedWrite.mockImplementation(() => {
      throw new Error('read-only')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    await install()

    expect(warnSpy).toHaveBeenCalledWith('[helperSudoers] could not write marker:', 'read-only')
    expect(mockedDialog).toHaveBeenLastCalledWith(win, expect.objectContaining({ type: 'info' }))
    warnSpy.mockRestore()
  })

  test('shows the manual command when pkexec exits non-zero', async () => {
    noSudo()
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const done = checkAndInstallHelperSudoers(win)
    await new Promise((r) => setImmediate(r))
    proc.emit('close', 126)
    await done

    expect(errSpy).toHaveBeenCalledWith('[helperSudoers] installation failed:', expect.any(Error))
    const opts = mockedDialog.mock.calls.at(-1)?.[1]
    expect(opts.type).toBe('error')
    expect(opts.detail).toContain("sudo tee /etc/sudoers.d/99-LIVI-helper <<'EOF'")
    expect(opts.detail).toContain('driver ALL=(root) NOPASSWD: LIVI_HELPER')
    errSpy.mockRestore()
  })

  test('handles a pkexec spawn error the same way', async () => {
    noSudo()
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const done = checkAndInstallHelperSudoers(win)
    await new Promise((r) => setImmediate(r))
    proc.emit('error', new Error('ENOENT'))
    await done

    expect(mockedDialog.mock.calls.at(-1)?.[1].type).toBe('error')
    errSpy.mockRestore()
  })
})

describe('shipped template', () => {
  test('names the helper, not one of its jobs', async () => {
    const fs = await vi.importActual<typeof import('node:fs')>('node:fs')
    const { join } = await vi.importActual<typeof import('node:path')>('node:path')
    const rule = fs.readFileSync(
      join(process.cwd(), 'assets', 'linux', '99-LIVI-helper.sudoers.template'),
      'utf8'
    )
    expect(rule).toContain(
      'Cmnd_Alias LIVI_HELPER = /home/__USERNAME__/.config/LIVI/driver/livi-helperd'
    )
    expect(rule).toContain('__USERNAME__ ALL=(root) NOPASSWD: SETENV: LIVI_HELPER')
  })
})
