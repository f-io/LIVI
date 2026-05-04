import type { ExtraConfig } from '@shared/types'

type ProjectionApiOverrides = {
  settings?: {
    get?: jest.Mock | undefined
    save?: jest.Mock | undefined
    onUpdate?: jest.Mock | undefined
  }
  usb?: {
    forceReset?: jest.Mock | undefined
  }
  ipc?: {
    setVolume?: jest.Mock | undefined
    setBluetoothPairedList?: jest.Mock | undefined
    connectBluetoothPairedDevice?: jest.Mock | undefined
    forgetBluetoothPairedDevice?: jest.Mock | undefined
    sendCommand?: jest.Mock | undefined
    onTelemetry?: jest.Mock | undefined
    offTelemetry?: jest.Mock | undefined
  }
}

type TestProjectionApi = {
  settings: {
    get: jest.Mock
    save: jest.Mock
    onUpdate: jest.Mock
  }
  usb: {
    forceReset: jest.Mock
  }
  ipc: {
    setVolume: jest.Mock | undefined
    setBluetoothPairedList: jest.Mock | undefined
    connectBluetoothPairedDevice: jest.Mock | undefined
    forgetBluetoothPairedDevice: jest.Mock | undefined
    sendCommand: jest.Mock | undefined
    onTelemetry: jest.Mock | undefined
    offTelemetry: jest.Mock | undefined
  }
}

type TestWindow = Omit<Window, 'projection'> & {
  projection?: TestProjectionApi
}

