import { spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync
} from 'node:fs'
import type { Mock } from 'vitest'

vi.mock('node:child_process', () => ({ spawn: vi.fn() }))
vi.mock('node:fs', () => {
  const __m = {
    cpSync: vi.fn(),
    existsSync: vi.fn(() => false),
    mkdirSync: vi.fn(),
    readdirSync: vi.fn(() => []),
    readFileSync: vi.fn(() => Buffer.alloc(0)),
    rmSync: vi.fn(),
    statSync: vi.fn(),
    writeFileSync: vi.fn()
  }
  return { ...__m, default: __m }
})
vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/data') }
}))
vi.mock('../../cp/stack/identity', () => ({
  loadOrCreateIdentity: vi.fn(() => ({
    privRaw: Buffer.alloc(32),
    pubRaw: Buffer.alloc(32),
    pairingId: 'pi-123',
    pkHex: 'a'.repeat(64)
  }))
}))

const mockedSpawn = spawn as Mock
const mockedExists = existsSync as Mock
const mockedReaddir = readdirSync as Mock
const mockedReadFile = readFileSync as Mock
const mockedStat = statSync as Mock
const mockedRm = rmSync as Mock
const mockedMkdir = mkdirSync as Mock
const mockedCp = cpSync as Mock
const mockedWrite = writeFileSync as Mock

type SupervisorModule = typeof import('../helperSupervisor')

async function load(debug: boolean): Promise<SupervisorModule> {
  vi.resetModules()
  vi.doMock('@main/constants', () => ({ DEBUG: debug }))
  return await import('../helperSupervisor')
}

type FakeChild = EventEmitter & {
  stdout: EventEmitter & { setEncoding: Mock }
  stderr: EventEmitter & { setEncoding: Mock }
  kill: Mock
  killed: boolean
  exitCode: number | null
}

function makeChild(): FakeChild {
  const child = new EventEmitter() as FakeChild
  child.stdout = Object.assign(new EventEmitter(), { setEncoding: vi.fn() }) as FakeChild['stdout']
  child.stderr = Object.assign(new EventEmitter(), { setEncoding: vi.fn() }) as FakeChild['stderr']
  child.kill = vi.fn()
  child.killed = false
  child.exitCode = null
  return child
}

function devScriptOnly(): void {
  mockedExists.mockImplementation(
    (p: string) => !String(p).startsWith('/res') && String(p).endsWith('/livi-helper.py')
  )
}

const CONFIG = { wirelessAaEnabled: true, wirelessCpEnabled: false } as never

const originalPlatform = process.platform
const originalResources = process.resourcesPath
const originalAppImage = process.env.APPIMAGE
const originalAppDir = process.env.APPDIR
const originalLiviOs = process.env.LIVI_OS

beforeEach(() => {
  mockedSpawn.mockReset()
  mockedSpawn.mockImplementation(() => makeChild())
  mockedExists.mockReset()
  mockedExists.mockReturnValue(false)
  mockedReaddir.mockReset()
  mockedReadFile.mockReset()
  mockedStat.mockReset()
  mockedRm.mockReset()
  mockedMkdir.mockReset()
  mockedCp.mockReset()
  mockedWrite.mockReset()
  Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = undefined
  delete process.env.APPIMAGE
  delete process.env.APPDIR
  delete process.env.LIVI_OS
})

afterEach(() => {
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = originalResources
  if (originalAppImage === undefined) delete process.env.APPIMAGE
  else process.env.APPIMAGE = originalAppImage
  if (originalAppDir === undefined) delete process.env.APPDIR
  else process.env.APPDIR = originalAppDir
  if (originalLiviOs === undefined) delete process.env.LIVI_OS
  else process.env.LIVI_OS = originalLiviOs
  vi.useRealTimers()
})

