const exposed: Record<string, unknown> = {}

type IpcHandler = (event: unknown, ...args: unknown[]) => void
type ExposedBridge = {
  projection?: unknown
  app?: unknown
}

const ipcOnHandlers = new Map<string, IpcHandler[]>()

const ipcRendererMock = {
  on: vi.fn(function (channel: string, handler: IpcHandler) {
    const arr = ipcOnHandlers.get(channel) ?? []
    arr.push(handler)
    ipcOnHandlers.set(channel, arr)
  }),
  invoke: vi.fn(),
  send: vi.fn(),
  removeListener: vi.fn(function (channel: string, handler: IpcHandler) {
    const arr = ipcOnHandlers.get(channel) ?? []
    ipcOnHandlers.set(
      channel,
      arr.filter((h) => h !== handler)
    )
  })
}

vi.mock('electron', () => ({
  contextBridge: {
    exposeInMainWorld: vi.fn(function (key: keyof ExposedBridge, value: unknown) {
      exposed[key] = value
    })
  },
  ipcRenderer: ipcRendererMock
}))

describe('preload api bridge', () => {
  beforeEach(async () => {
    vi.resetModules()
    vi.clearAllMocks()
    for (const key of Object.keys(exposed)) delete exposed[key]
    ipcOnHandlers.clear()
  })

  async function loadPreload() {
    await import('../index')
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

  test('exposes projection and app apis in main world', async () => {
    const { projection, app } = await loadPreload()

    expect(projection).toBeDefined()
    expect(app).toBeDefined()
  })

  test('projection quit forwards to ipcRenderer.invoke', async () => {
    const { projection } = await loadPreload()
    ipcRendererMock.invoke.mockResolvedValue(undefined)

    await projection.quit()

    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('quit')
  })

  test('projection ipc sendTouch forwards payload', async () => {
    const { projection } = await loadPreload()

    projection.ipc.sendTouch(0.1, 0.2, 3)

    expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-touch', {
      x: 0.1,
      y: 0.2,
      action: 3
    })
  })

  test('usb listenForEvents flushes queued usb events', async () => {
    const { projection } = await loadPreload()
    const cb = vi.fn()

    emit('usb-event', 'plugged', { vendorId: 1 })

    projection.usb.listenForEvents(cb)

    expect(cb).toHaveBeenCalledTimes(1)
    expect(cb).toHaveBeenCalledWith(expect.anything(), 'plugged', { vendorId: 1 })
  })

  test('usb listenForEvents returns an unsubscribe closure that removes the handler', async () => {
    const { projection } = await loadPreload()
    const cb = vi.fn()

    const unsubscribe = projection.usb.listenForEvents(cb)
    expect(typeof unsubscribe).toBe('function')

    unsubscribe()
    emit('usb-event', 'plugged')

    expect(cb).not.toHaveBeenCalled()
  })

  test('onUSBResetStatus subscribes to both channels and cleanup removes both listeners', async () => {
    const { projection } = await loadPreload()
    const cb = vi.fn()

    const cleanup = projection.onUSBResetStatus(cb)

    expect(ipcRendererMock.on).toHaveBeenCalledWith('usb-reset-start', cb)
    expect(ipcRendererMock.on).toHaveBeenCalledWith('usb-reset-done', cb)

    cleanup()

    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith('usb-reset-start', cb)
    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith('usb-reset-done', cb)
  })

  test('settings onUpdate subscribes and cleanup removes listener', async () => {
    const { projection } = await loadPreload()
    const cb = vi.fn()

    const cleanup = projection.settings.onUpdate(cb)
    emit('settings', { language: 'de' })

    expect(cb).toHaveBeenCalledWith(expect.anything(), { language: 'de' })

    cleanup()

    expect(ipcRendererMock.removeListener).toHaveBeenCalledWith('settings', cb)
  })

  test('ipc onEvent returns an unsubscribe closure that stops the projection-event fan-out', async () => {
    const { projection } = await loadPreload()
    const cb = vi.fn()

    const unsubscribe = projection.ipc.onEvent(cb)
    emit('projection-event', { type: 'plugged' })

    expect(typeof unsubscribe).toBe('function')
    expect(cb).toHaveBeenCalledTimes(1)
    expect(cb).toHaveBeenCalledWith(expect.anything(), { type: 'plugged' })

    unsubscribe()
    emit('projection-event', { type: 'unplugged' })

    expect(cb).toHaveBeenCalledTimes(1)
    expect(ipcRendererMock.removeListener).not.toHaveBeenCalledWith('projection-event', cb)
  })

  test('ipc onAudioChunk flushes queued chunks and offAudioChunk clears active handler', async () => {
    const { projection } = await loadPreload()
    const handler = vi.fn()

    emit('projection-audio-chunk', { id: 'x' })

    projection.ipc.onAudioChunk(handler)
    expect(handler).toHaveBeenCalledWith({ id: 'x' })

    projection.ipc.offAudioChunk(handler)
    emit('projection-audio-chunk', { id: 'y' })

    expect(handler).toHaveBeenCalledTimes(1)
  })

  test('ipc cluster handlers flush queued cluster payloads', async () => {
    const { projection } = await loadPreload()
    const resolutionHandler = vi.fn()

    emit('cluster-video-resolution', { width: 800, height: 480 })

    projection.ipc.onClusterResolution(resolutionHandler)

    expect(resolutionHandler).toHaveBeenCalledWith({ width: 800, height: 480 })
  })

  test('ipc telemetry is not buffered before subscription and offTelemetry removes handler', async () => {
    const { projection } = await loadPreload()
    const handler = vi.fn()

    emit('telemetry:update', { speed: 42 })
    projection.ipc.onTelemetry(handler)
    expect(handler).not.toHaveBeenCalled()

    emit('telemetry:update', { speed: 99 })
    expect(handler).toHaveBeenCalledWith({ speed: 99 })

    projection.ipc.offTelemetry(handler)
    emit('telemetry:update', { speed: 123 })

    expect(handler).toHaveBeenCalledTimes(1)
  })

  test('app onUpdateEvent and onUpdateProgress subscribe and clean up wrapper listeners', async () => {
    const { app } = await loadPreload()
    const eventCb = vi.fn()
    const progressCb = vi.fn()

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
    const { app } = await loadPreload()
    ipcRendererMock.invoke.mockResolvedValue({ ok: true })

    await app.getVersion()
    await app.getLatestRelease()
    await app.performUpdate('https://example.com/update.img')
    await app.resetDongleIcons()
    await app.beginInstall()
    await app.abortUpdate()
    await app.quitApp()
    await app.restartApp()
    await app.openExternal('https://example.com')
    app.notifyUserActivity()

    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:getVersion')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:getLatestRelease')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith(
      'app:performUpdate',
      'https://example.com/update.img'
    )
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('settings:reset-dongle-icons')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:beginInstall')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:abortUpdate')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:quitApp')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:restartApp')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:openExternal', 'https://example.com')
    expect(ipcRendererMock.send).toHaveBeenCalledWith('app:user-activity')
  })

  test('projection wrappers forward invoke calls', async () => {
    const { projection } = await loadPreload()
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
    await projection.ipc.connectBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')
    await projection.ipc.forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')
    await projection.ipc.dongleFirmware('check')
    await projection.ipc.readMedia()
    await projection.ipc.readNavigation()
    await projection.ipc.requestCluster(true)

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
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith(
      'projection-bt-connect-device',
      'AA:BB:CC:DD:EE:FF'
    )
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith(
      'projection-bt-forget-device',
      'AA:BB:CC:DD:EE:FF'
    )
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('dongle-fw', { action: 'check' })
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-media-read')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-navigation-read')
    expect(ipcRendererMock.invoke).toHaveBeenCalledWith('cluster:request', true)
  })

  test('projection volume and visualizer wrappers send ipc events', async () => {
    const { projection } = await loadPreload()

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

  test('usb listenForEvents forwards usb events directly when handler is already registered', async () => {
    const { projection } = await loadPreload()
    const cb = vi.fn()

    projection.usb.listenForEvents(cb)
    emit('usb-event', 'plugged', { vendorId: 1 })

    expect(cb).toHaveBeenCalledTimes(1)
    expect(cb).toHaveBeenCalledWith(expect.anything(), 'plugged', { vendorId: 1 })
  })

  test('ipc onTelemetry forwards telemetry updates directly when handler is already registered', async () => {
    const { projection } = await loadPreload()
    const handler = vi.fn()

    projection.ipc.onTelemetry(handler)
    emit('telemetry:update', { speed: 77 })

    expect(handler).toHaveBeenCalledTimes(1)
    expect(handler).toHaveBeenCalledWith({ speed: 77 })
  })

  test('ipc onAudioChunk forwards chunks directly when handler is already registered', async () => {
    const { projection } = await loadPreload()
    const handler = vi.fn()

    projection.ipc.onAudioChunk(handler)
    emit('projection-audio-chunk', { id: 'live-audio' })

    expect(handler).toHaveBeenCalledTimes(1)
    expect(handler).toHaveBeenCalledWith({ id: 'live-audio' })
  })

  test('ipc cluster handlers forward payloads directly when handlers are already registered', async () => {
    const { projection } = await loadPreload()
    const resolutionHandler = vi.fn()

    projection.ipc.onClusterResolution(resolutionHandler)

    emit('cluster-video-resolution', { width: 1280, height: 720 })

    expect(resolutionHandler).toHaveBeenCalledTimes(1)
    expect(resolutionHandler).toHaveBeenCalledWith({ width: 1280, height: 720 })
  })

  test('ipc offAudioChunk ignores different handler and removes matching handler', async () => {
    const { projection } = await loadPreload()
    const activeHandler = vi.fn()
    const otherHandler = vi.fn()

    projection.ipc.onAudioChunk(activeHandler)
    projection.ipc.offAudioChunk(otherHandler)

    emit('projection-audio-chunk', { id: 'still-active' })
    expect(activeHandler).toHaveBeenCalledWith({ id: 'still-active' })

    projection.ipc.offAudioChunk(activeHandler)
    emit('projection-audio-chunk', { id: 'after-remove' })

    expect(activeHandler).toHaveBeenCalledTimes(1)
  })

  describe('media-key bridge', () => {
    test('app:media-key ignores non-string payloads', async () => {
      const { app } = await loadPreload()
      const handler = vi.fn()
      app.onMediaKey(handler)
      emit('app:media-key', 123)
      emit('app:media-key', '')
      emit('app:media-key', null)
      expect(handler).not.toHaveBeenCalled()
    })

    test('app:media-key dispatches to registered handlers', async () => {
      const { app } = await loadPreload()
      const handler = vi.fn()
      app.onMediaKey(handler)
      emit('app:media-key', 'playPause')
      expect(handler).toHaveBeenCalledWith('playPause')
    })

    test('app:media-key queues commands until a handler subscribes, then flushes', async () => {
      const { app } = await loadPreload()
      // Fire before any handler is registered → queued
      emit('app:media-key', 'next')
      emit('app:media-key', 'prev')

      const handler = vi.fn()
      app.onMediaKey(handler)
      expect(handler).toHaveBeenCalledWith('next')
      expect(handler).toHaveBeenCalledWith('prev')
    })

    test('onMediaKey return value detaches the handler', async () => {
      const { app } = await loadPreload()
      const handler = vi.fn()
      const off = app.onMediaKey(handler)
      off()
      emit('app:media-key', 'playPause')
      expect(handler).not.toHaveBeenCalled()
    })

    test('broadcastMediaKey forwards the command via ipcRenderer.send', async () => {
      const { app } = await loadPreload()
      app.broadcastMediaKey('next')
      expect(ipcRendererMock.send).toHaveBeenCalledWith('app:media-key', 'next')
    })

    test('notifyUserActivity sends app:user-activity', async () => {
      const { app } = await loadPreload()
      app.notifyUserActivity()
      expect(ipcRendererMock.send).toHaveBeenCalledWith('app:user-activity')
    })
  })

  describe('projection ipc wrappers — additional', () => {
    test('restart, switchTransport, getTransportState, getTelemetrySnapshot forward to invoke', async () => {
      const { projection } = await loadPreload()
      ipcRendererMock.invoke.mockResolvedValue(undefined)

      await projection.ipc.restart()
      await projection.ipc.switchTransport()
      await projection.ipc.getTransportState()
      await projection.ipc.getTelemetrySnapshot()

      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('projection-restart')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('transport:switch')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('transport:state')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('telemetry:snapshot')
    })

    test('sendMultiTouch and sendCommand forward through send', async () => {
      const { projection } = await loadPreload()
      projection.ipc.sendMultiTouch([{ id: 0, x: 0.5, y: 0.5, action: 0 }])
      projection.ipc.sendCommand('play')
      expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-multi-touch', [
        { id: 0, x: 0.5, y: 0.5, action: 0 }
      ])
      expect(ipcRendererMock.send).toHaveBeenCalledWith('projection-command', 'play')
    })
  })

  describe('app ipc wrappers — additional', () => {
    test('all simple invoke wrappers forward correctly', async () => {
      const { app } = await loadPreload()
      ipcRendererMock.invoke.mockResolvedValue(undefined)

      await app.getVersion()
      await app.performUpdate('http://x')
      await app.resetDongleIcons()
      await app.beginInstall()
      await app.abortUpdate()
      await app.quitApp()
      await app.restartApp()

      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:getVersion')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:performUpdate', 'http://x')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('settings:reset-dongle-icons')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:beginInstall')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:abortUpdate')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:quitApp')
      expect(ipcRendererMock.invoke).toHaveBeenCalledWith('app:restartApp')
    })
  })
})
