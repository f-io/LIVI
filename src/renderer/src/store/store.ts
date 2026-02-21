import { create } from 'zustand'
import { ExtraConfig } from '../../../main/Globals'

type VolumeStreamKey = 'music' | 'nav' | 'siri' | 'call'

type CarplaySettingsApi = {
  get?: () => Promise<ExtraConfig>
  save?: (settings: Partial<ExtraConfig>) => Promise<void>
  onUpdate?: (cb: (event: unknown, settings: ExtraConfig) => void) => () => void
}

type CarplayUsbApi = {
  forceReset?: () => Promise<void> | void
}

type CarplayIpcApi = {
  setVolume?: (stream: VolumeStreamKey, volume: number) => void
}

type CarplayApi = {
  settings?: CarplaySettingsApi
  usb?: CarplayUsbApi
  ipc?: CarplayIpcApi
}

const getCarplayApi = () => {
  if (typeof window === 'undefined') return null
  const w = window as unknown as { carplay?: CarplayApi }
  return w.carplay ?? null
}

const clamp01 = (v: number) => Math.max(0, Math.min(1, v))

const sendCarplayVolume = (stream: VolumeStreamKey, volume: number) => {
  const api = getCarplayApi()
  if (!api?.ipc?.setVolume) return
  try {
    api.ipc.setVolume(stream, clamp01(volume))
  } catch (err) {
    console.warn('carplay-set-volume IPC failed', err)
  }
}

const saveSettingsIpc = async (patch: Partial<ExtraConfig>) => {
  const api = getCarplayApi()
  if (!api?.settings?.save) return
  try {
    await api.settings.save(patch)
  } catch (err) {
    console.warn('settings-save IPC failed', err)
  }
}

const getSettingsIpc = async (): Promise<ExtraConfig | null> => {
  const api = getCarplayApi()
  if (!api?.settings?.get) return null
  try {
    return await api.settings.get()
  } catch (err) {
    console.warn('settings-get IPC failed', err)
    return null
  }
}

const applyDerivedFromSettings = (s: ExtraConfig) => {
  const audioVolume = s.audioVolume ?? 1.0
  const navVolume = s.navVolume ?? 0.5
  const siriVolume = s.siriVolume ?? 0.5
  const callVolume = s.callVolume ?? 1.0
  const visualAudioDelayMs = s.visualAudioDelayMs ?? 120

  return { audioVolume, navVolume, siriVolume, callVolume, visualAudioDelayMs }
}

const deriveTelemetryEnabled = (cfg: ExtraConfig): boolean => {
  const d = cfg.telemetryDashboards
  if (!Array.isArray(d) || d.length === 0) return false
  return d.some((x) => x.enabled)
}

// Carplay Store
export interface CarplayStore {
  // Full app config (from main, includes defaults)
  settings: ExtraConfig | null

  // Used by "requires restart" logic
  restartBaseline: ExtraConfig | null
  markRestartBaseline: () => void

  // Bootstrapping
  init: () => void
  getSettings: () => Promise<void>

  // Save patches (main merges them into config.json)
  saveSettings: (patch: Partial<ExtraConfig>) => Promise<void>

  // Display resolution
  negotiatedWidth: number | null
  negotiatedHeight: number | null
  setNegotiatedResolution: (width: number, height: number) => void

  // USB descriptor
  vendorId: number | null
  productId: number | null
  usbFwVersion: string | null
  setDeviceInfo: (info: { vendorId: number; productId: number; usbFwVersion: string }) => void

  // USB dongle info
  dongleFwVersion: string | null
  boxInfo: unknown | null
  setDongleInfo: (info: { dongleFwVersion?: string; boxInfo?: unknown }) => void

  // Audio metadata
  audioCodec: string | null
  audioSampleRate: number | null
  audioChannels: number | null
  audioBitDepth: number | null
  setAudioInfo: (info: {
    codec: string
    sampleRate: number
    channels: number
    bitDepth: number
  }) => void

  // PCM data for FFT
  audioPcmData: Float32Array | null
  setPcmData: (data: Float32Array) => void

  // Audio settings
  audioVolume: number
  navVolume: number
  siriVolume: number
  callVolume: number
  visualAudioDelayMs: number

  // Audio setters
  setAudioVolume: (volume: number) => void
  setNavVolume: (volume: number) => void
  setSiriVolume: (volume: number) => void
  setCallVolume: (volume: number) => void

  // Reset volatile info
  resetInfo: () => void
}