describe('spawning', () => {
  test('emits an error when the helper script is missing', async () => {
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor()
    const onError = vi.fn()
    sup.on('error', onError)
    sup.start(CONFIG)
    expect(onError).toHaveBeenCalledWith(expect.any(Error))
    expect(mockedSpawn).not.toHaveBeenCalled()
    expect(sup.running).toBe(false)
  })

  test('spawns via sudo on linux with the wireless env from the config', async () => {
    devScriptOnly()
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor({ python: '/opt/py3' })
    sup.start(CONFIG)

    const [cmd, args, opts] = mockedSpawn.mock.calls[0]
    expect(cmd).toBe('sudo')
    expect(args.slice(0, 3)).toEqual(['-n', '-E', '/opt/py3'])
    expect(args[3]).toBe('-u')
    expect(String(args[4])).toContain('livi-helper.py')
    expect(opts.env.LIVI_AA_WIRELESS).toBe('1')
    expect(opts.env.LIVI_CP_WIRELESS).toBe('')
    expect(opts.env.LIVI_CP_PK).toBe('a'.repeat(64))
    expect(opts.env.LIVI_CP_PI).toBe('pi-123')
    expect(opts.env.DEBUG).toBe('')
    expect(sup.running).toBe(true)
  })

  test('uses the immutable OS helper without inherited environment or script elevation', async () => {
    process.env.LIVI_OS = '1'
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)

    const [cmd, args, opts] = mockedSpawn.mock.calls[0]
    expect(cmd).toBe('/usr/bin/sudo')
    expect(args).toEqual([
      '-n',
      '/usr/libexec/livi-projection-helper',
      '1',
      '0',
      'a'.repeat(64),
      'pi-123',
      '0'
    ])
    expect(opts.cwd).toBe('/usr/lib/livi/driver')
    expect(opts.env).toEqual({ PATH: '/usr/sbin:/usr/bin:/sbin:/bin' })
    expect(mockedExists).not.toHaveBeenCalled()
  })

  test('runs python directly off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    devScriptOnly()
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor()
    sup.start({ wirelessAaEnabled: false, wirelessCpEnabled: true } as never)

    const [cmd, args, opts] = mockedSpawn.mock.calls[0]
    expect(cmd).toBe('python3')
    expect(args[0]).toBe('-u')
    expect(opts.env.LIVI_AA_WIRELESS).toBe('')
    expect(opts.env.LIVI_CP_WIRELESS).toBe('1')
  })

  test('logs the spawn line and flags in debug builds', async () => {
    devScriptOnly()
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
    const { HelperSupervisor } = await load(true)
    new HelperSupervisor().start(CONFIG)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('[helper] spawning sudo'))
    expect(mockedSpawn.mock.calls[0][2].env.DEBUG).toBe('1')
    logSpy.mockRestore()
  })

  test('the debug spawn line shows 0 for a disabled aa link', async () => {
    devScriptOnly()
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
    const { HelperSupervisor } = await load(true)
    new HelperSupervisor().start({ wirelessAaEnabled: false, wirelessCpEnabled: true } as never)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('aa=0, cpWireless=1'))
    logSpy.mockRestore()
  })

  test('a bare _spawn without a config is a no-op', async () => {
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor()
    ;(sup as unknown as { _spawn: () => void })._spawn()
    expect(mockedSpawn).not.toHaveBeenCalled()
  })
})

