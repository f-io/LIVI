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

// Carplay Store
export interface CarplayStore {
  // App-Einstellungen
  settings: ExtraConfig | null
  saveSettings: (settings: ExtraConfig) => void
  getSettings: () => void
  stream: (stream: any) => void
  resetInfo: () => void

  // Display-Resolution
  negotiatedWidth: number | null
  negotiatedHeight: number | null

  // USB Device Info
  serial: string | null
  manufacturer: string | null
  product: string | null
  fwVersion: string | null

  // Audio-Metadata
  audioCodec: string | null
  audioSampleRate: number | null
  audioChannels: number | null
  audioBitDepth: number | null

  // PCM-Data for FFT
  audioPcmData: Float32Array | null
  setPcmData: (data: Float32Array) => void

  // Audio settings with direct access
  audioVolume: number
  navVolume: number
  audioJitterMs: number

  // Audio setters
  setAudioVolume: (volume: number) => void
  setNavVolume: (volume: number) => void
  setAudioJitterMs: (jitterMs: number) => void

  // Setter
  setDeviceInfo: (info: {
    serial: string
    manufacturer: string
    product: string
    fwVersion: string
  }) => void
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

  saveSettings: (settings) => {
    set({ settings })
    // Sync audio settings from saved settings
    set({
      audioVolume: settings.audioVolume ?? 1.0,
      navVolume: settings.navVolume ?? 0.5,
      audioJitterMs: settings.audioJitterMs ?? 15
    })
    socket.emit('saveSettings', settings)
  },

  getSettings: () => {
    socket.emit('getSettings')
  },

  stream: (stream) => {
    socket.emit('stream', stream)
  },

  // Reset all stored info
  resetInfo: () =>
    set({
      negotiatedWidth: null,
      negotiatedHeight: null,
      serial: null,
      manufacturer: null,
      product: null,
      fwVersion: null,
      audioCodec: null,
      audioSampleRate: null,
      audioChannels: null,
      audioBitDepth: null,
      audioPcmData: null
    }),

  negotiatedWidth: null,
  negotiatedHeight: null,
  serial: null,
  manufacturer: null,
  product: null,
  fwVersion: null,

  audioCodec: null,
  audioSampleRate: null,
  audioChannels: null,
  audioBitDepth: null,

  audioPcmData: null,
  setPcmData: (data) => set({ audioPcmData: data }),

  // Audio settings with defaults
  audioVolume: 1.0,
  navVolume: 0.5,
  audioJitterMs: 15,

  // Audio setters
  setAudioVolume: (audioVolume) => {
    set({ audioVolume })
    const { settings, navVolume, audioJitterMs } = get()
    if (settings) {
      const updatedSettings: ExtraConfig = {
        ...settings,
        audioVolume,
        navVolume,
        audioJitterMs
      }
      get().saveSettings(updatedSettings)
    }
  },

  setNavVolume: (navVolume) => {
    set({ navVolume })
    const { settings, audioVolume, audioJitterMs } = get()
    if (settings) {
      const updatedSettings: ExtraConfig = {
        ...settings,
        audioVolume,
        navVolume,
        audioJitterMs
      }
      get().saveSettings(updatedSettings)
    }
  },

  setAudioJitterMs: (audioJitterMs) => {
    set({ audioJitterMs })
    const { settings, audioVolume, navVolume } = get()
    if (settings) {
      const updatedSettings: ExtraConfig = {
        ...settings,
        audioVolume,
        navVolume,
        audioJitterMs
      }
      get().saveSettings(updatedSettings)
    }
  },

  setDeviceInfo: ({ serial, manufacturer, product, fwVersion }) =>
    set({ serial, manufacturer, product, fwVersion }),

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
  setReverse: (reverse) => set({ reverse }),
  setLights: (lights) => set({ lights })
}))

// Socket.IO Event-Handler
socket.on('settings', (settings: ExtraConfig) => {
  useCarplayStore.setState({
    settings,
    audioVolume: settings.audioVolume ?? 1.0,
    navVolume: settings.navVolume ?? 0.5,
    audioJitterMs: settings.audioJitterMs ?? 15
  })
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
