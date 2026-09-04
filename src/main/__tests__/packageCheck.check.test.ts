import { execFile, execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dialog } from 'electron'
import type { Mock } from 'vitest'
import {
  checkMissingPackages,
  missingPackages,
  pathPresent,
  readManifest
} from '../services/packageCheck'

vi.mock('node:child_process', () => ({
  execFile: vi.fn(),
  execFileSync: vi.fn(),
  spawn: vi.fn()
}))
vi.mock('node:fs', () => {
  const __m = { existsSync: vi.fn(() => false), readdirSync: vi.fn(), readFileSync: vi.fn() }
  return { ...__m, default: __m }
})
vi.mock('electron', () => ({
  app: { getAppPath: vi.fn(() => '/app') },
  dialog: { showMessageBox: vi.fn(() => Promise.resolve({ response: 0 })) }
}))

const mockedExecFile = execFile as unknown as Mock
const mockedExecFileSync = execFileSync as Mock
const mockedSpawn = spawn as Mock
const mockedExists = existsSync as Mock
const mockedReaddir = readdirSync as Mock
const mockedReadFile = readFileSync as Mock
const mockedDialog = dialog.showMessageBox as Mock

const win = {} as never
const MANIFEST_PATH = '/resources/packages.txt'

function makeProc(): EventEmitter & { unref: Mock } {
  const proc = new EventEmitter() as EventEmitter & { unref: Mock }
  proc.unref = vi.fn()
  return proc
}

function manifest(text: string, presentPaths: Set<string> = new Set()): void {
  mockedExists.mockImplementation(
    (p: string) => String(p) === MANIFEST_PATH || presentPaths.has(String(p))
  )
  mockedReadFile.mockReturnValue(text)
}

const originalPlatform = process.platform
const originalResources = process.resourcesPath
const originalXdg = process.env.XDG_CURRENT_DESKTOP

beforeEach(() => {
  mockedExecFile.mockReset()
  mockedExecFileSync.mockReset()
  mockedSpawn.mockReset()
  mockedExists.mockReset()
  mockedExists.mockReturnValue(false)
  mockedReaddir.mockReset()
  mockedReadFile.mockReset()
  mockedDialog.mockReset()
  mockedDialog.mockResolvedValue({ response: 0 })
  Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = '/resources'
  process.env.XDG_CURRENT_DESKTOP = 'GNOME'
})

afterEach(() => {
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = originalResources
  if (originalXdg === undefined) delete process.env.XDG_CURRENT_DESKTOP
  else process.env.XDG_CURRENT_DESKTOP = originalXdg
})

describe('readManifest', () => {
  test('parses the manifest next to the app resources', () => {
    manifest('core|bluez|cmd:bluetoothctl|Bluetooth|bluez')
    expect(readManifest()).toEqual([
      {
        section: 'core',
        name: 'bluez',
        probe: 'cmd:bluetoothctl',
        purpose: 'Bluetooth',
        fedora: 'bluez'
      }
    ])
  })

  test('returns [] without any manifest', () => {
    ;(process as { resourcesPath?: string }).resourcesPath = undefined
    expect(readManifest()).toEqual([])
  })

  test('returns [] when the manifest is unreadable', () => {
    mockedExists.mockImplementation((p: string) => String(p) === MANIFEST_PATH)
    mockedReadFile.mockImplementation(() => {
      throw new Error('EACCES')
    })
    expect(readManifest()).toEqual([])
  })
})

describe('pathPresent', () => {
  test('checks literal paths directly', () => {
    mockedExists.mockImplementation((p: string) => String(p) === '/usr/bin/tool')
    expect(pathPresent('/usr/bin/tool')).toBe(true)
    expect(pathPresent('/usr/bin/other')).toBe(false)
  })

  test('expands a single * into the multiarch directory', () => {
    mockedReaddir.mockReturnValue(['x86_64-linux-gnu', 'ladspa'])
    mockedExists.mockImplementation(
      (p: string) => String(p) === '/usr/lib/x86_64-linux-gnu/gstreamer-1.0/libgstopus.so'
    )
    expect(pathPresent('/usr/lib/*/gstreamer-1.0/libgstopus.so')).toBe(true)
  })

  test('finds a library under the flat lib64 layout Fedora uses', () => {
    mockedExists.mockImplementation((p: string) => String(p) === '/usr/lib64/libva.so.2')
    expect(pathPresent('/usr/lib/*/libva.so.2')).toBe(true)
  })

  test('handles a trailing * without a rest path', () => {
    mockedReaddir.mockReturnValue(['some-dir'])
    mockedExists.mockImplementation((p: string) => String(p) === '/opt/livi/some-dir')
    expect(pathPresent('/opt/livi/*')).toBe(true)
  })

  test('returns false when the base directory is unreadable', () => {
    mockedReaddir.mockImplementation(() => {
      throw new Error('ENOENT')
    })
    expect(pathPresent('/nope/*/lib.so')).toBe(false)
  })
})

