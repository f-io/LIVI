import { create } from 'zustand'
import { ExtraConfig } from '../../../main/Globals'
import { io } from 'socket.io-client'

const URL = 'http://localhost:4000'

// Socket.IO Setup
const socket = io(URL, {
  transports: ['websocket'],
  reconnection: true,
  reconnectionAttempts: 5,
  reconnectionDelay: 2000
})

socket.on('connect_error', (err) => {
  console.warn('Socket.IO connect_error:', err.message)
})

type VolumeStreamKey = 'music' | 'nav' | 'siri' | 'call'

type CarplayIpcApi = {
  setVolume?: (stream: VolumeStreamKey, volume: number) => void
}

const getCarplayApi = () => {
  if (typeof window === 'undefined') return null
  const w = window as unknown as { carplay?: { ipc?: CarplayIpcApi } }
  return w.carplay ?? null
}

const sendCarplayVolume = (stream: VolumeStreamKey, volume: number) => {
  const api = getCarplayApi()
  if (!api?.ipc?.setVolume) return
  const clamped = Math.max(0, Math.min(1, volume))
  try {
    api.ipc.setVolume(stream, clamped)
  } catch (err) {
    console.warn('carplay-set-volume IPC failed', err)
  }
}

// Carplay Store
export interface CarplayStore {
  // App settings
  settings: ExtraConfig | null
  saveSettings: (settings: ExtraConfig) => void
  getSettings: () => void
  stream: (payload: unknown) => void
  resetInfo: () => void

  restartBaseline: ExtraConfig | null
  markRestartBaseline: () => void

  // Display resolution
  negotiatedWidth: number | null
  negotiatedHeight: number | null

  // USB descriptor
  vendorId: number | null
  productId: number | null
  usbFwVersion: string | null

  // USB dongle info
  dongleFwVersion: string | null
  boxInfo: unknown | null

  // Audio metadata
  audioCodec: string | null
  audioSampleRate: number | null
  audioChannels: number | null
  audioBitDepth: number | null

  // PCM data for FFT
  audioPcmData: Float32Array | null
  setPcmData: (data: Float32Array) => void

  // Audio settings with direct access
  audioVolume: number
  navVolume: number
  siriVolume: number
  callVolume: number

  // Visual-only audio delay (FFT alignment)
  visualAudioDelayMs: number

  // Audio setters
  setAudioVolume: (volume: number) => void
  setNavVolume: (volume: number) => void
  setSiriVolume: (volume: number) => void
  setCallVolume: (volume: number) => void

  // Setters for metadata
  setDeviceInfo: (info: { vendorId: number; productId: number; usbFwVersion: string }) => void
  setDongleInfo: (info: { dongleFwVersion?: string; boxInfo?: unknown }) => void

  setNegotiatedResolution: (width: number, height: number) => void
  setAudioInfo: (info: {
    codec: string
    sampleRate: number
    channels: number
    bitDepth: number
  }) => void
}