describe('store', () => {
  const makeProjectionApi = (overrides?: {
    settings?: Partial<{
      get: jest.Mock
      save: jest.Mock
      onUpdate: jest.Mock
    }>
    usb?: Partial<{
      forceReset: jest.Mock
    }>
    ipc?: Partial<{
      setVolume: jest.Mock | undefined
      setBluetoothPairedList: jest.Mock | undefined
      connectBluetoothPairedDevice: jest.Mock | undefined
      forgetBluetoothPairedDevice: jest.Mock | undefined
      sendCommand: jest.Mock | undefined
      onTelemetry: jest.Mock | undefined
      offTelemetry: jest.Mock | undefined
    }>
  }) => ({
    settings: {
      get: jest.fn(),
      save: jest.fn(),
      onUpdate: jest.fn(),
      ...(overrides?.settings ?? {})
    },
    usb: {
      forceReset: jest.fn(),
      ...(overrides?.usb ?? {})
    },
    ipc: {
      setVolume: jest.fn(),
      setBluetoothPairedList: jest.fn(),
      connectBluetoothPairedDevice: jest.fn(),
      forgetBluetoothPairedDevice: jest.fn(),
      sendCommand: jest.fn(),
      onTelemetry: jest.fn(),
      offTelemetry: jest.fn(),
      ...(overrides?.ipc ?? {})
    }
  })

  const baseSettings = {
    audioVolume: 0.8,
    navVolume: 0.4,
    voiceAssistantVolume: 0.5,
    callVolume: 0.6,
    visualAudioDelayMs: 120,
    nightMode: false,
    micType: 0,
    telemetryDashboards: []
  } as unknown as ExtraConfig

  const loadFreshStore = (projectionOverrides?: ProjectionApiOverrides) => {
    jest.resetModules()

    const testWindow = window as unknown as TestWindow
    testWindow.projection = undefined

    if (projectionOverrides) {
      testWindow.projection = makeProjectionApi(projectionOverrides)
    }

    return require('../store') as typeof import('../store')
  }

  const waitForStoreSettings = async (useLiviStore: {
    getState: () => { settings: ExtraConfig | null }
  }) => {
    for (let i = 0; i < 10; i += 1) {
      if (useLiviStore.getState().settings) return
      await Promise.resolve()
    }
    throw new Error('store settings were not initialized')
  }

  beforeEach(() => {
    jest.clearAllMocks()
    const testWindow = window as unknown as TestWindow
    testWindow.projection = undefined
  })

  test('init loads settings from projection api and applies derived audio values', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const state = useLiviStore.getState()

    expect(projection.settings.get).toHaveBeenCalledTimes(1)
    expect(state.settings).toEqual(baseSettings)
    expect(state.restartBaseline).toEqual(baseSettings)
    expect(state.audioVolume).toBe(0.8)
    expect(state.navVolume).toBe(0.4)
    expect(state.voiceAssistantVolume).toBe(0.5)
    expect(state.callVolume).toBe(0.6)
    expect(state.visualAudioDelayMs).toBe(120)

    expect(projection.ipc.setVolume).toHaveBeenCalledWith('music', 0.8)
    expect(projection.ipc.setVolume).toHaveBeenCalledWith('nav', 0.4)
    expect(projection.ipc.setVolume).toHaveBeenCalledWith('voiceAssistant', 0.5)
    expect(projection.ipc.setVolume).toHaveBeenCalledWith('call', 0.6)
  })

  test('getSettings refreshes state from main settings api', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue({
          ...baseSettings,
          audioVolume: 0.2,
          navVolume: 0.3
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().getSettings()

    const state = useLiviStore.getState()
    expect(state.audioVolume).toBe(0.2)
    expect(state.navVolume).toBe(0.3)
  })

  test('markRestartBaseline stores current settings as restart baseline', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      settings: { ...baseSettings, nightMode: true } as ExtraConfig,
      restartBaseline: null
    })

    useLiviStore.getState().markRestartBaseline()

    expect(useLiviStore.getState().restartBaseline).toEqual({
      ...baseSettings,
      nightMode: true
    })
  })

  test('setNightMode delegates to saveSettings', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().setNightMode(true)

    expect(projection.settings.save).toHaveBeenCalledWith({ nightMode: true })
    expect(projection.ipc.sendCommand).toHaveBeenCalledWith('enableNightMode')
  })

  test('saveSettings updates store optimistically, persists patch and refreshes from main', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            audioVolume: 0.1,
            micType: 2,
            nightMode: true
          }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    await useLiviStore.getState().saveSettings({
      audioVolume: 0.1,
      micType: 2,
      nightMode: true
    })

    const state = useLiviStore.getState()

    expect(projection.settings.save).toHaveBeenCalledWith({
      audioVolume: 0.1,
      micType: 2,
      nightMode: true
    })

    expect(projection.ipc.sendCommand).toHaveBeenCalledWith('phoneMic')
    expect(projection.ipc.sendCommand).toHaveBeenCalledWith('enableNightMode')
    expect(state.audioVolume).toBe(0.1)
    expect(state.settings).toEqual({
      ...baseSettings,
      audioVolume: 0.1,
      micType: 2,
      nightMode: true
    })
  })

  test('saveSettings derives telemetryEnabled from telemetryDashboards', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            telemetryDashboards: [{ enabled: true }]
          }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    await useLiviStore.getState().saveSettings({
      telemetryDashboards: [{ enabled: true }] as ExtraConfig['telemetryDashboards']
    })

    expect(projection.settings.save).toHaveBeenCalledWith({
      telemetryDashboards: [{ enabled: true }],
      telemetryEnabled: true
    })
  })

  test('setAudioVolume/setNavVolume/setVoiceAssistantVolume/setCallVolume update state and persist', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().setAudioVolume(0.11)
    useLiviStore.getState().setNavVolume(0.22)
    useLiviStore.getState().setVoiceAssistantVolume(0.33)
    useLiviStore.getState().setCallVolume(0.44)

    await Promise.resolve()
    await Promise.resolve()

    const state = useLiviStore.getState()
    expect(state.audioVolume).toBe(0.11)
    expect(state.navVolume).toBe(0.22)
    expect(state.voiceAssistantVolume).toBe(0.33)
    expect(state.callVolume).toBe(0.44)

    expect(projection.settings.save).toHaveBeenCalledWith({ audioVolume: 0.11 })
    expect(projection.settings.save).toHaveBeenCalledWith({ navVolume: 0.22 })
    expect(projection.settings.save).toHaveBeenCalledWith({ voiceAssistantVolume: 0.33 })
    expect(projection.settings.save).toHaveBeenCalledWith({ callVolume: 0.44 })
  })

  test('setBluetoothPairedList parses raw device list', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore
      .getState()
      .setBluetoothPairedList('AA:BB:CC:DD:EE:FFPhone A\n11:22:33:44:55:66Phone B\n\0')

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
      { mac: '11:22:33:44:55:66', name: 'Phone B' }
    ])
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(false)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })

  test('buildBluetoothPairedListText reconstructs payload from devices', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ]
    })

    expect(useLiviStore.getState().buildBluetoothPairedListText()).toBe(
      'AA:BB:CC:DD:EE:FFPhone A\n11:22:33:44:55:66Phone B\n'
    )
  })

  test('applyBluetoothPairedList sends list to ipc and triggers usb reset when restart is needed', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        setBluetoothPairedList: jest.fn().mockResolvedValue({ ok: true })
      },
      usb: {
        forceReset: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [{ mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }],
      bluetoothPairedDirty: true,
      bluetoothPairedDeleteNeedsRestart: true
    })

    const ok = await useLiviStore.getState().applyBluetoothPairedList()

    expect(ok).toBe(true)
    expect(projection.ipc.setBluetoothPairedList).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FFPhone A\n')
    expect(projection.usb.forceReset).toHaveBeenCalledTimes(1)
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(false)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })

  test('applyBluetoothPairedList returns false when ipc api is missing', async () => {
    const { useLiviStore } = loadFreshStore()

    const ok = await useLiviStore.getState().applyBluetoothPairedList()
    expect(ok).toBe(false)
  })

  test('setNegotiatedResolution, setDeviceInfo, setDongleInfo, setAudioInfo and setPcmData update store', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const pcm = new Float32Array([0.1, 0.2])

    useLiviStore.getState().setNegotiatedResolution(800, 480)
    useLiviStore.getState().setDeviceInfo({
      vendorId: 4660,
      productId: 22136,
      usbFwVersion: ' 1.2.3 '
    })
    useLiviStore.getState().setDongleInfo({
      dongleFwVersion: ' 2.0.0 ',
      boxInfo: { a: 1 }
    })
    useLiviStore.getState().setDongleInfo({
      boxInfo: { b: 2 }
    })
    useLiviStore.getState().setAudioInfo({
      codec: 'aac',
      sampleRate: 48000,
      channels: 2,
      bitDepth: 16
    })
    useLiviStore.getState().setPcmData(pcm)

    const state = useLiviStore.getState()
    expect(state.negotiatedWidth).toBe(800)
    expect(state.negotiatedHeight).toBe(480)
    expect(state.vendorId).toBe(4660)
    expect(state.productId).toBe(22136)
    expect(state.usbFwVersion).toBe('1.2.3')
    expect(state.dongleFwVersion).toBe('2.0.0')
    expect(state.boxInfo).toEqual({ a: 1, b: 2 })
    expect(state.audioCodec).toBe('aac')
    expect(state.audioSampleRate).toBe(48000)
    expect(state.audioChannels).toBe(2)
    expect(state.audioBitDepth).toBe(16)
    expect(state.audioPcmData).toBe(pcm)
  })

  test('resetInfo clears volatile info fields', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      negotiatedWidth: 800,
      negotiatedHeight: 480,
      vendorId: 1,
      productId: 2,
      usbFwVersion: '1.0',
      audioCodec: 'aac',
      audioSampleRate: 48000,
      audioChannels: 2,
      audioBitDepth: 16,
      audioPcmData: new Float32Array([1])
    })

    useLiviStore.getState().resetInfo()

    expect(useLiviStore.getState()).toEqual(
      expect.objectContaining({
        negotiatedWidth: null,
        negotiatedHeight: null,
        vendorId: null,
        productId: null,
        usbFwVersion: null,
        audioCodec: null,
        audioSampleRate: null,
        audioChannels: null,
        audioBitDepth: null,
        audioPcmData: null
      })
    )
  })

  test('telemetry onTelemetry handler persists incoming nightMode changes', async () => {
    let telemetryHandler: ((payload: unknown) => void) | undefined

    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        onTelemetry: jest.fn((handler) => {
          telemetryHandler = handler
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    telemetryHandler?.({ nightMode: true })
    await Promise.resolve()
    await Promise.resolve()

    expect(projection.settings.save).toHaveBeenCalledWith({ nightMode: true })
  })

  test('status store setters update status flags', () => {
    const { useStatusStore } = loadFreshStore()

    useStatusStore.getState().setCameraFound(true)
    useStatusStore.getState().setDongleConnected(true)
    useStatusStore.getState().setStreaming(true)
    useStatusStore.getState().setReverse(true)
    useStatusStore.getState().setLights(true)

    expect(useStatusStore.getState()).toEqual(
      expect.objectContaining({
        cameraFound: true,
        isDongleConnected: true,
        isStreaming: true,
        reverse: true,
        lights: true
      })
    )
  })

  test('getSettings keeps defaults when projection settings api is missing', async () => {
    const { useLiviStore } = loadFreshStore({
      ipc: {
        setVolume: jest.fn(),
        setBluetoothPairedList: jest.fn(),
        sendCommand: jest.fn(),
        onTelemetry: jest.fn(),
        offTelemetry: jest.fn()
      },
      usb: {
        forceReset: jest.fn()
      }
    })

    await useLiviStore.getState().getSettings()

    const state = useLiviStore.getState()
    expect(state.settings).toBeNull()
    expect(state.audioVolume).toBe(0.95)
    expect(state.navVolume).toBe(0.95)
    expect(state.voiceAssistantVolume).toBe(0.95)
    expect(state.callVolume).toBe(0.95)
  })

  test('getSettings swallows settings.get errors and keeps state stable', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockRejectedValue(new Error('get failed'))
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await Promise.resolve()
    await Promise.resolve()

    expect(projection.settings.get).toHaveBeenCalled()
    expect(useLiviStore.getState().settings).toBeNull()
    expect(warnSpy).toHaveBeenCalledWith('settings-get IPC failed', expect.any(Error))
  })

  test('saveSettings swallows settings.save errors after optimistic update', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            audioVolume: 0.12
          }),
        save: jest.fn().mockRejectedValue(new Error('save failed'))
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ audioVolume: 0.12 })

    expect(projection.settings.save).toHaveBeenCalledWith({ audioVolume: 0.12 })
    expect(warnSpy).toHaveBeenCalledWith('settings-save IPC failed', expect.any(Error))
  })

  test('setAudioVolume clamps outgoing ipc volume to 0..1', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({ ...baseSettings, audioVolume: 2 })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ audioVolume: 2 })

    expect(projection.ipc.setVolume).toHaveBeenCalledWith('music', 1)
  })

  test('setNavVolume clamps negative outgoing ipc volume to 0', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({ ...baseSettings, navVolume: -1 })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ navVolume: -1 })

    expect(projection.ipc.setVolume).toHaveBeenCalledWith('nav', 0)
  })

  test('saveSettings sends default mic command for unsupported mic type', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            micType: 99
          }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ micType: 99 as never })

    expect(projection.ipc.sendCommand).toHaveBeenCalledWith('mic')
  })

  test('saveSettings sends disableNightMode for false', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce({ ...baseSettings, nightMode: true })
          .mockResolvedValueOnce({ ...baseSettings, nightMode: false }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ nightMode: false })

    expect(projection.ipc.sendCommand).toHaveBeenCalledWith('disableNightMode')
  })

  test('applyBluetoothPairedList returns false when ipc call throws', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        setBluetoothPairedList: jest.fn().mockRejectedValue(new Error('bt failed'))
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [{ mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }],
      bluetoothPairedDirty: true,
      bluetoothPairedDeleteNeedsRestart: true
    })

    const ok = await useLiviStore.getState().applyBluetoothPairedList()

    expect(ok).toBe(false)
    expect(warnSpy).toHaveBeenCalledWith('[BT] applyBluetoothPairedList failed', expect.any(Error))
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(true)
  })

  test('setDongleInfo keeps previous fw version when next fw version is blank', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().setDongleInfo({
      dongleFwVersion: '2.0.0',
      boxInfo: { a: 1 }
    })

    useLiviStore.getState().setDongleInfo({
      dongleFwVersion: '   ',
      boxInfo: null
    })

    expect(useLiviStore.getState().dongleFwVersion).toBe('2.0.0')
    expect(useLiviStore.getState().boxInfo).toEqual({ a: 1 })
  })

  test('setDongleInfo accepts non-object boxInfo when no previous value exists and keeps existing non-object value', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().setDongleInfo({
      boxInfo: 'raw-box-info'
    })

    expect(useLiviStore.getState().boxInfo).toBe('raw-box-info')

    useLiviStore.getState().setDongleInfo({
      boxInfo: 123
    })

    expect(useLiviStore.getState().boxInfo).toBe('raw-box-info')
  })

  test('telemetry handler ignores non-object payloads', async () => {
    let telemetryHandler: ((payload: unknown) => void) | undefined

    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        onTelemetry: jest.fn((handler) => {
          telemetryHandler = handler
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    telemetryHandler?.(null)
    telemetryHandler?.('oops')
    telemetryHandler?.(123)

    expect(projection.settings.save).not.toHaveBeenCalled()
  })

  test('saveSettings swallows projection setVolume ipc errors', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({ ...baseSettings, audioVolume: 0.7 }),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        setVolume: jest.fn(() => {
          throw new Error('volume failed')
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ audioVolume: 0.7 })

    expect(warnSpy).toHaveBeenCalledWith('projection-set-volume IPC failed', expect.any(Error))
  })

  test('saveSettings swallows projection mic ipc errors', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({ ...baseSettings, micType: 1 }),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        sendCommand: jest.fn(() => {
          throw new Error('mic failed')
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ micType: 1 })

    expect(warnSpy).toHaveBeenCalledWith('projection-set-mic IPC failed', expect.any(Error))
  })

  test('saveSettings swallows projection night mode ipc errors', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({ ...baseSettings, nightMode: true }),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        sendCommand: jest.fn(() => {
          throw new Error('night failed')
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ nightMode: true })

    expect(warnSpy).toHaveBeenCalledWith('projection-set-night-mode IPC failed', expect.any(Error))
  })

  test('init applies live settings updates from settings.onUpdate', async () => {
    let onUpdateHandler: ((event: unknown, settings: ExtraConfig) => void) | undefined

    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        onUpdate: jest.fn((cb) => {
          onUpdateHandler = cb
          return () => {}
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    onUpdateHandler?.(undefined, {
      ...baseSettings,
      audioVolume: 0.25,
      navVolume: 0.35,
      voiceAssistantVolume: 0.45,
      callVolume: 0.55
    } as ExtraConfig)

    const state = useLiviStore.getState()
    expect(state.audioVolume).toBe(0.25)
    expect(state.navVolume).toBe(0.35)
    expect(state.voiceAssistantVolume).toBe(0.45)
    expect(state.callVolume).toBe(0.55)

    expect(projection.ipc.setVolume).toHaveBeenCalledWith('music', 0.25)
    expect(projection.ipc.setVolume).toHaveBeenCalledWith('nav', 0.35)
    expect(projection.ipc.setVolume).toHaveBeenCalledWith('voiceAssistant', 0.45)
    expect(projection.ipc.setVolume).toHaveBeenCalledWith('call', 0.55)
  })

  test('saveSettings does not send mic command when ipc.sendCommand is missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            micType: 1
          }),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        sendCommand: undefined
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ micType: 1 })

    expect(projection.settings.save).toHaveBeenCalledWith({ micType: 1 })
  })

  test('saveSettings does not send night mode command when ipc.sendCommand is missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            nightMode: true
          }),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        sendCommand: undefined
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ nightMode: true })

    expect(projection.settings.save).toHaveBeenCalledWith({ nightMode: true })
  })

  test('saveSettings does not send volume when ipc.setVolume is missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            audioVolume: 0.42
          }),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        setVolume: undefined
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ audioVolume: 0.42 })

    expect(projection.settings.save).toHaveBeenCalledWith({ audioVolume: 0.42 })
  })

  test('applyBluetoothPairedList succeeds without usb reset when restart is not needed', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        setBluetoothPairedList: jest.fn().mockResolvedValue({ ok: true })
      },
      usb: {
        forceReset: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [{ mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }],
      bluetoothPairedDirty: true,
      bluetoothPairedDeleteNeedsRestart: false
    })

    const ok = await useLiviStore.getState().applyBluetoothPairedList()

    expect(ok).toBe(true)
    expect(projection.ipc.setBluetoothPairedList).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FFPhone A\n')
    expect(projection.usb.forceReset).not.toHaveBeenCalled()
  })

  test('init keeps settings null when projection settings.get is missing', async () => {
    const { useLiviStore } = loadFreshStore({
      settings: {
        get: undefined
      }
    })

    await Promise.resolve()
    await Promise.resolve()

    expect(useLiviStore.getState().settings).toBeNull()
  })

  test('init applies fallback derived audio values when settings fields are missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue({
          ...baseSettings,
          audioVolume: undefined,
          navVolume: undefined,
          voiceAssistantVolume: undefined,
          callVolume: undefined,
          visualAudioDelayMs: undefined
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const state = useLiviStore.getState()
    expect(state.audioVolume).toBe(1)
    expect(state.navVolume).toBe(0.5)
    expect(state.voiceAssistantVolume).toBe(0.5)
    expect(state.callVolume).toBe(1)
    expect(state.visualAudioDelayMs).toBe(120)
  })

  test('init does nothing when called a second time', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().init()

    expect(projection.settings.get).toHaveBeenCalledTimes(1)
  })

  test('setBluetoothPairedList ignores clearly invalid and empty bluetooth lines', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore
      .getState()
      .setBluetoothPairedList(
        [
          '',
          'invalid-line',
          'AABBCCDDEEFFNoColons',
          'short',
          'AA:BB:CC:DD:EE:FFValid Device',
          '11:22:33:44:55:66 Another Device',
          '\0'
        ].join('\n')
      )

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: 'AA:BB:CC:DD:EE:FF', name: 'Valid Device' },
      { mac: '11:22:33:44:55:66', name: 'Another Device' }
    ])
  })

  test('forgetBluetoothPairedDevice does not require restart when deleted device is not connected', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        forgetBluetoothPairedDevice: jest.fn().mockResolvedValue({ ok: true })
      }
    })
    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: false,
      boxInfo: { btMacAddr: '77:88:99:AA:BB:CC' }
    })

    await useLiviStore.getState().forgetBluetoothPairedDevice('11:22:33:44:55:66')
    expect(projection.ipc.forgetBluetoothPairedDevice).toHaveBeenCalledWith('11:22:33:44:55:66')

    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })

  test('init live update preserves existing restartBaseline', async () => {
    let onUpdateHandler: ((event: unknown, settings: ExtraConfig) => void) | undefined

    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        onUpdate: jest.fn((cb) => {
          onUpdateHandler = cb
          return () => {}
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const existingBaseline = {
      ...baseSettings,
      audioVolume: 0.99
    } as ExtraConfig

    useLiviStore.setState({
      restartBaseline: existingBaseline
    })

    onUpdateHandler?.(undefined, {
      ...baseSettings,
      audioVolume: 0.25
    } as ExtraConfig)

    expect(useLiviStore.getState().restartBaseline).toEqual(existingBaseline)
    expect(useLiviStore.getState().audioVolume).toBe(0.25)
  })

  test('setDeviceInfo normalizes blank usb firmware version to null', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().setDeviceInfo({
      vendorId: 1,
      productId: 2,
      usbFwVersion: '   '
    })

    expect(useLiviStore.getState().vendorId).toBe(1)
    expect(useLiviStore.getState().productId).toBe(2)
    expect(useLiviStore.getState().usbFwVersion).toBeNull()
  })

  test('forgetBluetoothPairedDevice preserves existing restart flag when already true', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        forgetBluetoothPairedDevice: jest.fn().mockResolvedValue({ ok: true })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: true,
      boxInfo: null
    })

    await useLiviStore.getState().forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(projection.ipc.forgetBluetoothPairedDevice).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FF')
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })

  test('applyBluetoothPairedList returns false when ipc responds with ok false', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        setBluetoothPairedList: jest.fn().mockResolvedValue({ ok: false })
      },
      usb: {
        forceReset: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [{ mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }],
      bluetoothPairedDirty: true,
      bluetoothPairedDeleteNeedsRestart: true
    })

    const ok = await useLiviStore.getState().applyBluetoothPairedList()

    expect(ok).toBe(false)
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(true)
    expect(projection.usb.forceReset).not.toHaveBeenCalled()
  })

  test('setDongleInfo replaces previous primitive boxInfo when next boxInfo is an object', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      boxInfo: 'raw-box-info'
    })

    useLiviStore.getState().setDongleInfo({
      boxInfo: { a: 1 }
    })

    expect(useLiviStore.getState().boxInfo).toEqual({ a: 1 })
  })

  test('setBluetoothPairedList trims trailing null bytes from raw list', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().setBluetoothPairedList('AA:BB:CC:DD:EE:FFPhone A\n\0\0\0')

    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('AA:BB:CC:DD:EE:FFPhone A\n')
  })

  test('getSettings returns early when settings api resolves null', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(null)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await Promise.resolve()
    await useLiviStore.getState().getSettings()

    expect(useLiviStore.getState().settings).toBeNull()
    expect(useLiviStore.getState().audioVolume).toBe(0.95)
    expect(useLiviStore.getState().navVolume).toBe(0.95)
    expect(useLiviStore.getState().voiceAssistantVolume).toBe(0.95)
    expect(useLiviStore.getState().callVolume).toBe(0.95)
  })

  test('saveSettings persists and refreshes even when current settings are null', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue({
          ...baseSettings,
          audioVolume: 0.66
        }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    useLiviStore.setState({
      settings: null
    })

    await useLiviStore.getState().saveSettings({ audioVolume: 0.66 })

    expect(projection.settings.save).toHaveBeenCalledWith({ audioVolume: 0.66 })
    expect(useLiviStore.getState().settings).toEqual({
      ...baseSettings,
      audioVolume: 0.66
    })
    expect(useLiviStore.getState().audioVolume).toBe(0.66)
  })

  test('init skips settings.onUpdate registration when handler is missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        onUpdate: undefined
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    expect(useLiviStore.getState().settings).toEqual(baseSettings)
  })

  test('init skips telemetry registration when ipc.onTelemetry is missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        onTelemetry: undefined
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    expect(useLiviStore.getState().settings).toEqual(baseSettings)
  })

  test('forgetBluetoothPairedDevice handles non-string btMacAddr without restart requirement', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        forgetBluetoothPairedDevice: jest.fn().mockResolvedValue({ ok: true })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: false,
      boxInfo: { btMacAddr: 12345 }
    })

    await useLiviStore.getState().forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')
    expect(projection.ipc.forgetBluetoothPairedDevice).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FF')

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: '11:22:33:44:55:66', name: 'Phone B' }
    ])
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })

  test('setDongleInfo keeps previous object boxInfo when next boxInfo is primitive', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      boxInfo: { a: 1 }
    })

    useLiviStore.getState().setDongleInfo({
      boxInfo: 'primitive-box-info'
    })

    expect(useLiviStore.getState().boxInfo).toEqual({ a: 1 })
  })

  test('connectBluetoothPairedDevice returns false when ipc api is missing', async () => {
    const { useLiviStore } = loadFreshStore()

    const ok = await useLiviStore.getState().connectBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(false)
  })

  test('connectBluetoothPairedDevice returns true when ipc responds with ok true', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        connectBluetoothPairedDevice: jest.fn().mockResolvedValue({ ok: true })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const ok = await useLiviStore.getState().connectBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(true)
    expect(projection.ipc.connectBluetoothPairedDevice).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FF')
  })

  test('connectBluetoothPairedDevice returns false when ipc responds with ok false', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        connectBluetoothPairedDevice: jest.fn().mockResolvedValue({ ok: false })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const ok = await useLiviStore.getState().connectBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(false)
    expect(projection.ipc.connectBluetoothPairedDevice).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FF')
  })

  test('connectBluetoothPairedDevice returns false and warns when ipc throws', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        connectBluetoothPairedDevice: jest.fn().mockRejectedValue(new Error('connect failed'))
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const ok = await useLiviStore.getState().connectBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(false)
    expect(warnSpy).toHaveBeenCalledWith(
      '[BT] connectBluetoothPairedDevice failed',
      expect.any(Error)
    )
  })

  test('forgetBluetoothPairedDevice returns false and warns when ipc throws', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        forgetBluetoothPairedDevice: jest.fn().mockRejectedValue(new Error('forget failed'))
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const ok = await useLiviStore.getState().forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(false)
    expect(warnSpy).toHaveBeenCalledWith(
      '[BT] forgetBluetoothPairedDevice failed',
      expect.any(Error)
    )
  })

  test('markRestartBaseline does nothing when settings are null', () => {
    const { useLiviStore } = loadFreshStore()

    useLiviStore.setState({
      settings: null,
      restartBaseline: null
    })

    useLiviStore.getState().markRestartBaseline()

    expect(useLiviStore.getState().restartBaseline).toBeNull()
  })

  test('telemetry handler ignores object payloads with non-boolean nightMode', async () => {
    let telemetryHandler: ((payload: unknown) => void) | undefined

    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        save: jest.fn().mockResolvedValue(undefined)
      },
      ipc: {
        onTelemetry: jest.fn((handler) => {
          telemetryHandler = handler
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    telemetryHandler?.({ nightMode: 'yes' })
    telemetryHandler?.({ nightMode: 1 })
    telemetryHandler?.({ other: true })

    expect(projection.settings.save).not.toHaveBeenCalled()
  })

  test('saveSettings derives telemetryEnabled false when all telemetry dashboards are disabled', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            telemetryDashboards: [{ enabled: false }]
          }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    await useLiviStore.getState().saveSettings({
      telemetryDashboards: [{ enabled: false }] as ExtraConfig['telemetryDashboards']
    })

    expect(projection.settings.save).toHaveBeenCalledWith({
      telemetryDashboards: [{ enabled: false }],
      telemetryEnabled: false
    })
  })

  test('forgetBluetoothPairedDevice returns false when ipc api is missing', async () => {
    const { useLiviStore } = loadFreshStore()

    const ok = await useLiviStore.getState().forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(false)
  })

  test('forgetBluetoothPairedDevice returns false when ipc responds with ok false', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        forgetBluetoothPairedDevice: jest.fn().mockResolvedValue({ ok: false })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [{ mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }]
    })

    const ok = await useLiviStore.getState().forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(false)
    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }
    ])
  })

  test('forgetBluetoothPairedDevice treats non-object ipc response as success', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        forgetBluetoothPairedDevice: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ]
    })

    const ok = await useLiviStore.getState().forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: '11:22:33:44:55:66', name: 'Phone B' }
    ])
  })

  test('connectBluetoothPairedDevice treats non-object ipc response as success', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        connectBluetoothPairedDevice: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    const ok = await useLiviStore.getState().connectBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')

    expect(ok).toBe(true)
    expect(projection.ipc.connectBluetoothPairedDevice).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FF')
  })

  test('applyBluetoothPairedList succeeds when restart is needed but usb api is missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      },
      ipc: {
        setBluetoothPairedList: jest.fn().mockResolvedValue({ ok: true })
      },
      usb: {
        forceReset: undefined
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [{ mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }],
      bluetoothPairedDirty: true,
      bluetoothPairedDeleteNeedsRestart: true
    })

    const ok = await useLiviStore.getState().applyBluetoothPairedList()

    expect(ok).toBe(true)
    expect(projection.ipc.setBluetoothPairedList).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FFPhone A\n')
  })

  test('saveSettings returns after optimistic update when settings.save api is missing', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        save: undefined
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)
    await useLiviStore.getState().saveSettings({ audioVolume: 0.8 })

    expect(useLiviStore.getState().audioVolume).toBe(0.8)
  })

  test('saveSettings persists telemetry dashboards even when current settings are null', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue({
          ...baseSettings,
          telemetryDashboards: [{ enabled: false }]
        }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    useLiviStore.setState({
      settings: null
    })

    await useLiviStore.getState().saveSettings({
      telemetryDashboards: [{ enabled: false }] as ExtraConfig['telemetryDashboards']
    })

    expect(projection.settings.save).toHaveBeenCalledWith({
      telemetryDashboards: [{ enabled: false }]
    })
  })

  test('getSettings keeps defaults when projection api is completely missing', async () => {
    const { useLiviStore } = loadFreshStore()

    await useLiviStore.getState().getSettings()

    expect(useLiviStore.getState().settings).toBeNull()
    expect(useLiviStore.getState().audioVolume).toBe(0.95)
    expect(useLiviStore.getState().navVolume).toBe(0.95)
    expect(useLiviStore.getState().voiceAssistantVolume).toBe(0.95)
    expect(useLiviStore.getState().callVolume).toBe(0.95)
  })

  test('actions handle missing window gracefully', async () => {
    jest.resetModules()

    const originalWindow = global.window
    // @ts-expect-error test override
    delete global.window

    const { useLiviStore } = require('../store') as typeof import('../store')

    await expect(useLiviStore.getState().getSettings()).resolves.toBeUndefined()
    await expect(
      useLiviStore.getState().connectBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')
    ).resolves.toBe(false)
    await expect(
      useLiviStore.getState().forgetBluetoothPairedDevice('AA:BB:CC:DD:EE:FF')
    ).resolves.toBe(false)
    await expect(useLiviStore.getState().applyBluetoothPairedList()).resolves.toBe(false)

    expect(useLiviStore.getState().settings).toBeNull()

    global.window = originalWindow
  })

  test('saveSettings derives telemetryEnabled false when telemetryDashboards is not an array', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest
          .fn()
          .mockResolvedValueOnce(baseSettings)
          .mockResolvedValueOnce({
            ...baseSettings,
            telemetryDashboards: null
          }),
        save: jest.fn().mockResolvedValue(undefined)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    await useLiviStore.getState().saveSettings({
      telemetryDashboards: null as never
    })

    expect(projection.settings.save).toHaveBeenCalledWith({
      telemetryDashboards: null,
      telemetryEnabled: false
    })
  })

  test('setBluetoothPairedList handles undefined raw input', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().setBluetoothPairedList(undefined as never)

    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('')
    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([])
  })

  test('buildBluetoothPairedListText falls back to empty string for missing device names', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [{ mac: 'AA:BB:CC:DD:EE:FF', name: undefined as never }]
    })

    expect(useLiviStore.getState().buildBluetoothPairedListText()).toBe('AA:BB:CC:DD:EE:FF\n')
  })

  test('init live update sets restartBaseline to incoming settings when baseline is null', async () => {
    let onUpdateHandler: ((event: unknown, settings: ExtraConfig) => void) | undefined

    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings),
        onUpdate: jest.fn((cb) => {
          onUpdateHandler = cb
          return () => {}
        })
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      restartBaseline: null
    })

    const nextSettings = {
      ...baseSettings,
      audioVolume: 0.25
    } as ExtraConfig

    onUpdateHandler?.(undefined, nextSettings)

    expect(useLiviStore.getState().settings).toEqual(nextSettings)
    expect(useLiviStore.getState().restartBaseline).toEqual(nextSettings)
  })

  test('setDongleInfo replaces null boxInfo with incoming object', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      boxInfo: null
    })

    useLiviStore.getState().setDongleInfo({
      boxInfo: { a: 1 }
    })

    expect(useLiviStore.getState().boxInfo).toEqual({ a: 1 })
  })

  test('setBluetoothPairedList normalizes undefined raw input to an empty string', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)

    await waitForStoreSettings(useLiviStore)

    useLiviStore.getState().setBluetoothPairedList(undefined as never)

    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('')
    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([])
  })

  test('removeBluetoothPairedDeviceLocal removes non-connected device without restart requirement', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)
    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: false,
      boxInfo: { btMacAddr: '77:88:99:AA:BB:CC' }
    })

    useLiviStore.getState().removeBluetoothPairedDeviceLocal('11:22:33:44:55:66')

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }
    ])
    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('AA:BB:CC:DD:EE:FFPhone A\n')
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })

  test('removeBluetoothPairedDeviceLocal marks restart required when connected device is removed', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)
    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: false,
      boxInfo: { btMacAddr: ' aa:bb:cc:dd:ee:ff ' }
    })

    useLiviStore.getState().removeBluetoothPairedDeviceLocal('AA:BB:CC:DD:EE:FF')

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: '11:22:33:44:55:66', name: 'Phone B' }
    ])
    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('11:22:33:44:55:66Phone B\n')
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(true)
  })

  test('removeBluetoothPairedDeviceLocal preserves existing restart flag when already true', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)
    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: true,
      boxInfo: { btMacAddr: '77:88:99:AA:BB:CC' }
    })

    useLiviStore.getState().removeBluetoothPairedDeviceLocal('11:22:33:44:55:66')

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' }
    ])
    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('AA:BB:CC:DD:EE:FFPhone A\n')
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(true)
  })

  test('removeBluetoothPairedDeviceLocal handles non-string btMacAddr without restart requirement', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)
    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: false,
      boxInfo: { btMacAddr: 12345 }
    })

    useLiviStore.getState().removeBluetoothPairedDeviceLocal('AA:BB:CC:DD:EE:FF')

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: '11:22:33:44:55:66', name: 'Phone B' }
    ])
    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('11:22:33:44:55:66Phone B\n')
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })

  test('removeBluetoothPairedDeviceLocal works without boxInfo', async () => {
    const projection = makeProjectionApi({
      settings: {
        get: jest.fn().mockResolvedValue(baseSettings)
      }
    })

    const { useLiviStore } = loadFreshStore(projection)
    await waitForStoreSettings(useLiviStore)

    useLiviStore.setState({
      bluetoothPairedDevices: [
        { mac: 'AA:BB:CC:DD:EE:FF', name: 'Phone A' },
        { mac: '11:22:33:44:55:66', name: 'Phone B' }
      ],
      bluetoothPairedDeleteNeedsRestart: false,
      boxInfo: null
    })

    useLiviStore.getState().removeBluetoothPairedDeviceLocal('AA:BB:CC:DD:EE:FF')

    expect(useLiviStore.getState().bluetoothPairedDevices).toEqual([
      { mac: '11:22:33:44:55:66', name: 'Phone B' }
    ])
    expect(useLiviStore.getState().bluetoothPairedListRaw).toBe('11:22:33:44:55:66Phone B\n')
    expect(useLiviStore.getState().bluetoothPairedDirty).toBe(true)
    expect(useLiviStore.getState().bluetoothPairedDeleteNeedsRestart).toBe(false)
  })
})
