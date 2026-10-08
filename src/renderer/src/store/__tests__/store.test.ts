import type { FromCore, State, ToCore } from '@shared/core/contract'
import type { Config } from '@shared/types'
import { act, renderHook } from '@testing-library/react'

type FakeCore = {
  client?: string
  onMessage?: (msg: FromCore) => void
  onClose?: () => void
  sent: ToCore[]
  connected: boolean
  connects: number
}

type TestWindow = Window & {
  core?: unknown
}

const baseSettings = {
  audioVolume: 0.8,
  navVolume: 0.4,
  voiceAssistantVolume: 0.5,
  callVolume: 0.6,
  visualAudioDelayMs: 120,
  darkMode: false
} as unknown as Config

const testWindow = window as unknown as TestWindow

function installCore(): FakeCore {
  const fake: FakeCore = { sent: [], connected: true, connects: 0 }
  testWindow.core = {
    connect: (client: string, onMessage: (msg: FromCore) => void, onClose?: () => void) => {
      fake.connects += 1
      fake.client = client
      fake.onMessage = onMessage
      fake.onClose = onClose
      return {
        send: (msg: ToCore) => {
          fake.sent.push(msg)
          return fake.connected
        },
        close: vi.fn()
      }
    }
  }
  return fake
}

const state = (config: unknown, main = 'livi'): State =>
  ({
    front: { main, dash: 'livi', aux: 'livi' },
    sessions: { active: null, position: 0, total: 0 },
    nowPlaying: {
      title: null,
      artist: null,
      album: null,
      app: null,
      durationMs: null,
      elapsedMs: null,
      playing: null,
      artwork: null
    },
    navigation: {
      active: null,
      orderType: null,
      roadName: null,
      afterRoadName: null,
      destinationName: null,
      timeToDestination: null,
      distanceToDestination: null,
      remainDistance: null,
      maneuverType: null,
      turnSide: null,
      junctionType: null,
      turnAngle: null,
      eta: null,
      etaText: null,
      appName: null,
      image: null
    },
    system: {
      wifiInterfaces: [],
      btAdapters: [],
      dongle: null,
      linkSpeed: null,
      wifiBands: [],
      wifiChannels: [],
      wifiCountries: [],
      displayModes: [],
      displayModeSettable: false,
      audioSinks: [],
      audioSources: []
    },
    devices: [],
    telemetry: {},
    update: {
      latest: null,
      checking: false,
      checked: false,
      phase: 'idle',
      received: 0,
      total: 0,
      error: null
    },
    config
  }) as unknown as State

const welcome = (fake: FakeCore, config: unknown) =>
  fake.onMessage?.({ type: 'welcome', protocol: 1, version: '0', rev: 0, state: state(config) })

const patch = (fake: FakeCore, rev: number, ops: unknown[]) =>
  fake.onMessage?.({ type: 'patch', rev, ops } as FromCore)

async function loadFreshStore(opts: { core?: boolean } = {}) {
  vi.resetModules()
  testWindow.core = undefined
  const fake = opts.core === false ? null : installCore()
  const store = await import('../store')
  return { ...store, fake: fake as FakeCore }
}

