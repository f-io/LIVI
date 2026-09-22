vi.mock('electron', () => ({
  app: { getPath: () => '/tmp/livi-userdata' },
  protocol: {
    registerSchemesAsPrivileged: vi.fn(),
    handle: vi.fn()
  },
  net: {
    fetch: vi.fn()
  }
}))

let mockHome = '/tmp'

describe('appProtocol', () => {
  beforeEach(async () => {
    vi.resetModules()
    vi.clearAllMocks()
    mockHome = '/tmp'
  })

  async function loadModule() {
    const existsSync = vi.fn()
    const mkdirSync = vi.fn()
    const writeFileSync = vi.fn()

    vi.doMock('fs', () => ({
      existsSync,
      mkdirSync,
      writeFileSync
    }))

    vi.doMock('os', () => ({
      homedir: () => mockHome
    }))

    vi.doMock('url', () => ({
      pathToFileURL: vi.fn(function (file: string) {
        return {
          toString: () => `file://${file}`
        }
      })
    }))

    const mod = await import('@main/protocol/appProtocol')
    const { protocol, net } = await import('electron')

    return {
      registerAppProtocol: mod.registerAppProtocol,
      seedCustomPage: mod.seedCustomPage,
      customPageExists: mod.customPageExists,
      customIconExists: mod.customIconExists,
      setCustomPageConfig: mod.setCustomPageConfig,
      protocol,
      net,
      existsSync,
      mkdirSync,
      writeFileSync
    }
  }

  function handler(protocol: { handle: ReturnType<typeof vi.fn> }) {
    return protocol.handle.mock.calls[0][1] as (req: { url: string }) => Promise<Response>
  }

  test('registers privileged app scheme at module load', async () => {
    const { protocol } = await loadModule()

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

  test('registerAppProtocol registers app handler', async () => {
    const { registerAppProtocol, protocol } = await loadModule()

    registerAppProtocol()

    expect(protocol.handle).toHaveBeenCalledWith('app', expect.any(Function))
  })

  test('registerAppProtocol responds 200 with fetched file body and security headers', async () => {
    const { registerAppProtocol, protocol, net, existsSync } = await loadModule()

    existsSync.mockReturnValue(true)
    net.fetch.mockResolvedValue(
      new Response('ok', {
        status: 200,
        headers: {
          'Content-Type': 'text/html'
        }
      })
    )

    registerAppProtocol()

    const handler = protocol.handle.mock.calls.find(([scheme]: [string]) => scheme === 'app')?.[1]
    expect(handler).toBeDefined()

    const response = await handler({ url: 'app://index.html' })

    expect(net.fetch).toHaveBeenCalledWith(expect.stringContaining('/renderer/index.html'))
    expect(response.status).toBe(200)
    expect(response.headers.get('Content-Type')).toBe('text/html')
    expect(response.headers.get('Cross-Origin-Opener-Policy')).toBe('same-origin')
    expect(response.headers.get('Cross-Origin-Embedder-Policy')).toBe('require-corp')
    expect(response.headers.get('Cross-Origin-Resource-Policy')).toBe('same-site')
    expect(await response.text()).toBe('ok')
  })

  test('registerAppProtocol responds 404 when file is missing', async () => {
    const { registerAppProtocol, protocol, net, existsSync } = await loadModule()

    existsSync.mockReturnValue(false)

    registerAppProtocol()

    const handler = protocol.handle.mock.calls.find(([scheme]: [string]) => scheme === 'app')?.[1]
    expect(handler).toBeDefined()

    const response = await handler({ url: 'app://missing.js' })

    expect(response.status).toBe(404)
    expect(net.fetch).not.toHaveBeenCalled()
  })

  test('registerAppProtocol responds 500 on invalid URL parse error', async () => {
    const { registerAppProtocol, protocol } = await loadModule()

    registerAppProtocol()

    const handler = protocol.handle.mock.calls.find(([scheme]: [string]) => scheme === 'app')?.[1]
    expect(handler).toBeDefined()

    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined)

    const response = await handler({ url: '::invalid-url::' })

    expect(response.status).toBe(500)
    expect(errSpy).toHaveBeenCalled()

    errSpy.mockRestore()
  })

  describe('custom folder', () => {
    test('seeds the example page once, naming the file it wrote', async () => {
      const { seedCustomPage, existsSync, mkdirSync, writeFileSync } = await loadModule()
      existsSync.mockReturnValue(false)

      seedCustomPage()

      expect(mkdirSync).toHaveBeenCalledWith('/tmp/livi-userdata/custom', { recursive: true })
      const [file, body] = writeFileSync.mock.calls[0]
      expect(file).toBe('/tmp/livi-userdata/custom/index.html')
      expect(body).toContain('~/livi-userdata/custom/index.html')
      expect(body).not.toContain('{{FILE}}')

      const [iconFile, icon] = writeFileSync.mock.calls[1]
      expect(iconFile).toBe('/tmp/livi-userdata/custom/icon.svg')
      expect(icon).toContain('<svg')
      expect(icon).toContain('viewBox="0 0 24 24"')
    })

    test('reports whether the folder names its own tab icon', async () => {
      const { customIconExists, existsSync } = await loadModule()

      existsSync.mockReturnValue(true)
      expect(customIconExists()).toBe(true)
      expect(existsSync).toHaveBeenCalledWith('/tmp/livi-userdata/custom/icon.svg')

      existsSync.mockReturnValue(false)
      expect(customIconExists()).toBe(false)
    })

    test('names the file relative to the home directory', async () => {
      const { seedCustomPage, existsSync, writeFileSync } = await loadModule()
      existsSync.mockReturnValue(false)

      seedCustomPage()

      expect(writeFileSync.mock.calls[0][1]).toContain('~/livi-userdata/custom/index.html')
    })

    test('names the file outright when it lies outside the home directory', async () => {
      const { seedCustomPage, existsSync, writeFileSync } = await loadModule()
      mockHome = '/somewhere/else'
      existsSync.mockReturnValue(false)

      seedCustomPage()

      expect(writeFileSync.mock.calls[0][1]).toContain('/tmp/livi-userdata/custom/index.html')
    })

    test('reports whether the folder holds a page', async () => {
      const { customPageExists, existsSync } = await loadModule()

      existsSync.mockReturnValue(true)
      expect(customPageExists()).toBe(true)
      expect(existsSync).toHaveBeenCalledWith('/tmp/livi-userdata/custom/index.html')

      existsSync.mockReturnValue(false)
      expect(customPageExists()).toBe(false)
    })

    test('leaves an existing folder untouched', async () => {
      const { seedCustomPage, existsSync, writeFileSync } = await loadModule()
      existsSync.mockReturnValue(true)

      seedCustomPage()

      expect(writeFileSync).not.toHaveBeenCalled()
    })

    test('reports a folder it cannot create', async () => {
      const { seedCustomPage, existsSync, mkdirSync } = await loadModule()
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      existsSync.mockReturnValue(false)
      mkdirSync.mockImplementation(() => {
        throw new Error('read-only')
      })

      seedCustomPage()

      expect(warn).toHaveBeenCalledWith(expect.stringContaining('could not seed'))
      warn.mockRestore()
    })

    test('serves a file from the custom folder without caching it', async () => {
      const { registerAppProtocol, protocol, net, existsSync } = await loadModule()
      existsSync.mockReturnValue(true)
      ;(net.fetch as ReturnType<typeof vi.fn>).mockResolvedValue(
        new Response('mine', { headers: { 'content-type': 'text/html' } })
      )
      registerAppProtocol()

      const res = await handler(protocol)({ url: 'app://index.html/custom/index.html' })

      expect(res.status).toBe(200)
      expect(res.headers.get('cache-control')).toBe('no-store')
      expect((net.fetch as ReturnType<typeof vi.fn>).mock.calls[0][0]).toContain(
        '/tmp/livi-userdata/custom/index.html'
      )
    })

    test('serves the palette from the config', async () => {
      const { registerAppProtocol, setCustomPageConfig, protocol } = await loadModule()
      setCustomPageConfig(() => ({ primaryColorDark: '#ff0000', darkMode: true }))
      registerAppProtocol()

      const res = await handler(protocol)({ url: 'app://index.html/custom/livi-theme.css' })
      const css = await res.text()

      expect(res.headers.get('content-type')).toContain('text/css')
      expect(css).toContain('--livi-primary: #ff0000')
      expect(css).toContain('color-scheme: dark')
    })

    test('follows the light theme when dark mode is off', async () => {
      const { registerAppProtocol, setCustomPageConfig, protocol } = await loadModule()
      setCustomPageConfig(() => ({ darkMode: false }))
      registerAppProtocol()

      const css = await (
        await handler(protocol)({ url: 'app://index.html/custom/livi-theme.css' })
      ).text()

      expect(css).toContain('color-scheme: light')
      expect(css).toContain('--livi-primary: #008585')
    })

    test('serves the default palette before a config was handed over', async () => {
      const { registerAppProtocol, protocol } = await loadModule()
      registerAppProtocol()

      const css = await (
        await handler(protocol)({ url: 'app://index.html/custom/livi-theme.css' })
      ).text()

      expect(css).toContain('--livi-primary: #00adad')
    })

    test('refuses to leave the custom folder', async () => {
      const { registerAppProtocol, protocol, existsSync } = await loadModule()
      existsSync.mockReturnValue(true)
      registerAppProtocol()

      const res = await handler(protocol)({
        url: 'app://index.html/custom/%2e%2e%2f%2e%2e%2fetc%2fhosts'
      })

      expect(res.status).toBe(404)
    })

    test('answers 404 for a missing file in the folder', async () => {
      const { registerAppProtocol, protocol, existsSync } = await loadModule()
      existsSync.mockReturnValue(false)
      registerAppProtocol()

      const res = await handler(protocol)({ url: 'app://index.html/custom/nope.html' })

      expect(res.status).toBe(404)
    })
  })
})
