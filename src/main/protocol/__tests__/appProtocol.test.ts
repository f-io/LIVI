describe('appProtocol', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  function loadModule() {
    const existsSync = jest.fn()
    const createReadStream = jest.fn()

    jest.doMock('fs', () => ({
      existsSync,
      createReadStream
    }))

    const mod = require('@main/protocol/appProtocol') as typeof import('@main/protocol/appProtocol')
    const { protocol } = require('electron')

    return {
      registerAppProtocol: mod.registerAppProtocol,
      protocol,
      existsSync,
      createReadStream
    }
  }

  test('registers privileged app scheme at module load', () => {
    const { protocol } = loadModule()

    expect(protocol.registerSchemesAsPrivileged).toHaveBeenCalledWith([
      {
        scheme: 'app',
        privileges: {
          secure: true,
          standard: true,
          corsEnabled: true,
          supportFetchAPI: true,
          stream: true
        }
      }
    ])
  })

  test('registerAppProtocol responds 200 with stream and security headers', () => {
    const { registerAppProtocol, protocol, existsSync, createReadStream } = loadModule()

    const stream = { kind: 'stream' }
    existsSync.mockReturnValue(true)
    createReadStream.mockReturnValue(stream)

    registerAppProtocol()

    const handler = protocol.registerStreamProtocol.mock.calls.find(
      ([scheme]: [string]) => scheme === 'app'
    )?.[1]
    expect(handler).toBeDefined()

    const cb = jest.fn()
    handler({ url: 'app://index.html' }, cb)

    expect(createReadStream).toHaveBeenCalledWith(expect.stringContaining('/renderer/index.html'))
    expect(cb).toHaveBeenCalledWith(
      expect.objectContaining({
        statusCode: 200,
        data: stream,
        headers: expect.objectContaining({
          'Content-Type': 'text/html',
          'Cross-Origin-Opener-Policy': 'same-origin',
          'Cross-Origin-Embedder-Policy': 'require-corp',
          'Cross-Origin-Resource-Policy': 'same-site'
        })
      })
    )
  })

  test('registerAppProtocol responds 404 when file is missing', () => {
    const { registerAppProtocol, protocol, existsSync } = loadModule()

    existsSync.mockReturnValue(false)
    registerAppProtocol()

    const handler = protocol.registerStreamProtocol.mock.calls.find(
      ([scheme]: [string]) => scheme === 'app'
    )?.[1]
    const cb = jest.fn()

    handler({ url: 'app://missing.js' }, cb)

    expect(cb).toHaveBeenCalledWith({ statusCode: 404 })
  })

  test('registerAppProtocol responds 500 on invalid URL parse error', () => {
    const { registerAppProtocol, protocol } = loadModule()

    registerAppProtocol()

    const handler = protocol.registerStreamProtocol.mock.calls.find(
      ([scheme]: [string]) => scheme === 'app'
    )?.[1]
    const cb = jest.fn()
    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    handler({ url: '::invalid-url::' }, cb)

    expect(cb).toHaveBeenCalledWith({ statusCode: 500 })
    expect(errSpy).toHaveBeenCalled()

    errSpy.mockRestore()
  })
})
