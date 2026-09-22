import { execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import type { Mock } from 'vitest'
import {
  asset,
  helperInstalls,
  markerHolds,
  pkexecAvailable,
  runAsRoot,
  stamp,
  sudoersLines,
  sudoGrants,
  username,
  writeMarker
} from '../privileged'

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
vi.mock('../projection/driver/helper/helperSupervisor', () => ({
  resolveHelperBin: () => '/data/driver/livi-helperd'
}))
vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/data'), getAppPath: vi.fn(() => '/app') }
}))

const mockedExec = execFileSync as Mock
const mockedSpawn = spawn as Mock
const mockedExists = existsSync as Mock
const mockedRead = readFileSync as Mock
const mockedWrite = writeFileSync as Mock

const originalResources = process.resourcesPath

beforeEach(() => {
  vi.clearAllMocks()
  mockedExists.mockReturnValue(false)
  ;(process as { resourcesPath?: string }).resourcesPath = undefined
  delete process.env.PKEXEC_UID
  delete process.env.SUDO_USER
})

afterEach(() => {
  ;(process as { resourcesPath?: string }).resourcesPath = originalResources
})

describe('asset', () => {
  test('prefers the copy packaged next to the app', () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/res'
    mockedExists.mockImplementation((p: string) => String(p) === '/res/x.template')
    mockedRead.mockReturnValue('packaged')
    expect(asset('x.template')).toBe('packaged')
    expect(mockedRead).toHaveBeenCalledWith('/res/x.template', 'utf8')
  })

  test('falls back to the repository copy', () => {
    ;(process as { resourcesPath?: string }).resourcesPath = '/res'
    mockedRead.mockReturnValue('repo')
    expect(asset('x.template')).toBe('repo')
    expect(mockedRead).toHaveBeenCalledWith('/app/assets/linux/x.template', 'utf8')
  })
})

describe('username', () => {
  test('the user behind pkexec, then sudo, then the process', () => {
    process.env.PKEXEC_UID = '1000'
    mockedExec.mockReturnValue('pkexec-user\n')
    expect(username()).toBe('pkexec-user')
    expect(mockedExec).toHaveBeenCalledWith('id', ['-nu', '1000'], { encoding: 'utf8' })

    mockedExec.mockImplementation(() => {
      throw new Error('no id')
    })
    process.env.SUDO_USER = 'sudo-user'
    expect(username()).toBe('sudo-user')

    delete process.env.PKEXEC_UID
    delete process.env.SUDO_USER
    expect(username()).toBe('driver')
  })
})

describe('pkexecAvailable', () => {
  test('follows which', () => {
    mockedExec.mockReturnValue('')
    expect(pkexecAvailable()).toBe(true)
    mockedExec.mockImplementation(() => {
      throw new Error('not found')
    })
    expect(pkexecAvailable()).toBe(false)
  })
})

describe('sudoGrants', () => {
  test('a rule carrying the needle, or everything', () => {
    mockedExec.mockReturnValue('(root) NOPASSWD: /usr/local/lib/livi/x.sh disable\n')
    expect(sudoGrants('/usr/local/lib/livi/x.sh')).toBe(true)
    expect(sudoGrants('/something/else')).toBe(false)
    mockedExec.mockReturnValue('(ALL : ALL) NOPASSWD: ALL\n')
    expect(sudoGrants('/something/else')).toBe(true)
  })

  test('false when sudo cannot answer', () => {
    mockedExec.mockImplementation(() => {
      throw new Error('a password is required')
    })
    expect(sudoGrants('x')).toBe(false)
  })
})