describe('helper root resolution', () => {
  test('prefers the packaged resources outside an AppImage mount', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/res'
    mockedExists.mockImplementation((p: string) => String(p).startsWith('/res'))
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/res/driver')
  })

  test('stages the helper out of an AppImage mount and reuses a valid stage', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/tmp/.mount_livi/res'
    process.env.APPIMAGE = '/apps/livi.AppImage'
    process.env.APPDIR = '/tmp/.mount_livi'
    mockedExists.mockImplementation((p: string) => {
      const s = String(p)
      return (
        s === '/tmp/.mount_livi/res/driver/helper/livi-helper.py' ||
        s === '/data/driver/unified/helper/livi-helper.py'
      )
    })
    mockedReaddir.mockImplementation((root: string) =>
      String(root).endsWith('/helper') ? ['livi-helper.py'] : ['helper']
    )
    mockedStat.mockImplementation((p: string) => ({
      isDirectory: () => String(p).endsWith('/helper'),
      isFile: () => !String(p).endsWith('/helper'),
      size: 10,
      mtimeMs: 1
    }))
    mockedReadFile.mockReturnValue(Buffer.from('print("hi")'))

    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)

    expect(mockedRm).toHaveBeenCalledWith('/data/driver/unified', { recursive: true, force: true })
    expect(mockedCp).toHaveBeenCalledWith('/tmp/.mount_livi/res/driver', '/data/driver/unified', {
      recursive: true,
      force: true
    })
    const [sigPath, sigContent, sigOpts] = mockedWrite.mock.calls[0]
    expect(sigPath).toBe('/data/driver/unified/.livi-staged-sig')
    expect(sigOpts).toEqual({ mode: 0o644 })
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/data/driver/unified')

    const sig = String(sigContent).trim()
    mockedExists.mockImplementation((p: string) => {
      const s = String(p)
      return (
        s === '/tmp/.mount_livi/res/driver/helper/livi-helper.py' ||
        s === '/data/driver/unified/helper/livi-helper.py' ||
        s === '/data/driver/unified/.livi-staged-sig'
      )
    })
    mockedReadFile.mockImplementation((p: string, enc?: string) =>
      enc === 'utf8' && String(p).endsWith('.livi-staged-sig')
        ? `${sig}\n`
        : Buffer.from('print("hi")')
    )
    mockedCp.mockClear()
    new HelperSupervisor().start(CONFIG)
    expect(mockedCp).not.toHaveBeenCalled()
  })

  test('re-stages when the signature is unreadable and warns when cleanup fails', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/x/.mount_abc/res'
    mockedExists.mockImplementation((p: string) => {
      const s = String(p)
      return (
        s === '/x/.mount_abc/res/driver/helper/livi-helper.py' ||
        s === '/data/driver/unified/helper/livi-helper.py' ||
        s === '/data/driver/unified/.livi-staged-sig'
      )
    })
    mockedReaddir.mockReturnValue([])
    mockedReadFile.mockImplementation((p: string, enc?: string) => {
      if (enc === 'utf8') throw new Error('corrupt')
      return Buffer.alloc(0)
    })
    mockedRm.mockImplementation(() => {
      throw new Error('busy')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})

    const { HelperSupervisor } = await load(true)
    new HelperSupervisor().start(CONFIG)

    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('could not clear'))
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('staged driver/'))
    expect(mockedCp).toHaveBeenCalled()
    warnSpy.mockRestore()
    logSpy.mockRestore()
  })

  test('falls back to the mount path when staging throws', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    mockedExists.mockImplementation(
      (p: string) => String(p) === '/y/.mount_z/res/driver/helper/livi-helper.py'
    )
    mockedReaddir.mockImplementation(() => {
      throw new Error('unreadable tree')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const { HelperSupervisor } = await load(true)
    new HelperSupervisor().start(CONFIG)

    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('staging failed'))
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/y/.mount_z/res/driver')
    warnSpy.mockRestore()
  })

  test('a mount path outside APPDIR still counts via the .mount_ marker', async () => {
    process.env.APPIMAGE = '/apps/livi.AppImage'
    process.env.APPDIR = '/somewhere/else'
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    mockedExists.mockImplementation(
      (p: string) => String(p) === '/y/.mount_z/res/driver/helper/livi-helper.py'
    )
    mockedReaddir.mockImplementation(() => {
      throw new Error('unreadable tree')
    })
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/y/.mount_z/res/driver')
  })

  test('with APPIMAGE set and no APPDIR every resources path counts as mounted', async () => {
    process.env.APPIMAGE = '/apps/livi.AppImage'
    ;(process as { resourcesPath?: string }).resourcesPath = '/plain/res'
    mockedExists.mockImplementation(
      (p: string) => String(p) === '/plain/res/driver/helper/livi-helper.py'
    )
    mockedReaddir.mockImplementation(() => {
      throw new Error('unreadable tree')
    })
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/plain/res/driver')
  })

  test('walkTree skips entries that are neither file nor directory', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    mockedExists.mockImplementation((p: string) => {
      const s = String(p)
      return (
        s === '/y/.mount_z/res/driver/helper/livi-helper.py' ||
        s === '/data/driver/unified/helper/livi-helper.py'
      )
    })
    mockedReaddir.mockImplementation((root: string) =>
      String(root).endsWith('/driver') ? ['weird.sock'] : []
    )
    mockedStat.mockImplementation(() => ({
      isDirectory: () => false,
      isFile: () => false,
      size: 0,
      mtimeMs: 0
    }))
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(mockedCp).toHaveBeenCalled()
  })

  test('quietly falls back when staging throws outside debug builds', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    mockedExists.mockImplementation(
      (p: string) => String(p) === '/y/.mount_z/res/driver/helper/livi-helper.py'
    )
    mockedReaddir.mockImplementation(() => {
      throw new Error('unreadable tree')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(warnSpy).not.toHaveBeenCalled()
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/y/.mount_z/res/driver')
    warnSpy.mockRestore()
  })
})

