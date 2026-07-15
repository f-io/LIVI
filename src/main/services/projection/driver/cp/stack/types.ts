import type { MfiSigner } from './mfiSigner'

export interface CpDisplayInsets {
  top: number
  bottom: number
  left: number
  right: number
}

export interface CpDisplayConfig {
  /** Encoded/streamed pixel dimensions. */
  widthPixels: number
  heightPixels: number
  /** Physical size in millimetres, if known (drives DPI on the phone). */
  widthPhysicalMm?: number
  heightPhysicalMm?: number
  fps?: number
  /** Primary input device kind (e.g. touchscreen). */
  primaryInputDevice?: number
  /** Drawable region inset from the display edges (CarPlay viewArea). */
  viewArea?: CpDisplayInsets
  /** Region safe from occlusion inset from the display edges (CarPlay safeArea). */
  safeArea?: CpDisplayInsets
  /** Main display only: allow the phone to draw UI outside the safe area. */
  safeAreaDrawOutside?: boolean
  /** Initial CarPlay URL rendered on this display (alt/cluster screen). */
  initialUrl?: string
}

/** How one CarPlay audioType category maps onto LIVI's lifecycle + mic uplink. */
export interface CpAudioProfile {
  /** Mic uplink decodeType carried on the start/stop command (5 = 16k mono). */
  decodeType: number
  /** LIVI audioType: 3 = media, 2 = phone, 1 = speech. */
  audioType: number
  /** AudioCommand sent when the stream goes active / idle. */
  startCmd: number
  stopCmd: number
  label: string
}

/** A profile bound to one SETUP stream, carrying its negotiated playback format. */
export interface CpStreamProfile extends CpAudioProfile {
  /** Playback sample rate the decoded stream delivers. */
  sampleRate: number
  /** Playback channel count (1 = mono, 2 = stereo). */
  channels: number
}

export interface CpStackConfig {
  /** Name shown as the car on the phone. */
  deviceName: string
  /** Stable identifier (derived from the BT adapter MAC). */
  deviceId: string
  sourceVersion: string
  /** HW-decodable codecs advertised to the phone (Pi5: hevc only, Pi4: h264 only). */
  hevc: boolean
  h264: boolean
  /** Main (centre) screen. */
  main: CpDisplayConfig
  /** Instrument-cluster screen (classic CarPlay second stream), if enabled. */
  cluster?: CpDisplayConfig
  /** TCP control port (Bonjour-advertised). */
  port: number
  /** Entertainment (type 102) AAC-LC sample rate, from the samplingFrequency setting. */
  entertainmentSampleRate: 44100 | 48000
  /** When true the head unit does not advertise media audio sinks, so the phone plays media itself. */
  disableAudioOutput?: boolean
  /** MFi coprocessor access (certificate + sign), via the helper control socket. */
  mfi: MfiSigner
  /** Label shown under the head-unit icon in the CarPlay dock. */
  oemLabel: string
  /** OEM app icons (PNG) the phone shows on the CarPlay homescreen. */
  icons: CpIcon[]
}

/** One CarPlay homescreen icon variant (PNG bytes at a given pixel size). */
export interface CpIcon {
  widthPixels: number
  heightPixels: number
  data: Buffer
}