export const useCarplayStore = create<CarplayStore>((set, get) => ({
  settings: null,

  restartBaseline: null,
  markRestartBaseline: () => {
    const s = get().settings
    if (!s) return
    set({ restartBaseline: s })
  },

  saveSettings: (settings) => {
    set({ settings })

    const audioVolume = settings.audioVolume ?? 1.0
    const navVolume = settings.navVolume ?? 1.0
    const siriVolume = settings.siriVolume ?? 1.0
    const callVolume = settings.callVolume ?? 1.0
    const visualAudioDelayMs = settings.visualAudioDelayMs ?? 120

    set({
      audioVolume,
      navVolume,
      siriVolume,
      callVolume,
      visualAudioDelayMs
    })

    socket.emit('saveSettings', settings)

    sendCarplayVolume('music', audioVolume)
    sendCarplayVolume('nav', navVolume)
    sendCarplayVolume('siri', siriVolume)
    sendCarplayVolume('call', callVolume)
  },

  getSettings: () => {
    socket.emit('getSettings')
  },

  stream: (payload: unknown) => {
    socket.emit('stream', payload)
  },

  // Reset all stored info
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
    }),

  negotiatedWidth: null,
  negotiatedHeight: null,

  vendorId: null,
  productId: null,
  usbFwVersion: null,
  dongleFwVersion: null,
  boxInfo: null,

  audioCodec: null,
  audioSampleRate: null,
  audioChannels: null,
  audioBitDepth: null,

  audioPcmData: null,
  setPcmData: (data) => set({ audioPcmData: data }),

  // Audio settings with defaults
  audioVolume: 1.0,
  navVolume: 0.5,
  siriVolume: 0.5,
  callVolume: 1.0,

  // Visual delay default
  visualAudioDelayMs: 120,

  // Audio setters
  setAudioVolume: (audioVolume) => {
    set({ audioVolume })
    const { settings, navVolume, siriVolume, callVolume } = get()
    if (settings) {
      const updatedSettings: ExtraConfig = {
        ...settings,
        audioVolume,
        navVolume,
        siriVolume,
        callVolume
      }
      get().saveSettings(updatedSettings)
    }
  },

  setNavVolume: (navVolume) => {
    set({ navVolume })
    const { settings, audioVolume, siriVolume, callVolume } = get()
    if (settings) {
      const updatedSettings: ExtraConfig = {
        ...settings,
        audioVolume,
        navVolume,
        siriVolume,
        callVolume
      }
      get().saveSettings(updatedSettings)
    }
  },

  setSiriVolume: (siriVolume) => {
    set({ siriVolume })
    const { settings, audioVolume, navVolume, callVolume } = get()
    if (settings) {
      const updatedSettings: ExtraConfig = {
        ...settings,
        audioVolume,
        navVolume,
        siriVolume,
        callVolume
      }
      get().saveSettings(updatedSettings)
    }
  },

  setCallVolume: (callVolume) => {
    set({ callVolume })
    const { settings, audioVolume, navVolume, siriVolume } = get()
    if (settings) {
      const updatedSettings: ExtraConfig = {
        ...settings,
        audioVolume,
        navVolume,
        siriVolume,
        callVolume
      }
      get().saveSettings(updatedSettings)
    }
  },

  setDeviceInfo: ({ vendorId, productId, usbFwVersion }) =>
    set(() => ({
      vendorId,
      productId,
      usbFwVersion: usbFwVersion?.trim() ? usbFwVersion.trim() : null
    })),

  setDongleInfo: ({ dongleFwVersion, boxInfo }) =>
    set((state) => {
      const nextFw =
        typeof dongleFwVersion === 'string' && dongleFwVersion.trim()
          ? dongleFwVersion.trim()
          : null

      // Merge objects
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

  setNegotiatedResolution: (width, height) =>
    set({ negotiatedWidth: width, negotiatedHeight: height }),

  setAudioInfo: ({ codec, sampleRate, channels, bitDepth }) =>
    set({
      audioCodec: codec,
      audioSampleRate: sampleRate,
      audioChannels: channels,
      audioBitDepth: bitDepth
    })
}))

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
  setReverse: (reverse: boolean) => set({ reverse }),
  setLights: (lights: boolean) => set({ lights })
}))

// Socket.IO event handlers
socket.on('settings', (settings: ExtraConfig) => {
  const audioVolume = settings.audioVolume ?? 1.0
  const navVolume = settings.navVolume ?? 0.5
  const siriVolume = settings.siriVolume ?? 0.5
  const callVolume = settings.callVolume ?? 1.0
  const visualAudioDelayMs = settings.visualAudioDelayMs ?? 120
  const prevBaseline = useCarplayStore.getState().restartBaseline

  useCarplayStore.setState({
    settings,
    restartBaseline: prevBaseline ?? settings,
    audioVolume,
    navVolume,
    siriVolume,
    callVolume,
    visualAudioDelayMs
  })

  // initial volumes to main AudioMixer
  sendCarplayVolume('music', audioVolume)
  sendCarplayVolume('nav', navVolume)
  sendCarplayVolume('siri', siriVolume)
  sendCarplayVolume('call', callVolume)
})

socket.on('reverse', (reverse: boolean) => {
  useStatusStore.setState({ reverse })
})

socket.on('dongle-status', (connected: boolean) => {
  useStatusStore.setState({ isDongleConnected: connected })
})

socket.on('stream-status', (streaming: boolean) => {
  useStatusStore.setState({ isStreaming: streaming })
})

socket.on('camera-found', (found: boolean) => {
  useStatusStore.setState({ cameraFound: found })
})
