import type { Mock, MockInstance } from 'vitest'

process.stdout.setMaxListeners(0)
process.stderr.setMaxListeners(0)

describe('installMainProcessErrorHandlers', () => {
  const realOn = process.on.bind(process)
  let handlers: Record<string, ((arg: unknown) => void) | undefined> = {}
  let warnSpy: MockInstance
  let errorSpy: MockInstance

  beforeEach(async () => {
    vi.resetModules()
    vi.clearAllMocks()
    handlers = {}
    vi.spyOn(process, 'on').mockImplementation(((event: string, cb: (arg: unknown) => void) => {
      handlers[event] = cb
      return process
    }) as typeof process.on)
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    errorSpy = vi.spyOn(console, 'error').mockImplementation(function () {})
  })

  afterEach(async () => {
    ;(process.on as unknown as MockInstance).mockRestore()
    warnSpy.mockRestore()
    errorSpy.mockRestore()
    process.on = realOn
  })

  async function install() {
    const mod = await import('../errorHandler')
    mod.installMainProcessErrorHandlers()
  }

  test('logs non-benign uncaught exceptions to console.error without popping a dialog', async () => {
    await install()
    handlers.uncaughtException?.(new Error('Something completely unrelated'))
    expect(errorSpy).toHaveBeenCalled()
  })

  test('logs non-benign rejections to console.error', async () => {
    await install()
    handlers.unhandledRejection?.('plain string rejection')
    expect(errorSpy).toHaveBeenCalled()
  })

  test('is idempotent — installing twice only registers handlers once', async () => {
    await install()
    await install()
    expect((process.on as unknown as Mock).mock.calls.length).toBe(2)
  })

  test('swallows EPIPE for exceptions and rejections', async () => {
    await install()
    const epipe = Object.assign(new Error('write EPIPE'), { code: 'EPIPE' })
    handlers.uncaughtException?.(epipe)
    handlers.unhandledRejection?.(epipe)
    expect(warnSpy).not.toHaveBeenCalled()
    expect(errorSpy).not.toHaveBeenCalled()
  })

  test('handles null rejection reasons', async () => {
    await install()
    handlers.unhandledRejection?.(null)
    expect(errorSpy).toHaveBeenCalledWith('[errorHandler] unhandledRejection:', 'null')
  })

  test('describes errors without a stack', async () => {
    await install()
    const bare = new Error('no stack here')
    bare.stack = undefined
    handlers.uncaughtException?.(bare)
    expect(errorSpy).toHaveBeenCalledWith('[errorHandler] uncaughtException:', 'no stack here\n')
  })

  test('falls back to a placeholder for unprintable errors', async () => {
    await install()
    const unprintable = {
      toString() {
        throw new Error('nope')
      }
    }
    handlers.uncaughtException?.(unprintable)
    expect(errorSpy).toHaveBeenCalledWith(
      '[errorHandler] uncaughtException:',
      '<unprintable error>'
    )
  })

  test('installs no-op error listeners on stdout and stderr', async () => {
    const stdoutOn = vi.spyOn(process.stdout, 'on').mockImplementation((() => {}) as never)
    const stderrOn = vi.spyOn(process.stderr, 'on').mockImplementation((() => {}) as never)
    await install()
    const outHandler = stdoutOn.mock.calls.find((c) => c[0] === 'error')?.[1] as () => void
    const errHandler = stderrOn.mock.calls.find((c) => c[0] === 'error')?.[1] as () => void
    expect(() => outHandler()).not.toThrow()
    expect(() => errHandler()).not.toThrow()
    stdoutOn.mockRestore()
    stderrOn.mockRestore()
  })
})
