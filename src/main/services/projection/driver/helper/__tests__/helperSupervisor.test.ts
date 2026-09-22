import { execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  statSync
} from 'node:fs'
import type { Mock } from 'vitest'

vi.mock('node:child_process', () => ({ spawn: vi.fn(), execFileSync: vi.fn(() => '') }))
vi.mock('node:fs', () => {
  const __m = {
    chmodSync: vi.fn(),
    copyFileSync: vi.fn(),
    existsSync: vi.fn(() => false),
    mkdirSync: vi.fn(),
    readFileSync: vi.fn(() => Buffer.from('x')),
    renameSync: vi.fn(),
    statSync: vi.fn(() => ({ size: 0 }))
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
    pkHex: 'aabbcc'
  }))
}))

const mockedSpawn = spawn as Mock
const mockedExists = existsSync as Mock
const mockedStat = statSync as Mock
const mockedRead = readFileSync as Mock
const mockedMkdir = mkdirSync as Mock
const mockedCopy = copyFileSync as Mock
const mockedRename = renameSync as Mock
const mockedChmod = chmodSync as Mock

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

/** Models the paths that exist, so a staged copy is visible to the next check. */
function fakeFs(present: string[], sizes: Record<string, number> = {}): void {
  const files = new Set(present)
  mockedExists.mockImplementation((p: string) => files.has(String(p)))
  mockedStat.mockImplementation((p: string) => ({ size: sizes[String(p)] ?? 4096 }))
  mockedRead.mockImplementation((p: string) => Buffer.from(`c${sizes[String(p)] ?? 4096}`))
  mockedCopy.mockImplementation((_src: string, dest: string) => {
    files.add(String(dest))
  })
  mockedRename.mockImplementation((src: string, dest: string) => {
    files.delete(String(src))
    files.add(String(dest))
  })
}

function devBinOnly(): void {
  mockedExists.mockImplementation(
    (p: string) => !String(p).startsWith('/res') && String(p).endsWith('/livi-helperd')
  )
}

const CONFIG = { wirelessAaEnabled: true, wirelessCpEnabled: false } as never

const originalPlatform = process.platform
const originalResources = process.resourcesPath
const originalAppImage = process.env.APPIMAGE
const originalAppDir = process.env.APPDIR

beforeEach(() => {
  mockedSpawn.mockReset()
  mockedSpawn.mockImplementation(() => makeChild())
  mockedExists.mockReset()
  mockedExists.mockReturnValue(false)
  mockedStat.mockReset()
  mockedStat.mockReturnValue({ size: 0 })
  mockedRead.mockReset()
  mockedRead.mockReturnValue(Buffer.from('x'))
  mockedMkdir.mockReset()
  mockedCopy.mockReset()
  mockedChmod.mockReset()
  delete process.env.LIVI_HELPER_BIN
  Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = undefined
  delete process.env.APPIMAGE
  delete process.env.APPDIR
})

afterEach(() => {
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  ;(process as { resourcesPath?: string }).resourcesPath = originalResources
  if (originalAppImage === undefined) delete process.env.APPIMAGE
  else process.env.APPIMAGE = originalAppImage
  if (originalAppDir === undefined) delete process.env.APPDIR
  else process.env.APPDIR = originalAppDir
  vi.useRealTimers()
})