export const useCarplayStore = create<CarplayStore>((set, get) => {
  // Prevent double init (strict mode / hot reload)
  let didInit = false

  const refreshFromMain = async () => {
    const s = await getSettingsIpc()
    if (!s) return

    const derived = applyDerivedFromSettings(s)
    const baseline = get().restartBaseline

    set({
      settings: s,
      restartBaseline: baseline ?? s,
      ...derived
    })

    // Keep mixer in sync
    sendCarplayVolume('music', derived.audioVolume)
    sendCarplayVolume('nav', derived.navVolume)
    sendCarplayVolume('siri', derived.siriVolume)
    sendCarplayVolume('call', derived.callVolume)
  }

  return {
    settings: null,

    restartBaseline: null,
    markRestartBaseline: () => {
      const s = get().settings
      if (!s) return
      set({ restartBaseline: s })
    },

    init: () => {
      if (didInit) return
      didInit = true

      // initial snapshot
      void refreshFromMain()

      // live sync: main -> renderer
      const api = getCarplayApi()
      if (api?.settings?.onUpdate) {
        api.settings.onUpdate((_evt, s) => {
          const derived = applyDerivedFromSettings(s)
          const baseline = get().restartBaseline

          set({
            settings: s,
            restartBaseline: baseline ?? s,
            ...derived
          })

          // keep mixer in sync
          sendCarplayVolume('music', derived.audioVolume)
          sendCarplayVolume('nav', derived.navVolume)
          sendCarplayVolume('siri', derived.siriVolume)
          sendCarplayVolume('call', derived.callVolume)
        })
      }
    },

    getSettings: async () => {
      await refreshFromMain()
    },

    saveSettings: async (patchArg) => {
      let patch = patchArg

      // Optimistic merge so UI updates instantly
      const prev = get().settings
      if (prev) {
        let merged = { ...prev, ...patch } as ExtraConfig

        if (patch.telemetryDashboards !== undefined) {
          merged = { ...merged, telemetryEnabled: deriveTelemetryEnabled(merged) }
          patch = { ...patch, telemetryEnabled: merged.telemetryEnabled }
        }

        const derived = applyDerivedFromSettings(merged)

        set({ settings: merged, ...derived })

        sendCarplayVolume('music', derived.audioVolume)
        sendCarplayVolume('nav', derived.navVolume)
        sendCarplayVolume('siri', derived.siriVolume)
        sendCarplayVolume('call', derived.callVolume)
      }

      // Persist patch in main
      await saveSettingsIpc(patch)

      // Re-fetch full merged config from main
      await refreshFromMain()
    },

    negotiatedWidth: null,
    negotiatedHeight: null,
    setNegotiatedResolution: (width, height) =>
      set({ negotiatedWidth: width, negotiatedHeight: height }),

    vendorId: null,
    productId: null,
    usbFwVersion: null,
    setDeviceInfo: ({ vendorId, productId, usbFwVersion }) =>
      set({
        vendorId,
        productId,
        usbFwVersion: usbFwVersion?.trim() ? usbFwVersion.trim() : null
      }),

    dongleFwVersion: null,
    boxInfo: null,
    setDongleInfo: ({ dongleFwVersion, boxInfo }) =>
      set((state) => {
        const nextFw =
          typeof dongleFwVersion === 'string' && dongleFwVersion.trim()
            ? dongleFwVersion.trim()
            : null

        const mergeObjects = (a: unknown, b: unknown) => {
          if (!a || typeof a !== 'object') return b
          if (!b || typeof b !== 'object') return a
          return { ...(a as Record<string, unknown>), ...(b as Record<string, unknown>) }
        }

        const nextBox =
          boxInfo == null
            ? state.boxInfo
            : typeof boxInfo === 'object'
              ? mergeObjects(state.boxInfo, boxInfo)
              : (state.boxInfo ?? boxInfo)

        return {
          dongleFwVersion: nextFw ?? state.dongleFwVersion,
          boxInfo: nextBox
        }
      }),

    audioCodec: null,
    audioSampleRate: null,
    audioChannels: null,
    audioBitDepth: null,
    setAudioInfo: ({ codec, sampleRate, channels, bitDepth }) =>
      set({
        audioCodec: codec,
        audioSampleRate: sampleRate,
        audioChannels: channels,
        audioBitDepth: bitDepth
      }),

    audioPcmData: null,
    setPcmData: (data) => set({ audioPcmData: data }),

    // Defaults until first IPC load arrives
    audioVolume: 0.95,
    navVolume: 0.95,
    siriVolume: 0.95,
    callVolume: 0.95,
    visualAudioDelayMs: 120,

    setAudioVolume: (audioVolume) => {
      set({ audioVolume })
      void get().saveSettings({ audioVolume })
    },
    setNavVolume: (navVolume) => {
      set({ navVolume })
      void get().saveSettings({ navVolume })
    },
    setSiriVolume: (siriVolume) => {
      set({ siriVolume })
      void get().saveSettings({ siriVolume })
    },
    setCallVolume: (callVolume) => {
      set({ callVolume })
      void get().saveSettings({ callVolume })
    },

    resetInfo: () =>
      set({
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
  }
})

// Auto-init
useCarplayStore.getState().init()

// Status store
export interface StatusStore {
  reverse: boolean
  lights: boolean
  isDongleConnected: boolean
  isStreaming: boolean
  cameraFound: boolean

  setCameraFound: (found: boolean) => void
  setDongleConnected: (connected: boolean) => void
  setStreaming: (streaming: boolean) => void
  setReverse: (reverse: boolean) => void
  setLights: (lights: boolean) => void
}

export const useStatusStore = create<StatusStore>((set) => ({
  reverse: false,
  lights: false,
  isDongleConnected: false,
  isStreaming: false,
  cameraFound: false,

  setCameraFound: (found) => set({ cameraFound: found }),
  setDongleConnected: (connected) => set({ isDongleConnected: connected }),
  setStreaming: (streaming) => set({ isStreaming: streaming }),
  setReverse: (reverse) => set({ reverse }),
  setLights: (lights) => set({ lights })
}))