describe('io and lifecycle', () => {
  async function started(debug = false): Promise<{
    sup: InstanceType<SupervisorModule['HelperSupervisor']>
    child: FakeChild
  }> {
    devScriptOnly()
    const { HelperSupervisor } = await load(debug)
    const child = makeChild()
    mockedSpawn.mockReturnValue(child)
    const sup = new HelperSupervisor({ restartDelayMs: 100 })
    sup.start(CONFIG)
    return { sup, child }
  }

  test('splits stdout and stderr into trimmed lines', async () => {
    const { sup, child } = await started()
    const out: string[] = []
    const err: string[] = []
    sup.on('stdout', (l) => out.push(l))
    sup.on('stderr', (l) => err.push(l))

    child.stdout.emit('data', 'hello ')
    child.stdout.emit('data', 'world\r\npartial')
    child.stdout.emit('data', '\n\n')
    child.stderr.emit('data', 'oops\n\nmore\n')

    expect(out).toEqual(['hello world', 'partial'])
    expect(err).toEqual(['oops', 'more'])
  })

  test('forwards child errors', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { sup, child } = await started(true)
    const onError = vi.fn()
    sup.on('error', onError)
    child.emit('error', new Error('EACCES'))
    expect(onError).toHaveBeenCalledWith(expect.any(Error))
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('child error'))
    warnSpy.mockRestore()
  })

  test('forwards child errors quietly outside debug builds', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { sup, child } = await started(false)
    const onError = vi.fn()
    sup.on('error', onError)
    child.emit('error', new Error('EACCES'))
    expect(onError).toHaveBeenCalledWith(expect.any(Error))
    expect(warnSpy).not.toHaveBeenCalled()
    warnSpy.mockRestore()
  })

  test('restarts after an unexpected exit', async () => {
    vi.useFakeTimers()
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
    const { sup, child } = await started(true)
    const onExit = vi.fn()
    sup.on('exit', onExit)

    const second = makeChild()
    mockedSpawn.mockReturnValue(second)
    child.emit('exit', 1, null)
    expect(onExit).toHaveBeenCalledWith(1, null)
    expect(sup.running).toBe(false)

    vi.advanceTimersByTime(150)
    expect(mockedSpawn).toHaveBeenCalledTimes(2)
    expect(sup.running).toBe(true)
    logSpy.mockRestore()
  })

  test('gives up after maxRestarts', async () => {
    vi.useFakeTimers()
    devScriptOnly()
    const { HelperSupervisor } = await load(false)
    const first = makeChild()
    mockedSpawn.mockReturnValue(first)
    const sup = new HelperSupervisor({ restartDelayMs: 100, maxRestarts: 1 })
    const onError = vi.fn()
    sup.on('error', onError)
    sup.start(CONFIG)

    const second = makeChild()
    mockedSpawn.mockReturnValue(second)
    first.emit('exit', 1, null)
    vi.advanceTimersByTime(150)
    expect(mockedSpawn).toHaveBeenCalledTimes(2)

    second.emit('exit', 1, null)
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: expect.stringContaining('exceeded max restarts') })
    )
    vi.advanceTimersByTime(1000)
    expect(mockedSpawn).toHaveBeenCalledTimes(2)
  })

  test('stop terminates the child and cancels a pending restart', async () => {
    vi.useFakeTimers()
    const { sup, child } = await started()

    const stopping = sup.stop()
    expect(child.kill).toHaveBeenCalledWith('SIGTERM')
    child.exitCode = 0
    child.emit('exit', 0, null)
    await stopping
    expect(sup.running).toBe(false)

    vi.advanceTimersByTime(1000)
    expect(mockedSpawn).toHaveBeenCalledTimes(1)
  })

  test('stop escalates to SIGKILL when the child ignores SIGTERM', async () => {
    vi.useFakeTimers()
    const { sup, child } = await started()

    const stopping = sup.stop()
    vi.advanceTimersByTime(3100)
    expect(child.kill).toHaveBeenLastCalledWith('SIGKILL')
    child.emit('exit', null, 'SIGKILL')
    await stopping
  })

  test('stop skips the SIGKILL when the child died in time', async () => {
    vi.useFakeTimers()
    const { sup, child } = await started()
    const stopping = sup.stop()
    child.exitCode = 0
    child.emit('exit', 0, null)
    await stopping
    vi.advanceTimersByTime(3100)
    expect(child.kill).toHaveBeenCalledTimes(1)
  })

  test('stop cancels a scheduled restart', async () => {
    vi.useFakeTimers()
    const { sup, child } = await started()
    child.emit('exit', 1, null)
    await sup.stop()
    vi.advanceTimersByTime(1000)
    expect(mockedSpawn).toHaveBeenCalledTimes(1)
  })

  test('stop without a running child resolves immediately', async () => {
    const { HelperSupervisor } = await load(false)
    await expect(new HelperSupervisor().stop()).resolves.toBeUndefined()
  })
})
