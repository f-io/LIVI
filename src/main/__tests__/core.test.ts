import { EventEmitter } from 'node:events'
import type { FromCore, State, ToCore } from '@shared/core/contract'
import type { LinkOptions } from '@shared/core/link'
import { app } from 'electron'

const links: Array<{ opts: LinkOptions; sent: ToCore[]; connected: boolean }> = []

const spawnMock = vi.hoisted(() => vi.fn())
const existsMock = vi.hoisted(() => vi.fn(() => true))
const fstatMock = vi.hoisted(() => vi.fn())
const logThroughCore = vi.hoisted(() => vi.fn())
vi.mock('node:child_process', () => ({ spawn: spawnMock }))
vi.mock('node:fs', () => ({ existsSync: existsMock, fstatSync: fstatMock }))
vi.mock('@main/logTimestamps', () => ({ logThroughCore }))

class FakeChild extends EventEmitter {
  stdin = Object.assign(new EventEmitter(), { write: vi.fn() })
  kill = vi.fn()
}

const realPlatform = process.platform
const onPlatform = (platform: string) =>
  Object.defineProperty(process, 'platform', { value: platform })

vi.mock('@shared/core/link', () => ({
  coreSocketPath: () => '/run/livi/core.sock',
  CoreLink: class {
    entry: { opts: LinkOptions; sent: ToCore[]; connected: boolean }
    constructor(opts: LinkOptions) {
      this.entry = { opts, sent: [], connected: true }
      links.push(this.entry)
    }
    send(msg: ToCore) {
      this.entry.sent.push(msg)
      return this.entry.connected
    }
  }
}))

const welcome = (config: Record<string, unknown>): FromCore => ({
  type: 'welcome',
  protocol: 1,
  version: '0',
  rev: 0,
  state: { front: { main: 'livi', cluster: 'livi', aux: 'livi' }, config } as unknown as State
})

async function load() {
  vi.resetModules()
  links.length = 0
  return import('@main/core')
}

beforeEach(() => {
  onPlatform('linux')
  spawnMock.mockReset()
  existsMock.mockReset().mockReturnValue(true)
  logThroughCore.mockReset()
})

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  onPlatform(realPlatform)
})

describe('startCore', () => {
  test('follows the config core sends and resolves on the welcome', async () => {
    const { startCore } = await load()
    const runtimeState = { config: {} } as never
    const onConfig = vi.fn()
    const started = startCore(runtimeState, onConfig)
    expect(links[0].opts.client).toBe('electron')
    expect(links[0].opts.path).toBe('/run/livi/core.sock')

    const config = { huVolume: 0.5 }
    links[0].opts.onMessage(welcome(config))
    await started
    expect(onConfig).toHaveBeenCalledWith(config)

    links[0].opts.onMessage({
      type: 'patch',
      rev: 1,
      ops: [{ op: 'set', path: ['front', 'main'], value: 'projection' }]
    })
    expect(onConfig).toHaveBeenCalledTimes(2)
  })

  test('leaves a config alone that the windows already have', async () => {
    const { startCore } = await load()
    const config = { huVolume: 0.5 }
    const onConfig = vi.fn()
    const started = startCore({ config } as never, onConfig)
    links[0].opts.onMessage(welcome(config))
    await started
    expect(onConfig).not.toHaveBeenCalled()
  })

  test('opens the windows on the defaults when core does not answer', async () => {
    vi.useFakeTimers()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { startCore } = await load()
    const started = startCore({ config: {} } as never, vi.fn())
    vi.advanceTimersByTime(3000)
    await started
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no answer from /run/livi/core.sock'))
  })
})

