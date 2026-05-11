describe('installMainProcessErrorHandlers', () => {
  const realOn = process.on.bind(process)
  let handlers: Record<string, ((arg: unknown) => void) | undefined> = {}
  let warnSpy: jest.SpyInstance
  let errorSpy: jest.SpyInstance

  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
    handlers = {}
    jest.spyOn(process, 'on').mockImplementation(((event: string, cb: (arg: unknown) => void) => {
      handlers[event] = cb
      return process
    }) as typeof process.on)
    warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    errorSpy = jest.spyOn(console, 'error').mockImplementation(() => {})
  })

  afterEach(() => {
    ;(process.on as unknown as jest.SpyInstance).mockRestore()
    warnSpy.mockRestore()
    errorSpy.mockRestore()
    process.on = realOn
  })

  function install() {
    const mod = require('../errorHandler') as typeof import('../errorHandler')
    mod.installMainProcessErrorHandlers()
  }

  test.each([
    ["Couldn't find matching udev device"],
    ['could not find matching udev device'],
    ['Couldnt find matching udev device'],
    ['LIBUSB_ERROR_NO_DEVICE'],
    ['matching udev device']
  ])('warns but never raises on benign libusb noise: %s', (msg) => {
    install()
    handlers.uncaughtException?.(new Error(msg))
    expect(warnSpy).toHaveBeenCalled()
    expect(errorSpy).not.toHaveBeenCalled()
  })

  test('logs non-benign uncaught exceptions to console.error without popping a dialog', () => {
    install()
    handlers.uncaughtException?.(new Error('Something completely unrelated'))
    expect(errorSpy).toHaveBeenCalled()
  })

  test('logs non-benign rejections to console.error', () => {
    install()
    handlers.unhandledRejection?.('plain string rejection')
    expect(errorSpy).toHaveBeenCalled()
  })

  test('is idempotent — installing twice only registers handlers once', () => {
    install()
    install()
    expect((process.on as unknown as jest.Mock).mock.calls.length).toBe(2)
  })
})