describe('runAsRoot', () => {
  test('resolves on exit 0 and rejects otherwise, set -e in front', async () => {
    const proc = new EventEmitter()
    mockedSpawn.mockReturnValue(proc)
    const done = runAsRoot(['echo hi'])
    expect(mockedSpawn).toHaveBeenCalledWith('pkexec', ['bash', '-c', 'set -e\necho hi'], {
      stdio: 'ignore'
    })
    proc.emit('close', 0)
    await expect(done).resolves.toBeUndefined()

    const failing = new EventEmitter()
    mockedSpawn.mockReturnValue(failing)
    const fail = runAsRoot(['false'])
    failing.emit('close', 127)
    await expect(fail).rejects.toThrow('pkexec exited with code 127')
  })

  test('rejects when pkexec cannot be spawned', async () => {
    const proc = new EventEmitter()
    mockedSpawn.mockReturnValue(proc)
    const done = runAsRoot(['x'])
    proc.emit('error', new Error('ENOENT'))
    await expect(done).rejects.toThrow('ENOENT')
  })
})

describe('sudoersLines', () => {
  test('stages, validates, then moves', () => {
    const lines = sudoersLines('/etc/sudoers.d/99-x', 'a ALL=(root) NOPASSWD: /x\n')
    expect(lines).toEqual([
      "trap 'rm -f /etc/sudoers.d/99-x.livi-tmp' EXIT",
      "cat > /etc/sudoers.d/99-x.livi-tmp <<'EOF'",
      'a ALL=(root) NOPASSWD: /x',
      'EOF',
      'chmod 0440 /etc/sudoers.d/99-x.livi-tmp',
      'chown root:root /etc/sudoers.d/99-x.livi-tmp',
      'visudo -c -f /etc/sudoers.d/99-x.livi-tmp',
      'mv /etc/sudoers.d/99-x.livi-tmp /etc/sudoers.d/99-x'
    ])
  })
})

describe('marker', () => {
  test('holds the stamp of what was installed', () => {
    writeMarker('x.installed', 'content')
    expect(mockedWrite).toHaveBeenCalledWith('/data/x.installed', stamp('content'), {
      mode: 0o644
    })
    mockedRead.mockReturnValue(`${stamp('content')}\n`)
    expect(markerHolds('x.installed', 'content')).toBe(true)
    expect(markerHolds('x.installed', 'changed')).toBe(false)
  })

  test('a missing marker holds nothing', () => {
    mockedRead.mockImplementation(() => {
      throw new Error('ENOENT')
    })
    expect(markerHolds('x.installed', 'content')).toBe(false)
  })

  test('the stamp is short and stable', () => {
    expect(stamp('a')).toMatch(/^[0-9a-f]{16}$/)
    expect(stamp('a')).toBe(stamp('a'))
    expect(stamp('a')).not.toBe(stamp('b'))
  })
})

describe('helperInstalls', () => {
  test('writes the files in order, hands them to the helper as root, cleans up', async () => {
    const { rmSync } = await import('node:fs')
    mockedExec.mockReturnValue('')
    expect(helperInstalls('install-x', { first: 'a', second: 'b' })).toBe(true)
    expect(mockedWrite).toHaveBeenNthCalledWith(1, '/tmp/livi-install-x/first', 'a')
    expect(mockedWrite).toHaveBeenNthCalledWith(2, '/tmp/livi-install-x/second', 'b')
    expect(mockedExec).toHaveBeenCalledWith(
      'sudo',
      [
        '-n',
        '/data/driver/livi-helperd',
        '--install-x',
        '/tmp/livi-install-x/first',
        '/tmp/livi-install-x/second'
      ],
      { stdio: 'ignore', timeout: 20_000 }
    )
    expect(rmSync).toHaveBeenCalledWith('/tmp/livi-install-x', { recursive: true, force: true })
  })

  test('false when sudo refuses, the directory still goes', async () => {
    const { rmSync } = await import('node:fs')
    mockedExec.mockImplementation(() => {
      throw new Error('a password is required')
    })
    expect(helperInstalls('install-x', { f: 'a' })).toBe(false)
    expect(rmSync).toHaveBeenCalled()
  })

  test('false when no temp directory can be made', async () => {
    const { mkdtempSync } = await import('node:fs')
    ;(mkdtempSync as Mock).mockImplementationOnce(() => {
      throw new Error('EROFS')
    })
    expect(helperInstalls('install-x', { f: 'a' })).toBe(false)
  })
})