describe('missingPackages', () => {
  const entry = (name: string, probe: string) => ({
    section: 'core' as const,
    name,
    probe,
    purpose: '',
    fedora: ''
  })

  test('probes cmd, py, gst, file and tolerates unknown kinds', async () => {
    const originalPath = process.env.PATH
    delete process.env.PATH
    mockedExecFile.mockImplementation(
      (cmd: string, args: string[], opts: unknown, cb?: (e: Error | null, r?: unknown) => void) => {
        const done = (typeof opts === 'function' ? opts : cb) as (
          e: Error | null,
          r?: unknown
        ) => void
        if (cmd === 'gst-inspect-1.0' && args[1] === 'missingelement') done(new Error('no element'))
        else done(null, { stdout: '', stderr: '' })
      }
    )
    try {
      const missing = await missingPackages([
        entry('cmd-ok', 'cmd:bluetoothctl'),
        entry('gst-ok', 'gst:opusenc'),
        entry('gst-missing', 'gst:missingelement'),
        entry('file-missing', 'file:/nope'),
        entry('unknown-kind', 'zz:whatever'),
        entry('no-colon', 'nocolon'),
        entry('empty-arg', 'cmd:')
      ])
      expect(missing.map((m) => m.name)).toEqual(['gst-missing', 'file-missing'])
    } finally {
      process.env.PATH = originalPath
    }
  })
})

