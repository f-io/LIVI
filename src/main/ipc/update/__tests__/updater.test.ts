describe('Updater', () => {
  const originalPlatform = process.platform

  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  function loadSubject() {
    const sendUpdateEvent = jest.fn()
    const sendUpdateProgress = jest.fn()
    const downloadWithProgress = jest.fn()
    const installOnMacFromFile = jest.fn(() => Promise.resolve())
    const installOnLinuxFromFile = jest.fn(() => Promise.resolve())
    const pickAssetForPlatform = jest.fn(() => ({ url: 'https://example.com/LIVI.AppImage' }))
    const unlink = jest.fn(() => Promise.resolve())
    const existsSync = jest.fn(() => true)

    jest.doMock('@main/ipc/utils', () => ({
      sendUpdateEvent,
      sendUpdateProgress
    }))
    jest.doMock('@main/ipc/update/downloader', () => ({
      downloadWithProgress
    }))
    jest.doMock('@main/ipc/update/install.mac', () => ({
      installOnMacFromFile
    }))
    jest.doMock('@main/ipc/update/install.linux', () => ({
      installOnLinuxFromFile
    }))
    jest.doMock('@main/ipc/update/pickAsset', () => ({
      pickAssetForPlatform
    }))
    jest.doMock('fs', () => ({
      existsSync,
      promises: { unlink }
    }))

    const { Updater } =
      require('@main/ipc/update/updater') as typeof import('@main/ipc/update/updater')

    return {
      Updater,
      sendUpdateEvent,
      sendUpdateProgress,
      downloadWithProgress,
      installOnMacFromFile,
      installOnLinuxFromFile,
      pickAssetForPlatform,
      unlink,
      existsSync
    }
  }

  test('perform emits error on unsupported platform', async () => {
    Object.defineProperty(process, 'platform', { value: 'win32' })
    const { Updater, sendUpdateEvent } = loadSubject()

    const updater = new Updater({} as never)
    await updater.perform({} as never)

    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'start' })
    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'Unsupported platform'
    })
  })

  test('perform downloads direct URL and reports progress/ready', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, sendUpdateEvent, sendUpdateProgress, downloadWithProgress } = loadSubject()

    const cancel = jest.fn()
    downloadWithProgress.mockImplementation((_url, _dest, onProgress) => {
      onProgress({ received: 50, total: 100, percent: 0.5 })
      return { promise: Promise.resolve(), cancel }
    })

    const updater = new Updater({} as never)
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
    const { Updater, sendUpdateEvent, downloadWithProgress, unlink, existsSync } = loadSubject()

    downloadWithProgress.mockReturnValue({ promise: Promise.resolve(), cancel: jest.fn() })
    existsSync.mockReturnValue(true)

    const updater = new Updater({} as never)
    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await updater.abort()

    expect(unlink).toHaveBeenCalledWith(expect.stringMatching(/\.AppImage$/))
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'error', message: 'Aborted' })
  })

  test('install emits error when no downloaded update is ready', async () => {
    const { Updater, sendUpdateEvent } = loadSubject()

    const updater = new Updater({ usbService: { gracefulReset: jest.fn() } } as never)
    await updater.install()

    expect(sendUpdateEvent).toHaveBeenCalledWith({
      phase: 'error',
      message: 'No downloaded update ready'
    })
  })

  test('install runs graceful reset and linux installer when update is ready', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    const { Updater, downloadWithProgress, installOnLinuxFromFile, installOnMacFromFile } =
      loadSubject()

    downloadWithProgress.mockReturnValue({ promise: Promise.resolve(), cancel: jest.fn() })
    const timeoutSpy = jest.spyOn(global, 'setTimeout').mockImplementation(((
      cb: (...args: unknown[]) => void
    ) => {
      cb()
      return 0 as unknown as NodeJS.Timeout
    }) as typeof setTimeout)

    const gracefulReset = jest.fn(() => Promise.resolve())
    const updater = new Updater({ usbService: { gracefulReset } } as never)

    await updater.perform({} as never, 'https://example.com/LIVI.AppImage')
    await updater.install()

    expect(gracefulReset).toHaveBeenCalledTimes(1)
    expect(installOnLinuxFromFile).toHaveBeenCalledWith(expect.stringMatching(/\.AppImage$/))
    expect(installOnMacFromFile).not.toHaveBeenCalled()
    timeoutSpy.mockRestore()
  })
})
