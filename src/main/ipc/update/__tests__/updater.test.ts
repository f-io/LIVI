describe('Updater', () => {
  const originalPlatform = process.platform

  beforeEach(async () => {
    vi.resetModules()
    vi.clearAllMocks()
  })

  afterEach(async () => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  async function loadSubject() {
    const sendUpdateEvent = vi.fn()
    const sendUpdateProgress = vi.fn()
    const downloadWithProgress = vi.fn()
    const installOnMacFromFile = vi.fn(() => Promise.resolve())
    const installOnLinuxFromFile = vi.fn(() => Promise.resolve())
    const pickAssetForPlatform = vi.fn(function () {
      return { url: 'https://example.com/LIVI.AppImage' }
    })
    const unlink = vi.fn(() => Promise.resolve())
    const existsSync = vi.fn(() => true)
    const restartApp = vi.fn(() => Promise.resolve())

    vi.doMock('@main/ipc/app', () => ({
      restartApp
    }))
    vi.doMock('@main/ipc/utils', () => ({
      sendUpdateEvent,
      sendUpdateProgress
    }))
    vi.doMock('@main/ipc/update/downloader', () => ({
      downloadWithProgress
    }))
    vi.doMock('@main/ipc/update/install.mac', () => ({
      installOnMacFromFile
    }))
    vi.doMock('@main/ipc/update/install.linux', () => ({
      installOnLinuxFromFile
    }))
    vi.doMock('@main/ipc/update/pickAsset', () => ({
      pickAssetForPlatform
    }))
    vi.doMock('fs', () => ({
      existsSync,
      promises: { unlink }
    }))

    const { Updater } = await import('@main/ipc/update/updater')

    return {
      Updater,
      sendUpdateEvent,
      sendUpdateProgress,
      downloadWithProgress,
      installOnMacFromFile,
      installOnLinuxFromFile,
      pickAssetForPlatform,
      unlink,
      existsSync,
      restartApp
    }
  }

  test('perform emits error on unsupported platform', async () => {
    Object.defineProperty(process, 'platform', { value: 'win32' })
    const { Updater, sendUpdateEvent } = await loadSubject()

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never)

    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'start' })
    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'Unsupported platform'
    })
  })

  test('perform downloads direct URL and reports progress/ready', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, sendUpdateProgress, downloadWithProgress } =
      await loadSubject()

    const cancel = vi.fn()
    downloadWithProgress.mockImplementation(function (_url, _dest, onProgress) {
      onProgress({ received: 50, total: 100, percent: 0.5 })
      return { promise: Promise.resolve(), cancel }
    })

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')

    expect(downloadWithProgress).toHaveBeenCalledWith(
      'https://example.com/LIVI.AppImage',
      expect.stringMatching(/\.AppImage$/),
      expect.any(Function)
    )
    expect(sendUpdateProgress).toHaveBeenCalledWith({
      phase: 'download',
      received: 50,
      total: 100,
      percent: 0.5
    })
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'ready' })
  })

  test('abort removes ready temp file and emits aborted', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, downloadWithProgress, unlink, existsSync } =
      await loadSubject()

    downloadWithProgress.mockReturnValue({ promise: Promise.resolve(), cancel: vi.fn() })
    existsSync.mockReturnValue(true)

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await updater.abort()

    expect(unlink).toHaveBeenCalledWith(expect.stringMatching(/\.AppImage$/))
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'error', message: 'Aborted' })
  })

  test('install emits error when no downloaded update is ready', async () => {
    const { Updater, sendUpdateEvent } = await loadSubject()

    const updater = new Updater({} as never, { usbService: { gracefulReset: vi.fn() } } as never)
    await updater.install()

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'No downloaded update ready'
    })
  })

  test('install runs graceful reset and linux installer when update is ready', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const {
      Updater,
      downloadWithProgress,
      installOnLinuxFromFile,
      installOnMacFromFile,
      restartApp
    } = await loadSubject()

    downloadWithProgress.mockReturnValue({ promise: Promise.resolve(), cancel: vi.fn() })
    const updater = new Updater({} as never, {} as never)

    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await updater.install()

    expect(installOnLinuxFromFile).toHaveBeenCalledWith(expect.stringMatching(/\.AppImage$/))
    expect(restartApp).toHaveBeenCalledTimes(1)
    expect(installOnMacFromFile).not.toHaveBeenCalled()
  })

  test('perform fetches latest release when directUrl is missing and downloads picked asset', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, downloadWithProgress, pickAssetForPlatform } = await loadSubject()

    const fetchSpy = vi.spyOn(global, 'fetch' as any).mockResolvedValue({
      ok: true,
      json: async () => ({
        assets: [{ name: 'LIVI.AppImage', browser_download_url: 'https://example.com/from-feed' }]
      })
    } as Response)

    downloadWithProgress.mockReturnValue({
      promise: Promise.resolve(),
      cancel: vi.fn()
    })
    pickAssetForPlatform.mockReturnValue({ url: 'https://example.com/from-feed' })

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never)

    expect(fetchSpy).toHaveBeenCalledWith(
      'https://api.github.com/repos/f-io/LIVI/releases/latest',
      { headers: { 'User-Agent': 'LIVI-updater' } }
    )
    expect(pickAssetForPlatform).toHaveBeenCalledWith([
      { name: 'LIVI.AppImage', browser_download_url: 'https://example.com/from-feed' }
    ])
    expect(downloadWithProgress).toHaveBeenCalledWith(
      'https://example.com/from-feed',
      expect.stringMatching(/\.AppImage$/),
      expect.any(Function)
    )

    fetchSpy.mockRestore()
  })

  test('perform emits feed status error when release feed response is not ok', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent } = await loadSubject()

    const fetchSpy = vi.spyOn(global, 'fetch' as any).mockResolvedValue({
      ok: false,
      status: 503
    } as Response)

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never)

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'feed 503'
    })

    fetchSpy.mockRestore()
  })

  test('perform emits error when no asset url is available for current platform', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, pickAssetForPlatform } = await loadSubject()

    const fetchSpy = vi.spyOn(global, 'fetch' as any).mockResolvedValue({
      ok: true,
      json: async () => ({
        assets: [{ name: 'something-else' }]
      })
    } as Response)

    pickAssetForPlatform.mockReturnValue({ url: undefined })

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never)

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'No asset found for platform'
    })

    fetchSpy.mockRestore()
  })

  test('abort while downloading calls cancel closure and emits aborted', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, downloadWithProgress } = await loadSubject()

    let resolveDownload!: () => void
    const cancel = vi.fn()

    downloadWithProgress.mockReturnValue({
      promise: new Promise<void>((resolve) => {
        resolveDownload = resolve
      }),
      cancel
    })

    const updater = new Updater({} as never, {} as never)

    const performPromise = updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await Promise.resolve()

    await updater.abort()
    resolveDownload()
    await performPromise

    expect(cancel).toHaveBeenCalledTimes(1)
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'error', message: 'Aborted' })
  })

  test('install continues with the mac installer when gracefulReset fails', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    const { Updater, downloadWithProgress, installOnMacFromFile } = await loadSubject()

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    downloadWithProgress.mockReturnValue({
      promise: Promise.resolve(),
      cancel: vi.fn()
    })

    const timeoutSpy = vi.spyOn(global, 'setTimeout').mockImplementation(((
      cb: (...args: unknown[]) => void
    ) => {
      cb()
      return 0 as unknown as NodeJS.Timeout
    }) as typeof setTimeout)
    const gracefulReset = vi.fn().mockRejectedValue(new Error('reset failed'))
    const updater = new Updater({} as never, { usbService: { gracefulReset } } as never)

    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await updater.install()

    expect(warnSpy).toHaveBeenCalledWith(
      '[MAIN] gracefulReset failed (continuing install):',
      expect.any(Error)
    )
    expect(installOnMacFromFile).toHaveBeenCalledWith(expect.stringMatching(/\.dmg$/))

    timeoutSpy.mockRestore()
    warnSpy.mockRestore()
  })

  test('perform emits error when another update is already in progress', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, downloadWithProgress } = await loadSubject()

    let resolveDownload!: () => void
    downloadWithProgress.mockReturnValue({
      promise: new Promise<void>((resolve) => {
        resolveDownload = resolve
      }),
      cancel: vi.fn()
    })

    const updater = new Updater({} as never, {} as never)

    const firstPerform = updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await Promise.resolve()

    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'Update already in progress'
    })

    resolveDownload()
    await firstPerform
  })

  test('perform uses empty assets array fallback when feed json has no assets', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, pickAssetForPlatform, sendUpdateEvent } = await loadSubject()

    const fetchSpy = vi.spyOn(global, 'fetch' as any).mockResolvedValue({
      ok: true,
      json: async () => ({})
    } as Response)

    pickAssetForPlatform.mockReturnValue({ url: undefined })

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never)

    expect(pickAssetForPlatform).toHaveBeenCalledWith([])
    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'No asset found for platform'
    })

    fetchSpy.mockRestore()
  })

  test('perform on darwin downloads dmg and reaches ready state', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    const { Updater, downloadWithProgress, sendUpdateEvent, sendUpdateProgress } =
      await loadSubject()

    downloadWithProgress.mockImplementation(function (_url, _dest, onProgress) {
      onProgress({ received: 10, total: 20, percent: 0.5 })
      return {
        promise: Promise.resolve(),
        cancel: vi.fn()
      }
    })

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never, 'https://example.com/LIVI.dmg')

    expect(downloadWithProgress).toHaveBeenCalledWith(
      'https://example.com/LIVI.dmg',
      expect.stringMatching(/\.dmg$/),
      expect.any(Function)
    )
    expect(sendUpdateProgress).toHaveBeenCalledWith({
      phase: 'download',
      received: 10,
      total: 20,
      percent: 0.5
    })
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'ready' })
  })

  test('install uses mac installer on darwin when update is ready', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    const { Updater, downloadWithProgress, installOnMacFromFile, installOnLinuxFromFile } =
      await loadSubject()

    downloadWithProgress.mockReturnValue({
      promise: Promise.resolve(),
      cancel: vi.fn()
    })

    const timeoutSpy = vi.spyOn(global, 'setTimeout').mockImplementation(((
      cb: (...args: unknown[]) => void
    ) => {
      cb()
      return 0 as unknown as NodeJS.Timeout
    }) as typeof setTimeout)
    const gracefulReset = vi.fn().mockResolvedValue(undefined)
    const updater = new Updater({} as never, { usbService: { gracefulReset } } as never)

    await updater.perform({} as never, 'https://example.com/LIVI.dmg')
    await updater.install()

    expect(installOnMacFromFile).toHaveBeenCalledWith(expect.stringMatching(/\.dmg$/))
    expect(installOnLinuxFromFile).not.toHaveBeenCalled()

    timeoutSpy.mockRestore()
  })

  test('install emits error when installer throws', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    const { Updater, downloadWithProgress, installOnMacFromFile, sendUpdateEvent } =
      await loadSubject()

    installOnMacFromFile.mockRejectedValue(new Error('install failed'))
    downloadWithProgress.mockReturnValue({
      promise: Promise.resolve(),
      cancel: vi.fn()
    })

    const timeoutSpy = vi.spyOn(global, 'setTimeout').mockImplementation(((
      cb: (...args: unknown[]) => void
    ) => {
      cb()
      return 0 as unknown as NodeJS.Timeout
    }) as typeof setTimeout)
    const gracefulReset = vi.fn().mockResolvedValue(undefined)
    const updater = new Updater({} as never, { usbService: { gracefulReset } } as never)

    await updater.perform({} as never, 'https://example.com/LIVI.dmg')
    await updater.install()

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'install failed'
    })

    timeoutSpy.mockRestore()
  })

  test('perform stringifies non-Error throw values in catch block', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, downloadWithProgress } = await loadSubject()

    downloadWithProgress.mockReturnValue({
      promise: Promise.reject('download failed as string'),
      cancel: vi.fn()
    })

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'download failed as string'
    })
  })

  test('abort in ready state skips unlink when tmpFile is missing or does not exist', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, downloadWithProgress, unlink, existsSync } =
      await loadSubject()

    downloadWithProgress.mockReturnValue({
      promise: Promise.resolve(),
      cancel: vi.fn()
    })
    existsSync.mockReturnValue(false)

    const updater = new Updater({} as never, {} as never)
    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await updater.abort()

    expect(existsSync).toHaveBeenCalledWith(expect.stringMatching(/\.AppImage$/))
    expect(unlink).not.toHaveBeenCalled()
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'error', message: 'Aborted' })
  })

  test('install stringifies non-Error throw values in catch block', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    const { Updater, downloadWithProgress, installOnMacFromFile, sendUpdateEvent } =
      await loadSubject()

    installOnMacFromFile.mockRejectedValue('install failed as string')
    downloadWithProgress.mockReturnValue({
      promise: Promise.resolve(),
      cancel: vi.fn()
    })

    const timeoutSpy = vi.spyOn(global, 'setTimeout').mockImplementation(((
      cb: (...args: unknown[]) => void
    ) => {
      cb()
      return 0 as unknown as NodeJS.Timeout
    }) as typeof setTimeout)
    const gracefulReset = vi.fn().mockResolvedValue(undefined)
    const updater = new Updater({} as never, { usbService: { gracefulReset } } as never)

    await updater.perform({} as never, 'https://example.com/LIVI.dmg')
    await updater.install()

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'install failed as string'
    })

    timeoutSpy.mockRestore()
  })
})