describe('checkMissingPackages', () => {
  test('returns {} off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    await expect(checkMissingPackages(win, [])).resolves.toEqual({})
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('returns {} without required packages', async () => {
    await expect(checkMissingPackages(win, [])).resolves.toEqual({})
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('returns {} when everything is present', async () => {
    delete process.env.XDG_CURRENT_DESKTOP
    manifest('core|foo|file:/present|X\nlite|bar|file:/nope|Y', new Set(['/present']))
    mockedExists.mockImplementation((p: string) =>
      [MANIFEST_PATH, '/present', '/usr/bin/gnome-session'].includes(String(p))
    )
    await expect(checkMissingPackages(win, [])).resolves.toEqual({})
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('skips packages the user already declined', async () => {
    manifest('core|foo|file:/nope|X')
    await expect(checkMissingPackages(win, ['foo'])).resolves.toEqual({})
    expect(mockedDialog).not.toHaveBeenCalled()
  })

  test('with no supported package manager it only reports and can dismiss forever', async () => {
    manifest('core|foo|file:/nope|Feature X')
    // pkexec is present, but neither apt-get nor dnf, so there is nothing to install with.
    mockedExecFileSync.mockImplementation((_cmd: string, args: string[]) => {
      if (args[0] === 'apt-get' || args[0] === 'dnf') throw new Error(`no ${args[0]}`)
      return ''
    })
    mockedDialog.mockResolvedValueOnce({ response: 0 })

    await expect(checkMissingPackages(win, ['old'])).resolves.toEqual({
      dismissed: ['old', 'foo']
    })
    const opts = mockedDialog.mock.calls[0][1]
    expect(opts.message).toContain('1 component is missing')
    expect(opts.detail).toContain('foo — Feature X')
    expect(opts.buttons).toEqual(['Never', 'Later'])
  })

  test('without pkexec Later leaves the dismissed list untouched', async () => {
    manifest('core|foo|file:/nope|X\ncore|bar|file:/nope|')
    mockedExecFileSync.mockImplementation((_cmd: string, args: string[]) => {
      if (args[0] === 'pkexec') throw new Error('no pkexec')
      return ''
    })
    mockedDialog.mockResolvedValueOnce({ response: 1 })

    await expect(checkMissingPackages(win, [])).resolves.toEqual({})
    const opts = mockedDialog.mock.calls[0][1]
    expect(opts.message).toContain('2 components are missing')
    expect(opts.detail).toContain('  bar')
  })

  test('Never with apt available records the dismissal', async () => {
    manifest('core|foo|file:/nope|X')
    mockedExecFileSync.mockReturnValue('')
    mockedDialog.mockResolvedValueOnce({ response: 1 })

    await expect(checkMissingPackages(win, [])).resolves.toEqual({ dismissed: ['foo'] })
    expect(mockedDialog.mock.calls[0][1].buttons).toEqual(['Now', 'Never', 'Later'])
  })

  test('Later with apt available defers', async () => {
    manifest('core|foo|file:/nope|X')
    mockedExecFileSync.mockReturnValue('')
    mockedDialog.mockResolvedValueOnce({ response: 2 })

    await expect(checkMissingPackages(win, [])).resolves.toEqual({})
    expect(mockedSpawn).not.toHaveBeenCalled()
  })

  test('Now installs, verifies and reports success', async () => {
    const present = new Set<string>()
    mockedExists.mockImplementation(
      (p: string) => String(p) === MANIFEST_PATH || present.has(String(p))
    )
    mockedReadFile.mockReturnValue('core|foo|file:/nope|X')
    mockedExecFileSync.mockReturnValue('')
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)

    const done = checkMissingPackages(win, [])
    await new Promise((r) => setImmediate(r))
    present.add('/nope')
    proc.emit('close', 0)
    await expect(done).resolves.toEqual({})

    expect(mockedSpawn).toHaveBeenCalledWith(
      'pkexec',
      ['bash', '-c', 'apt-get update && apt-get install -y foo'],
      { stdio: 'ignore' }
    )
    const followUp = mockedDialog.mock.calls[1][1]
    expect(followUp.type).toBe('info')
    expect(followUp.message).toContain('All packages installed')
  })

  test('on a dnf host installs the fedora names, de-duplicated, and hints dnf', async () => {
    // Two entries map to one rpm (libva), a third has no fedora package.
    manifest(
      'core|libva2|file:/nope|A|libva\ncore|libva-drm2|file:/nope|B|libva\ncore|extra|file:/nope|C|'
    )
    // apt-get absent, dnf + pkexec present.
    mockedExecFileSync.mockImplementation((_cmd: string, args: string[]) => {
      if (args[0] === 'apt-get') throw new Error('no apt-get')
      return ''
    })
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)

    const done = checkMissingPackages(win, [])
    await new Promise((r) => setImmediate(r))
    proc.emit('close', 0)
    await expect(done).resolves.toEqual({})

    expect(mockedSpawn).toHaveBeenCalledWith('pkexec', ['bash', '-c', 'dnf install -y libva'], {
      stdio: 'ignore'
    })
    // Still missing afterwards → warn path uses the dnf manual hint.
    const followUp = mockedDialog.mock.calls[1][1]
    expect(followUp.type).toBe('warning')
    expect(followUp.detail).toContain('sudo dnf install libva')
  })

  test('Now warns when packages stay missing after the install', async () => {
    manifest('core|foo|file:/nope|X')
    mockedExecFileSync.mockReturnValue('')
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)

    const done = checkMissingPackages(win, [])
    await new Promise((r) => setImmediate(r))
    proc.emit('close', 0)
    await expect(done).resolves.toEqual({})

    const followUp = mockedDialog.mock.calls[1][1]
    expect(followUp.type).toBe('warning')
    expect(followUp.detail).toContain('sudo apt install foo')
  })

  test('Now surfaces a failing pkexec run', async () => {
    manifest('core|foo|file:/nope|X')
    mockedExecFileSync.mockReturnValue('')
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const done = checkMissingPackages(win, [])
    await new Promise((r) => setImmediate(r))
    proc.emit('close', 127)
    await expect(done).resolves.toEqual({})

    expect(errSpy).toHaveBeenCalledWith('[packageCheck] installation failed:', expect.any(Error))
    const followUp = mockedDialog.mock.calls[1][1]
    expect(followUp.type).toBe('error')
    expect(followUp.detail).toContain('pkexec exited with code 127')
    errSpy.mockRestore()
  })

  test('Now surfaces a pkexec spawn error', async () => {
    manifest('core|foo|file:/nope|X')
    mockedExecFileSync.mockReturnValue('')
    const proc = makeProc()
    mockedSpawn.mockReturnValue(proc)
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const done = checkMissingPackages(win, [])
    await new Promise((r) => setImmediate(r))
    proc.emit('error', new Error('ENOENT'))
    await expect(done).resolves.toEqual({})

    expect(mockedDialog.mock.calls[1][1].type).toBe('error')
    errSpy.mockRestore()
  })
})