describe('spawning', () => {
  test('emits an error when the helper binary is missing', async () => {
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor()
    const onError = vi.fn()
    sup.on('error', onError)
    sup.start(CONFIG)
    expect(onError).toHaveBeenCalledWith(expect.any(Error))
    expect(mockedSpawn).not.toHaveBeenCalled()
    expect(sup.running).toBe(false)
  })

  test('kills stale helpers (python-era and rust) before spawning, sparing the AP unit', async () => {
    devBinOnly()
    const mockedExec = execFileSync as Mock
    mockedExec.mockReturnValue('1234\n')
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor()
    sup.start(CONFIG)
    for (const pattern of ['livi-helper\\.py', 'driver/livi-helperd$']) {
      expect(mockedExec).toHaveBeenCalledWith('pgrep', ['-f', pattern], expect.anything())
      expect(mockedExec).toHaveBeenCalledWith('sudo', ['-n', 'pkill', '-f', pattern], {
        stdio: 'ignore'
      })
    }
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('stale helper'))
    warn.mockRestore()
    // execFileSync is module-mocked and not reset per test; restore its default.
    mockedExec.mockReset()
    mockedExec.mockImplementation(() => '')
  })

  test('spawns via sudo on linux with the wireless env from the config', async () => {
    devBinOnly()
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor()
    sup.start(CONFIG)

    const [cmd, args, opts] = mockedSpawn.mock.calls[0]
    expect(cmd).toBe('sudo')
    expect(args.slice(0, 2)).toEqual(['-n', '-E'])
    expect(String(args[2])).toContain('livi-helperd')
    expect(opts.env.LIVI_AA_WIRELESS).toBe('1')
    expect(opts.env.LIVI_CP_WIRELESS).toBe('')
    expect(opts.env.LIVI_CP_PK).toBe('aabbcc')
    expect(opts.env.LIVI_CP_PI).toBe('pi-123')
    expect(opts.env.DEBUG).toBe('')
    expect(sup.running).toBe(true)
  })

  test('runs the binary directly off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    devBinOnly()
    const { HelperSupervisor } = await load(false)
    const sup = new HelperSupervisor()
    sup.start({ wirelessAaEnabled: false, wirelessCpEnabled: true } as never)

    const [cmd, args, opts] = mockedSpawn.mock.calls[0]
    expect(String(cmd)).toContain('livi-helperd')
    expect(args).toEqual([])
    expect(opts.env.LIVI_AA_WIRELESS).toBe('')
    expect(opts.env.LIVI_CP_WIRELESS).toBe('1')
  })

  test('logs the spawn line and flags in debug builds', async () => {
    devBinOnly()
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
    const { HelperSupervisor } = await load(true)
    new HelperSupervisor().start(CONFIG)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('[helper] spawning sudo'))
    expect(mockedSpawn.mock.calls[0][2].env.DEBUG).toBe('1')
    logSpy.mockRestore()
  })

  test('the debug spawn line shows 0 for a disabled aa link', async () => {
    devBinOnly()
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

  test('stages the binary out of an AppImage mount so root can exec it, and reuses it', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/tmp/.mount_livi/res'
    process.env.APPIMAGE = '/apps/livi.AppImage'
    process.env.APPDIR = '/tmp/.mount_livi'
    fakeFs(['/tmp/.mount_livi/res/driver/livi-helperd'])

    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)

    expect(mockedMkdir).toHaveBeenCalledWith('/data/driver', { recursive: true })
    expect(mockedCopy).toHaveBeenCalledWith(
      '/tmp/.mount_livi/res/driver/livi-helperd',
      '/data/driver/livi-helperd.new'
    )
    expect(mockedChmod).toHaveBeenCalledWith('/data/driver/livi-helperd.new', 0o755)
    expect(mockedRename).toHaveBeenCalledWith(
      '/data/driver/livi-helperd.new',
      '/data/driver/livi-helperd'
    )
    expect(String(mockedSpawn.mock.calls[0][1][2])).toBe('/data/driver/livi-helperd')
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/data/driver')

    // A staged copy with identical content is reused instead of copied again.
    mockedCopy.mockClear()
    new HelperSupervisor().start(CONFIG)
    expect(mockedCopy).not.toHaveBeenCalled()
  })

  test('re-stages when the staged copy has different content', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/x/.mount_abc/res'
    fakeFs(['/x/.mount_abc/res/driver/livi-helperd', '/data/driver/livi-helperd'], {
      '/x/.mount_abc/res/driver/livi-helperd': 4096,
      '/data/driver/livi-helperd': 128
    })

    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)

    expect(mockedCopy).toHaveBeenCalledWith(
      '/x/.mount_abc/res/driver/livi-helperd',
      '/data/driver/livi-helperd.new'
    )
    expect(mockedRename).toHaveBeenCalled()
  })

  test('when staging throws, a previously staged copy wins over the mount path', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    mockedExists.mockImplementation((p: string) =>
      ['/y/.mount_z/res/driver/livi-helperd', '/data/driver/livi-helperd'].includes(String(p))
    )
    mockedRead.mockImplementation(() => {
      throw new Error('unreadable')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const { HelperSupervisor } = await load(true)
    new HelperSupervisor().start(CONFIG)

    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('staging failed'))
    expect(String(mockedSpawn.mock.calls[0][1][2])).toBe('/data/driver/livi-helperd')
    warnSpy.mockRestore()
  })

  test('falls back to the mount path only when nothing was ever staged', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    mockedExists.mockImplementation(
      (p: string) => String(p) === '/y/.mount_z/res/driver/livi-helperd'
    )
    mockedCopy.mockImplementation(() => {
      throw new Error('read-only fs')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const { HelperSupervisor } = await load(true)
    new HelperSupervisor().start(CONFIG)

    expect(String(mockedSpawn.mock.calls[0][1][2])).toBe('/y/.mount_z/res/driver/livi-helperd')
    warnSpy.mockRestore()
  })

  test('an explicit LIVI_HELPER_BIN wins over the packaged copy', async () => {
    process.env.LIVI_HELPER_BIN = '/opt/dev/livi-helperd'
    ;(process as { resourcesPath?: string }).resourcesPath = '/res'
    mockedExists.mockReturnValue(true)

    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)

    expect(String(mockedSpawn.mock.calls[0][1][2])).toBe('/opt/dev/livi-helperd')
    expect(mockedCopy).not.toHaveBeenCalled()
  })

  test('a mount path outside APPDIR still counts via the .mount_ marker', async () => {
    process.env.APPIMAGE = '/apps/livi.AppImage'
    process.env.APPDIR = '/somewhere/else'
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    fakeFs(['/y/.mount_z/res/driver/livi-helperd'])
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(mockedCopy).toHaveBeenCalled()
    expect(mockedSpawn.mock.calls[0][2].cwd).toBe('/data/driver')
  })

  test('with APPIMAGE set and no APPDIR every resources path counts as mounted', async () => {
    process.env.APPIMAGE = '/apps/livi.AppImage'
    ;(process as { resourcesPath?: string }).resourcesPath = '/plain/res'
    fakeFs(['/plain/res/driver/livi-helperd'])
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(mockedCopy).toHaveBeenCalledWith(
      '/plain/res/driver/livi-helperd',
      '/data/driver/livi-helperd.new'
    )
  })

  test('staging failures warn even outside debug builds', async () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/y/.mount_z/res'
    mockedExists.mockImplementation((p: string) =>
      ['/y/.mount_z/res/driver/livi-helperd', '/data/driver/livi-helperd'].includes(String(p))
    )
    mockedRead.mockImplementation(() => {
      throw new Error('unreadable')
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { HelperSupervisor } = await load(false)
    new HelperSupervisor().start(CONFIG)
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('staging failed'))
    expect(String(mockedSpawn.mock.calls[0][1][2])).toBe('/data/driver/livi-helperd')
    warnSpy.mockRestore()
  })
})

describe('io and lifecycle', () => {
  async function started(debug = false): Promise<{
    sup: InstanceType<SupervisorModule['HelperSupervisor']>
    child: FakeChild
  }> {
    devBinOnly()
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
    devBinOnly()
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