describe('core on macOS', () => {
  let child: FakeChild

  beforeEach(() => {
    onPlatform('darwin')
    vi.useFakeTimers()
    vi.spyOn(console, 'log').mockImplementation(() => {})
    child = new FakeChild()
    spawnMock.mockReturnValue(child)
    Object.assign(app, { isPackaged: false, getAppPath: () => '/repo' })
  })

  test('starts core from the checkout when none answers', async () => {
    const { startCore, stopCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    vi.advanceTimersByTime(1000)
    const [bin, args, opts] = spawnMock.mock.calls[0]
    expect(bin).toBe('/repo/native/livi-helperd/build/Release/livi-core')
    expect(args).toEqual([])
    expect(opts.env).toMatchObject({
      LIVI_ROOT: '/repo',
      LIVI_NO_UI: '1',
      LIVI_VERSION: '0.0.0-test',
      LIVI_LIFELINE: '1',
      LIVI_UI_PLANES: '/run/livi/ui-planes.sock'
    })
    expect(opts.stdio).toEqual(['pipe', 'inherit', 'inherit'])

    const write = logThroughCore.mock.calls[0][0]
    write('[00:00:00.000] hello')
    expect(child.stdin.write).toHaveBeenCalledWith('[00:00:00.000] hello\n')

    child.emit('exit', 0)
    expect(logThroughCore).toHaveBeenLastCalledWith(null)
    expect(app.quit).toHaveBeenCalled()
    stopCore()
    expect(child.kill).not.toHaveBeenCalled()
  })

  test('an installed app starts the core it carries', async () => {
    Object.assign(app, { isPackaged: true })
    Object.defineProperty(process, 'resourcesPath', { value: '/LIVI.app/Res', configurable: true })
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    vi.advanceTimersByTime(1000)
    const [bin, , opts] = spawnMock.mock.calls[0]
    expect(bin).toBe('/LIVI.app/Res/core/livi-core')
    expect(opts.env.LIVI_RESOURCES).toBe('/LIVI.app/Res')
  })

  test('the app starts again from the update core put in place', async () => {
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    vi.advanceTimersByTime(1000)
    vi.mocked(app.relaunch).mockClear()
    child.emit('exit', 0)
    expect(app.relaunch).not.toHaveBeenCalled()
    child.emit('exit', 75)
    expect(app.relaunch).toHaveBeenCalledTimes(1)
    expect(app.quit).toHaveBeenCalled()
  })

  test('a core that answers in time is left alone', async () => {
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    links[0].opts.onMessage(welcome({}))
    vi.advanceTimersByTime(1000)
    expect(spawnMock).not.toHaveBeenCalled()
  })

  test('says so when there is no core to start', async () => {
    existsMock.mockReturnValue(false)
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    vi.advanceTimersByTime(1000)
    expect(spawnMock).not.toHaveBeenCalled()
    expect(warn).toHaveBeenCalledWith('[core] no livi-core to start')
  })

  test('quitting stops the core it started, a broken stdin stops the hand-over', async () => {
    const { startCore, stopCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    vi.advanceTimersByTime(1000)
    child.stdin.emit('error', new Error('EPIPE'))
    expect(logThroughCore).toHaveBeenLastCalledWith(null)
    stopCore()
    expect(child.kill).toHaveBeenCalledWith('SIGTERM')
  })
})

describe('the UI core started', () => {
  const stdinOf = (kind: 'fifo' | 'socket' | 'device') =>
    fstatMock.mockReturnValue({ isFIFO: () => kind === 'fifo', isSocket: () => kind === 'socket' })

  beforeEach(() => {
    process.env.LIVI_LIFELINE = '1'
    vi.spyOn(process.stdin, 'resume').mockReturnValue(process.stdin)
    vi.spyOn(console, 'log').mockImplementation(() => {})
  })

  afterEach(() => {
    delete process.env.LIVI_LIFELINE
    process.stdin.removeAllListeners('end')
  })

  test('quits when core is gone', async () => {
    stdinOf('fifo')
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    vi.mocked(app.quit).mockClear()
    process.stdin.emit('end')
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('takes a socket as well as a pipe', async () => {
    stdinOf('socket')
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    expect(process.stdin.listenerCount('end')).toBe(1)
  })

  test('ignores a stdin that is no pipe, or none at all', async () => {
    stdinOf('device')
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    fstatMock.mockImplementation(() => {
      throw new Error('EBADF')
    })
    void startCore({ config: {} } as never, vi.fn())
    expect(process.stdin.listenerCount('end')).toBe(0)
  })

  test('a UI nobody started with a lifeline does not watch stdin', async () => {
    delete process.env.LIVI_LIFELINE
    stdinOf('fifo')
    const { startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    expect(process.stdin.listenerCount('end')).toBe(0)
  })
})

describe('saveConfig', () => {
  test('does nothing before core is followed', async () => {
    const { saveConfig } = await load()
    expect(() => saveConfig({ huVolume: 1 } as never)).not.toThrow()
  })

  test('hands the change to core and logs a refusal', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { saveConfig, startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    saveConfig({ mainScreenBounds: { x: 1, y: 2, width: 3, height: 4 } })
    expect(links[0].sent).toEqual([
      {
        type: 'action',
        id: 1,
        action: {
          kind: 'setConfig',
          patch: { mainScreenBounds: { x: 1, y: 2, width: 3, height: 4 } }
        }
      }
    ])
    links[0].opts.onMessage({ type: 'reply', id: 1, error: 'no' })
    await new Promise((r) => setImmediate(r))
    expect(warn).toHaveBeenCalledWith('[core] config not saved: no')
  })

  test('a lost connection fails what still waits', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { saveConfig, startCore } = await load()
    void startCore({ config: {} } as never, vi.fn())
    saveConfig({ kiosk: { main: true, cluster: false, aux: false } })
    links[0].opts.onClose?.()
    await new Promise((r) => setImmediate(r))
    expect(warn).toHaveBeenCalledWith('[core] config not saved: core went away')
  })
})
