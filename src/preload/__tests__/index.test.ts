const exposed: Record<string, unknown> = {}

type IpcHandler = (event: unknown, ...args: unknown[]) => void
type ExposedBridge = {
  projection?: unknown
  app?: unknown
}

const ipcOnHandlers = new Map<string, IpcHandler[]>()

const ipcRendererMock = {
  on: jest.fn((channel: string, handler: IpcHandler) => {
    const arr = ipcOnHandlers.get(channel) ?? []
    arr.push(handler)
    ipcOnHandlers.set(channel, arr)
  }),
  invoke: jest.fn(),
  send: jest.fn(),
  removeListener: jest.fn((channel: string, handler: IpcHandler) => {
    const arr = ipcOnHandlers.get(channel) ?? []
    ipcOnHandlers.set(
      channel,
      arr.filter((h) => h !== handler)
    )
  })
}

jest.mock('electron', () => ({
  contextBridge: {
    exposeInMainWorld: jest.fn((key: keyof ExposedBridge, value: unknown) => {
      exposed[key] = value
    })
  },
  ipcRenderer: ipcRendererMock
}))

describe('preload api bridge', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
    for (const key of Object.keys(exposed)) delete exposed[key]
    ipcOnHandlers.clear()
  })

  function loadPreload() {
    require('../index')
    return {
      projection: exposed.projection,
      app: exposed.app
    }
  }

  function emit(channel: string, ...args: unknown[]) {
    const handlers = ipcOnHandlers.get(channel) ?? []
    for (const handler of handlers) {
      handler({ channel }, ...args)
    }
  }

  test('exposes projection and app apis in main world', () => {
    const { projection, app } = loadPreload()

    expect(projection).toBeDefined()
    expect(app).toBeDefined()
  })

  test('projection quit forwards to ipcRenderer.invoke', async () => {
    const { projection } = loadPreload()
    ipcRendererMock.invoke.mockResolvedValue(undefined)

    await projection.quit()

    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('quit')
  })

  test('projection ipc sendRawMessage converts Uint8Array to number array', () => {
    const { projection } = loadPreload()

    projection.ipc.sendRawMessage(7, new Uint8Array([1, 2, 255]))

    expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-raw-message', {
      type: 7,
      data: [1, 2, 255]
    })
  })

  test('projection ipc sendTouch forwards payload', () => {
    const { projection } = loadPreload()

    projection.ipc.sendTouch(0.1, 0.2, 3)

    expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-touch', {
      x: 0.1,
      y: 0.2,
      action: 3
    })
  })

  test('usb listenForEvents flushes queued usb events', () => {
    const { projection } = loadPreload()
    const cb = jest.fn()

    emit('usb-event', 'plugged', { vendorId: 1 })

    projection.usb.listenForEvents(cb)

    expect(cb).toHaveBeenCalledTimes(1)
    expect(cb).toHaveBeenCalledWith(expect.anything(), 'plugged', { vendorId: 1 })
  })

  test('usb unlistenForEvents removes handler', () => {
    const { projection } = loadPreload()
    const cb = jest.fn()

    projection.usb.listenForEvents(cb)
    projection.usb.unlistenForEvents(cb)
    emit('usb-event', 'plugged')

    expect(cb).not.toHaveBeenCalled()
  })

  test('onUSBResetStatus subscribes to both channels and cleanup removes both listeners', () => {
    const { projection } = loadPreload()
    const cb = jest.fn()

    const cleanup = projection.onUSBResetStatus(cb)

    expect(ipcRendererMock.on).toHaveBeenCalledWith('usb-reset-start', cb)
    expect(ipcRendererMock.on).toHaveBeenCalledWith('usb-reset-done', cb)

    cleanup()

    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith('usb-reset-start', cb)
    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith('usb-reset-done', cb)
  })

  test('settings onUpdate subscribes and cleanup removes listener', () => {
    const { projection } = loadPreload()
    const cb = jest.fn()

    const cleanup = projection.settings.onUpdate(cb)
    emit('settings', { language: 'de' })

    expect(cb).toHaveBeenCalledWith(expect.anything(), { language: 'de' })

    cleanup()

    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith('settings', cb)
  })

  test('ipc onEvent and offEvent forward projection-event listener management', () => {
    const { projection } = loadPreload()
    const cb = jest.fn()

    projection.ipc.onEvent(cb)
    emit('projection-event', { type: 'plugged' })
    expect(cb).toHaveBeenCalledWith(expect.anything(), { type: 'plugged' })

    projection.ipc.offEvent(cb)
    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith('projection-event', cb)
  })

  test('ipc onVideoChunk flushes queued chunks and offVideoChunk clears active handler', () => {
    const { projection } = loadPreload()
    const handler = jest.fn()

    emit('projection-video-chunk', { id: 'a', offset: 0 })
    emit('projection-video-chunk', { id: 'a', offset: 1 })

    projection.ipc.onVideoChunk(handler)

    expect(handler).toHaveBeenCalledTimes(2)
    expect(handler).toHaveBeenNthCalledWith(1, { id: 'a', offset: 0 })
    expect(handler).toHaveBeenNthCalledWith(2, { id: 'a', offset: 1 })

    projection.ipc.offVideoChunk(handler)
    emit('projection-video-chunk', { id: 'b', offset: 0 })

    expect(handler).toHaveBeenCalledTimes(2)
  })

  test('ipc onAudioChunk flushes queued chunks and offAudioChunk clears active handler', () => {
    const { projection } = loadPreload()
    const handler = jest.fn()

    emit('projection-audio-chunk', { id: 'x' })

    projection.ipc.onAudioChunk(handler)
    expect(handler).toHaveBeenCalledWith({ id: 'x' })

    projection.ipc.offAudioChunk(handler)
    emit('projection-audio-chunk', { id: 'y' })

    expect(handler).toHaveBeenCalledTimes(1)
  })

  test('ipc maps handlers flush queued maps payloads', () => {
    const { projection } = loadPreload()
    const videoHandler = jest.fn()
    const resolutionHandler = jest.fn()

    emit('maps-video-chunk', { chunk: 1 })
    emit('maps-video-resolution', { width: 800, height: 480 })

    projection.ipc.onMapsVideoChunk(videoHandler)
    projection.ipc.onMapsResolution(resolutionHandler)

    expect(videoHandler).toHaveBeenCalledWith({ chunk: 1 })
    expect(resolutionHandler).toHaveBeenCalledWith({ width: 800, height: 480 })
  })

  test('ipc telemetry queues before subscription and offTelemetry removes handler', () => {
    const { projection } = loadPreload()
    const handler = jest.fn()

    emit('telemetry:update', { speed: 42 })

    projection.ipc.onTelemetry(handler)
    expect(handler).toHaveBeenCalledWith({ speed: 42 })

    projection.ipc.offTelemetry(handler)
    emit('telemetry:update', { speed: 99 })

    expect(handler).toHaveBeenCalledTimes(1)
  })

  test('app onUpdateEvent and onUpdateProgress subscribe and clean up wrapper listeners', () => {
    const { app } = loadPreload()
    const eventCb = jest.fn()
    const progressCb = jest.fn()

    const offEvent = app.onUpdateEvent(eventCb)
    const offProgress = app.onUpdateProgress(progressCb)

    emit('update:event', { phase: 'check' })
    emit('update:progress', { percent: 50 })

    expect(eventCb).toHaveBeenCalledWith({ phase: 'check' })
    expect(progressCb).toHaveBeenCalledWith({ percent: 50 })

    offEvent()
    offProgress()

    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith(
      'update:event',
      expect.any(Function)
    )
    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith(
      'update:progress',
      expect.any(Function)
    )
  })

  test('app wrappers forward invoke and send calls', async () => {
    const { app } = loadPreload()
    ipcRendererMock.invoke.mockResolvedValue({ ok: true })

    await app.quitApp()
    await app.restartApp()
    await app.openExternal('https://example.com')
    app.notifyUserActivity()

    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:quitApp')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:restartApp')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:openExternal', 'https://example.com')
    expect(ipcRendererMock.send).toHaveBeenCalledWith('app:user-activity')
  })

  test('projection wrappers forward invoke calls', async () => {
    const { projection } = loadPreload()
    ipcRendererMock.invoke.mockResolvedValue({ ok: true })

    await projection.usb.forceReset()
    await projection.usb.detectDongle()
    await projection.usb.getDeviceInfo()
    await projection.usb.getLastEvent()
    await projection.usb.getSysdefaultPrettyName()
    await projection.usb.uploadIcons()
    await projection.usb.uploadLiviScripts()
    await projection.settings.get()
    await projection.settings.save({ language: 'de' })
    await projection.ipc.start()
    await projection.ipc.stop()
    await projection.ipc.sendFrame()
    await projection.ipc.setBluetoothPairedList('abc')
    await projection.ipc.dongleFirmware('check')
    await projection.ipc.readMedia()
    await projection.ipc.readNavigation()
    await projection.ipc.requestMaps(true)

    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('usb-force-reset')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('usb-detect-dongle')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection:usbDevice')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('usb-last-event')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('get-sysdefault-mic-label')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-upload-icons')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-upload-livi-scripts')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('getSettings')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('save-settings', { language: 'de' })
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-start')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-stop')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-sendframe')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-bt-pairedlist-set', 'abc')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('dongle-fw', { action: 'check' })
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-media-read')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-navigation-read')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('maps:request', true)
  })

  test('projection volume and visualizer wrappers send ipc events', () => {
    const { projection } = loadPreload()

    projection.ipc.setVolume('nav', 0.4)
    projection.ipc.setVisualizerEnabled(1 as any)
    projection.ipc.sendCommand('frame')
    projection.ipc.sendMultiTouch([{ id: 1, x: 0.1, y: 0.2, action: 2 }])

    expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-set-volume', {
      stream: 'nav',
      volume: 0.4
    })
    expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-set-visualizer-enabled', true)
    expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-command', 'frame')
    expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-multi-touch', [
      { id: 1, x: 0.1, y: 0.2, action: 2 }
    ])
  })
})