describe('store', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    testWindow.core = undefined
  })

  describe('settings from core', () => {
    test('the welcome brings the config and the derived audio values', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      expect(fake.client).toBe('ui:main')

      welcome(fake, baseSettings)

      const s = useLiviStore.getState()
      expect(s.settings).toEqual(baseSettings)
      expect(s.restartBaseline).toEqual(baseSettings)
      expect(s.audioVolume).toBe(0.8)
      expect(s.navVolume).toBe(0.4)
      expect(s.voiceAssistantVolume).toBe(0.5)
      expect(s.callVolume).toBe(0.6)
    })

    test('a secondary window names its role', async () => {
      window.history.replaceState({}, '', '/?role=dash')
      const { fake } = await loadFreshStore()
      expect(fake.client).toBe('ui:dash')
      window.history.replaceState({}, '', '/')
    })

    test('missing audio fields fall back to their defaults', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, {})

      const s = useLiviStore.getState()
      expect(s.audioVolume).toBe(1.0)
      expect(s.navVolume).toBe(0.5)
      expect(s.voiceAssistantVolume).toBe(0.5)
      expect(s.callVolume).toBe(1.0)
    })

    test('during a session a config patch keeps the restart baseline', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      patch(fake, 1, [{ op: 'set', path: ['sessions', 'total'], value: 1 }])

      patch(fake, 2, [{ op: 'set', path: ['config', 'darkMode'], value: true }])

      expect(useLiviStore.getState().settings?.darkMode).toBe(true)
      expect(useLiviStore.getState().restartBaseline).toEqual(baseSettings)
    })

    test('during a session without a baseline the incoming config becomes it', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      patch(fake, 1, [{ op: 'set', path: ['sessions', 'total'], value: 1 }])
      useLiviStore.setState({ restartBaseline: null })

      patch(fake, 2, [{ op: 'set', path: ['config', 'darkMode'], value: true }])

      expect(useLiviStore.getState().restartBaseline?.darkMode).toBe(true)
    })

    test('with no phone connected the baseline follows the config', async () => {
      const { useLiviStore, useSessionsOpen, fake } = await loadFreshStore()
      welcome(fake, baseSettings)

      patch(fake, 1, [{ op: 'set', path: ['config', 'darkMode'], value: true }])

      expect(useLiviStore.getState().restartBaseline?.darkMode).toBe(true)
      expect(renderHook(() => useSessionsOpen()).result.current).toBe(false)
    })

    test('when the last phone leaves the waiting changes count as applied', async () => {
      const { useLiviStore, useSessionsOpen, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      patch(fake, 1, [{ op: 'set', path: ['sessions', 'total'], value: 1 }])
      expect(renderHook(() => useSessionsOpen()).result.current).toBe(true)
      patch(fake, 2, [{ op: 'set', path: ['config', 'darkMode'], value: true }])
      expect(useLiviStore.getState().restartBaseline).toEqual(baseSettings)

      patch(fake, 3, [{ op: 'set', path: ['sessions', 'total'], value: 0 }])

      expect(useLiviStore.getState().restartBaseline?.darkMode).toBe(true)
    })

    test('a patch that leaves the config alone leaves the settings alone', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      const before = useLiviStore.getState().settings

      patch(fake, 1, [{ op: 'set', path: ['front', 'main'], value: 'projection' }])

      expect(useLiviStore.getState().settings).toBe(before)
    })

    test('the phones core holds reach the status store', async () => {
      const { useLiviStore, useStatusStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      expect(useStatusStore.getState().isStreaming).toBe(false)

      patch(fake, 1, [
        { op: 'set', path: ['sessions', 'active'], value: 'carplay' },
        { op: 'set', path: ['sessions', 'position'], value: 1 },
        { op: 'set', path: ['sessions', 'total'], value: 2 }
      ])
      const sessions = useLiviStore.getState().sessions
      expect(sessions).toEqual({ active: 'carplay', position: 1, total: 2 })
      expect(useStatusStore.getState().activeProtocol).toBe('carplay')
      expect(useStatusStore.getState().isStreaming).toBe(true)

      patch(fake, 2, [{ op: 'set', path: ['config', 'darkMode'], value: true }])
      expect(useLiviStore.getState().sessions).toBe(sessions)

      patch(fake, 3, [
        { op: 'set', path: ['sessions', 'active'], value: null },
        { op: 'set', path: ['sessions', 'position'], value: 0 },
        { op: 'set', path: ['sessions', 'total'], value: 0 }
      ])
      expect(useStatusStore.getState().activeProtocol).toBeNull()
      expect(useStatusStore.getState().isStreaming).toBe(false)
    })

    test('what each screen has in front follows core', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      expect(useLiviStore.getState().front).toBeNull()
      welcome(fake, baseSettings)
      expect(useLiviStore.getState().front?.main).toBe('livi')

      patch(fake, 1, [{ op: 'set', path: ['front', 'main'], value: 'projection' }])
      expect(useLiviStore.getState().front?.main).toBe('projection')
    })

    test('now playing follows core', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      expect(useLiviStore.getState().nowPlaying).toBeNull()
      welcome(fake, baseSettings)
      expect(useLiviStore.getState().nowPlaying?.title).toBeNull()

      patch(fake, 1, [{ op: 'set', path: ['nowPlaying', 'title'], value: 'Song' }])
      expect(useLiviStore.getState().nowPlaying?.title).toBe('Song')
    })

    test('navigation follows core', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      expect(useLiviStore.getState().navigation).toBeNull()
      welcome(fake, baseSettings)
      patch(fake, 1, [{ op: 'set', path: ['navigation', 'active'], value: true }])
      expect(useLiviStore.getState().navigation?.active).toBe(true)
    })

    test('the system lists follow core', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      expect(useLiviStore.getState().system).toBeNull()
      welcome(fake, baseSettings)
      patch(fake, 1, [{ op: 'set', path: ['system', 'wifiInterfaces'], value: ['wlan0'] }])
      expect(useLiviStore.getState().system?.wifiInterfaces).toEqual(['wlan0'])
    })

    test('a new audio device list makes the pickers load again', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      const before = useLiviStore.getState().audioDevicesRevision
      patch(fake, 1, [{ op: 'set', path: ['system', 'wifiChannels'], value: [1] }])
      expect(useLiviStore.getState().audioDevicesRevision).toBe(before)
      const sink = { id: 'out', name: 'Out', isDefault: true, offline: false }
      patch(fake, 2, [{ op: 'set', path: ['system', 'audioSinks'], value: [sink] }])
      expect(useLiviStore.getState().audioDevicesRevision).toBe(before + 1)
      patch(fake, 3, [{ op: 'set', path: ['system', 'audioSources'], value: [sink] }])
      expect(useLiviStore.getState().audioDevicesRevision).toBe(before + 2)
    })

    test('the device list follows core', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      expect(useLiviStore.getState().devices).toEqual([])
      welcome(fake, baseSettings)
      patch(fake, 1, [{ op: 'set', path: ['devices'], value: [{ id: 'a', status: 'active' }] }])
      expect(useLiviStore.getState().devices).toEqual([{ id: 'a', status: 'active' }])
    })

    test('the update follows core', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      expect(useLiviStore.getState().update).toBeNull()
      welcome(fake, baseSettings)
      patch(fake, 1, [{ op: 'set', path: ['update', 'phase'], value: 'download' }])
      expect(useLiviStore.getState().update?.phase).toBe('download')
    })

    test('init runs once', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      useLiviStore.getState().init()
      expect(fake.connects).toBe(1)
    })

    test('without core the defaults stay', async () => {
      const { useLiviStore } = await loadFreshStore({ core: false })
      const s = useLiviStore.getState()
      expect(s.settings).toBeNull()
      expect(s.audioVolume).toBe(0.95)
      expect(s.navVolume).toBe(0.95)
      expect(s.voiceAssistantVolume).toBe(0.95)
      expect(s.callVolume).toBe(0.95)
    })
  })

  describe('saveSettings', () => {
    test('updates at once and asks core to keep it', async () => {
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)

      const saved = useLiviStore.getState().saveSettings({ audioVolume: 0.3 })

      expect(useLiviStore.getState().settings?.audioVolume).toBe(0.3)
      expect(useLiviStore.getState().audioVolume).toBe(0.3)
      expect(fake.sent).toEqual([
        { type: 'action', id: 1, action: { kind: 'setConfig', patch: { audioVolume: 0.3 } } }
      ])
      fake.onMessage?.({ type: 'reply', id: 1 })
      await expect(saved).resolves.toBeUndefined()
    })

    test('a refused change goes back to what core has', async () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)

      const saved = useLiviStore.getState().saveSettings({ audioVolume: 7 } as Partial<Config>)
      fake.onMessage?.({ type: 'reply', id: 1, error: 'out of range' })
      await saved

      expect(warn).toHaveBeenCalledWith('settings not saved', expect.any(Error))
      expect(fake.sent.at(-1)).toEqual({ type: 'resync' })
    })

    test('a lost core fails the change and asks again later', async () => {
      vi.spyOn(console, 'warn').mockImplementation(() => {})
      const { useLiviStore, fake } = await loadFreshStore()
      welcome(fake, baseSettings)

      const saved = useLiviStore.getState().saveSettings({ darkMode: true })
      fake.onClose?.()
      await saved

      expect(fake.sent.at(-1)).toEqual({ type: 'resync' })
    })

    test('goes to core even before the first config arrived', async () => {
      const { useLiviStore, fake } = await loadFreshStore()

      const saved = useLiviStore.getState().saveSettings({ darkMode: true })

      expect(useLiviStore.getState().settings).toBeNull()
      expect(fake.sent).toHaveLength(1)
      fake.onMessage?.({ type: 'reply', id: 1 })
      await saved
    })

    test('without core only the optimistic update happens', async () => {
      const { useLiviStore } = await loadFreshStore({ core: false })
      useLiviStore.setState({ settings: baseSettings })

      await expect(
        useLiviStore.getState().saveSettings({ darkMode: true })
      ).resolves.toBeUndefined()

      expect(useLiviStore.getState().settings?.darkMode).toBe(true)
    })
  })

  describe('core actions', () => {
    test('coreAction goes to core and waits for the reply', async () => {
      const { coreAction, fake } = await loadFreshStore()
      const done = coreAction({ kind: 'quit' })
      expect(fake.sent).toEqual([{ type: 'action', id: 1, action: { kind: 'quit' } }])
      fake.onMessage?.({ type: 'reply', id: 1 })
      await expect(done).resolves.toBeUndefined()
    })

    test('coreAction fails without core', async () => {
      const { coreAction } = await loadFreshStore({ core: false })
      await expect(coreAction({ kind: 'restart' })).rejects.toThrow('core is not connected')
    })

    test('reportPath tells core the route, and is a no-op without core', async () => {
      const { reportPath, fake } = await loadFreshStore()
      reportPath('/media')
      expect(fake.sent).toEqual([{ type: 'path', path: '/media' }])

      const without = await loadFreshStore({ core: false })
      expect(() => without.reportPath('/media')).not.toThrow()
    })

    test('sendInput hands input to core, and is a no-op without core', async () => {
      const { sendInput, fake } = await loadFreshStore()
      const touch = { kind: 'pointer', screen: 'main', points: [] } as const
      sendInput(touch)
      expect(fake.sent).toEqual([{ type: 'input', input: touch }])

      const without = await loadFreshStore({ core: false })
      expect(() => without.sendInput(touch)).not.toThrow()
    })

    test('reportShown tells core what a screen shows, and is a no-op without core', async () => {
      const { reportShown, fake } = await loadFreshStore()
      reportShown('main', 'projection')
      expect(fake.sent).toEqual([
        { type: 'action', id: 1, action: { kind: 'show', screen: 'main', front: 'projection' } }
      ])

      const without = await loadFreshStore({ core: false })
      expect(() => without.reportShown('main', 'livi')).not.toThrow()
    })

    test('a window drawing the spectrum asks for its frames and gets them', async () => {
      const { reportSpectrum, onSpectrum, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      const frames: number[][] = []
      const stop = onSpectrum((bands) => frames.push(bands))
      reportSpectrum(true)
      expect(fake.sent).toEqual([{ type: 'spectrum', on: true }])
      fake.onMessage?.({ type: 'spectrum', bands: [0.5] })
      stop()
      fake.onMessage?.({ type: 'spectrum', bands: [1] })
      expect(frames).toEqual([[0.5]])

      const without = await loadFreshStore({ core: false })
      expect(() => without.reportSpectrum(false)).not.toThrow()
      expect(() => without.onSpectrum(() => {})()).not.toThrow()
    })

    test('a window showing the link speed tells core', async () => {
      const { reportLinkSpeed, fake } = await loadFreshStore()
      welcome(fake, baseSettings)
      reportLinkSpeed(true)
      reportLinkSpeed(false)
      expect(fake.sent).toEqual([
        { type: 'linkSpeed', on: true },
        { type: 'linkSpeed', on: false }
      ])

      const without = await loadFreshStore({ core: false })
      expect(() => without.reportLinkSpeed(true)).not.toThrow()
    })
  })

  test('markRestartBaseline stores the current settings and ignores none', async () => {
    const { useLiviStore, fake } = await loadFreshStore()
    useLiviStore.getState().markRestartBaseline()
    expect(useLiviStore.getState().restartBaseline).toBeNull()

    welcome(fake, baseSettings)
    useLiviStore.setState({ settings: { ...baseSettings, darkMode: true } })
    useLiviStore.getState().markRestartBaseline()
    expect(useLiviStore.getState().restartBaseline?.darkMode).toBe(true)
  })

  test('status store setters update status flags', async () => {
    const { useStatusStore } = await loadFreshStore()

    useStatusStore.getState().setCameraFound(true)
    useStatusStore.getState().setStreaming(true)
    useStatusStore.getState().setReverse(true)
    useStatusStore.getState().setLights(true)

    expect(useStatusStore.getState()).toEqual(
      expect.objectContaining({ cameraFound: true, isStreaming: true, reverse: true, lights: true })
    )
  })

  describe('telemetry', () => {
    async function withTelemetry() {
      const store = await loadFreshStore()
      welcome(store.fake, baseSettings)
      let rev = 0
      const send = (telemetry: Record<string, unknown>) => {
        rev += 1
        patch(store.fake, rev, [{ op: 'set', path: ['telemetry'], value: telemetry }])
      }
      return { ...store, send }
    }

    const welcomeWith = (fake: FakeCore, telemetry: Record<string, unknown>) =>
      fake.onMessage?.({
        type: 'welcome',
        protocol: 1,
        version: '0',
        rev: 0,
        state: { ...state(baseSettings), telemetry } as unknown as State
      })

    test('forwards explicit reverse and lights to the status store', async () => {
      const { useLiviStore, useStatusStore, send } = await withTelemetry()
      send({ reverse: true, lights: true })
      expect(useStatusStore.getState().reverse).toBe(true)
      expect(useStatusStore.getState().lights).toBe(true)
      expect(useLiviStore.getState().telemetry).toEqual({ reverse: true, lights: true })
    })

    test('skips no-op writes when reverse and lights already match', async () => {
      const { useStatusStore, send } = await withTelemetry()
      useStatusStore.getState().setReverse(true)
      useStatusStore.getState().setLights(true)
      const setReverse = vi.spyOn(useStatusStore.getState(), 'setReverse')
      const setLights = vi.spyOn(useStatusStore.getState(), 'setLights')

      send({ reverse: true, lights: true })

      expect(setReverse).not.toHaveBeenCalled()
      expect(setLights).not.toHaveBeenCalled()
    })

    test('derives reverse from the gear', async () => {
      const { useStatusStore, send } = await withTelemetry()
      send({ gear: 'R' })
      expect(useStatusStore.getState().reverse).toBe(true)
      send({ gear: -1 })
      expect(useStatusStore.getState().reverse).toBe(true)
      send({ gear: 3 })
      expect(useStatusStore.getState().reverse).toBe(false)
    })

    test('ignores fields it does not use', async () => {
      const { useStatusStore, fake, send } = await withTelemetry()
      send({ nightMode: true })
      expect(useStatusStore.getState().reverse).toBe(false)
      expect(useStatusStore.getState().lights).toBe(false)
      expect(fake.sent).toEqual([])
    })

    test('a path request is taken when it changes, not again from the kept snapshot', async () => {
      const { useStatusStore, send } = await withTelemetry()
      send({ path: '/camera' })
      expect(useStatusStore.getState().requestedPath).toBe('/camera')
      useStatusStore.getState().clearRequestedPath()
      send({ path: '/camera', speedKph: 5 })
      expect(useStatusStore.getState().requestedPath).toBeNull()
      send({ path: '/media', speedKph: 5 })
      expect(useStatusStore.getState().requestedPath).toBe('/media')
    })

    test('the welcome brings the snapshot', async () => {
      const { useStatusStore, fake } = await loadFreshStore()
      welcomeWith(fake, { reverse: true, lights: true })
      expect(useStatusStore.getState().reverse).toBe(true)
      expect(useStatusStore.getState().lights).toBe(true)
    })

    test('an empty snapshot changes nothing', async () => {
      const { useStatusStore, fake } = await loadFreshStore()
      welcomeWith(fake, {})
      expect(useStatusStore.getState().reverse).toBe(false)
      expect(useStatusStore.getState().lights).toBe(false)
    })
  })

  test('setActiveProtocol flips the status flag and useProjectionActive reflects it', async () => {
    const { useStatusStore, useProjectionActive } = await loadFreshStore()

    const { result } = renderHook(() => useProjectionActive())
    expect(result.current).toBe(false)

    act(() => {
      useStatusStore.getState().setActiveProtocol('androidauto')
    })
    expect(useStatusStore.getState().activeProtocol).toBe('androidauto')
    expect(result.current).toBe(true)

    act(() => {
      useStatusStore.getState().setActiveProtocol(null)
    })
    expect(result.current).toBe(false)
  })

  test('the cluster-dash setter updates state', async () => {
    const { useStatusStore } = await loadFreshStore()

    useStatusStore.getState().setClusterDashActive(true)
    expect(useStatusStore.getState().clusterDashActive).toBe(true)
  })
})
