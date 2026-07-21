/**
 * getInfo — builds the CarPlay GetInfo (/info) response plist.
 *
 * /info is where the accessory declares its full capability set. The phone reads
 * it after session SETUP and only proceeds (requesting the screen stream) if the
 * declaration is complete: it needs the display, the audio formats/latencies, the
 * CarPlay resource `modes`, and HID input devices. A partial /info makes the
 * phone abort. Structure and the protocol constants (stream/resource/audio
 * enums, feature bitfield).
 */

import type { PlistValue } from './bplist'
import { knobHidDevice, mediaHidDevice, telephonyHidDevice, touchHidDevice } from './hid'
import type { CpDisplayConfig, CpStackConfig } from './types'

const STREAM_TYPE_MAIN_SCREEN = 110
const STREAM_TYPE_ALT_SCREEN = 111
const DISPLAY_FEATURE_KNOBS = 0x02
const DISPLAY_FEATURE_HIGH_FIDELITY_TOUCH = 0x08
const PRIMARY_INPUT_KNOBS = 3

// CarPlay accessory feature bitfield (screen + audio + core capabilities).
const CARPLAY_FEATURES = 0x615653aee2
// Audio capability bits, cleared to advertise no audio source or sink.
const CARPLAY_AUDIO_FEATURES = 0x10004540a00
const CARPLAY_FEATURES_NO_AUDIO = Number(BigInt(CARPLAY_FEATURES) & ~BigInt(CARPLAY_AUDIO_FEATURES))

export const MAIN_UUID = 'b7e6c5a0-1111-4000-8000-000000000001'
export const ALT_UUID = 'b7e6c5a0-2222-4000-8000-000000000002'

// Resource arbitration constants (CarPlay "modes").
const RESOURCE_SCREEN = 1
const RESOURCE_AUDIO = 2
const TRANSFER_TAKE = 1
const PRIORITY_NICE_TO_HAVE = 100
const CONSTRAINT_ANYTIME = 100

function resource(resourceID: number): PlistValue {
  return {
    resourceID,
    transferType: TRANSFER_TAKE,
    transferPriority: PRIORITY_NICE_TO_HAVE,
    takeConstraint: CONSTRAINT_ANYTIME,
    borrowConstraint: CONSTRAINT_ANYTIME,
    unborrowConstraint: CONSTRAINT_ANYTIME
  }
}

// Reference-standard initial modes: resources + appStates only.
function modes(): PlistValue {
  return {
    resources: [resource(RESOURCE_SCREEN), resource(RESOURCE_AUDIO)],
    appStates: [
      { appStateID: 2, state: false }, // call
      { appStateID: 1, speechMode: -1 }, // speech
      { appStateID: 3, state: false } // turn-by-turn
    ]
  }
}

function audioLatencies(): PlistValue[] {
  // Informational A/V-sync latency per stream. The phone ignores this for buffered-audio
  // pacing (empirically confirmed: advertising mediaDelay here did not move its requested
  // audioLatencyMs off 1000)
  const base = (type: number, audioType?: string): PlistValue => {
    const d: { [k: string]: PlistValue } = { type, inputLatencyMicros: 0, outputLatencyMicros: 0 }
    if (audioType) d.audioType = audioType
    return d
  }
  return [
    base(100),
    base(100, 'default'),
    base(100, 'media'),
    base(100, 'telephony'),
    base(100, 'speechRecognition'),
    base(100, 'alert'),
    base(101),
    base(101, 'default'),
    base(102, 'default')
  ]
}

// AudioFormat bits
// AAC-LC / AAC-ELD, the codecs wireless CarPlay uses. AAC-LC media is decoded to PCM.
function audioFormats(entertainmentRate: 44100 | 48000): PlistValue[] {
  const f = (
    type: number,
    audioType: string,
    outputFormats: number,
    inputFormats?: number
  ): PlistValue => {
    const d: { [k: string]: PlistValue } = { type, audioType, audioOutputFormats: outputFormats }
    if (inputFormats !== undefined) d.audioInputFormats = inputFormats
    return d
  }
  // PCM, OPUS and AAC-LC. PCM is USB-only, so
  // over wireless the phone picks OPUS for the low-latency streams (100/101) and
  // AAC-LC for the entertainment stream (102).
  // The samplingFrequency setting picks the media rate (44.1k vs 48k).
  const is48 = entertainmentRate === 48000
  const PCM_VOICE = 0x3fc // PCM 8/16/24/32k, mono + stereo
  const PCM = PCM_VOICE | (is48 ? 0xc000 : 0xc00) // + media 48k (or 44.1k) mono + stereo
  const PCM_MONO = 0x154 | (is48 ? 0x4000 : 0x400) // voice mono + media mono
  const OPUS = 0x70000000 // OPUS 16k/24k/48k mono
  const AAC_LC = is48 ? 0x800000 : 0x400000 // AAC-LC at the configured media rate
  return [
    f(100, 'compatibility', PCM, PCM_MONO),
    f(101, 'compatibility', PCM),
    f(100, 'default', PCM | OPUS, PCM_MONO | OPUS),
    f(100, 'alert', PCM | OPUS),
    f(100, 'media', PCM),
    f(100, 'telephony', PCM_MONO | OPUS, PCM_MONO | OPUS),
    f(100, 'speechRecognition', PCM_MONO | OPUS, PCM_MONO | OPUS),
    f(101, 'default', PCM | OPUS),
    f(102, 'media', AAC_LC)
  ]
}

function areaDict(d: CpDisplayConfig): PlistValue | null {
  const va = d.viewArea
  if (!va) return null
  const W = d.widthPixels
  const H = d.heightPixels
  const view: Record<string, PlistValue> = {
    widthPixels: W - va.left - va.right,
    heightPixels: H - va.top - va.bottom,
    originXPixels: va.left,
    originYPixels: va.top
  }
  const sa = d.safeArea
  if (sa) {
    const safe: Record<string, PlistValue> = {
      widthPixels: W - sa.left - sa.right,
      heightPixels: H - sa.top - sa.bottom,
      originXPixels: sa.left,
      originYPixels: sa.top
    }
    if (d.safeAreaDrawOutside !== undefined) safe.drawUIOutsideSafeArea = d.safeAreaDrawOutside
    view.safeArea = safe
  }
  return view
}

function displayEntry(d: CpDisplayConfig, type: number, uuid: string): PlistValue {
  const widthPhysical = d.widthPhysicalMm ?? 200
  const heightPhysical =
    d.heightPhysicalMm ??
    Math.max(1, Math.round((widthPhysical * d.heightPixels) / Math.max(1, d.widthPixels)))
  const entry: Record<string, PlistValue> = {
    uuid,
    type,
    maxFPS: d.fps ?? 60,
    widthPixels: d.widthPixels,
    heightPixels: d.heightPixels,
    widthPhysical,
    heightPhysical,
    features: DISPLAY_FEATURE_HIGH_FIDELITY_TOUCH | DISPLAY_FEATURE_KNOBS,
    primaryInputDevice: d.primaryInputDevice ?? PRIMARY_INPUT_KNOBS
  }
  const view = areaDict(d)
  if (view) {
    entry.viewAreas = [view]
    entry.initialViewArea = 0
  }
  if (d.initialUrl) entry.initialURL = d.initialUrl
  return entry
}

export function buildInfoPlist(cfg: CpStackConfig): PlistValue {
  const displays: PlistValue[] = [displayEntry(cfg.main, STREAM_TYPE_MAIN_SCREEN, MAIN_UUID)]
  if (cfg.cluster) displays.push(displayEntry(cfg.cluster, STREAM_TYPE_ALT_SCREEN, ALT_UUID))

  const info: { [k: string]: PlistValue } = {
    sourceVersion: cfg.sourceVersion,
    features: cfg.disableAudioOutput ? CARPLAY_FEATURES_NO_AUDIO : CARPLAY_FEATURES,
    statusFlags: 4,
    model: 'LIVI',
    manufacturer: 'LIVI',
    deviceID: cfg.deviceId,
    bluetoothIDs: [cfg.deviceId],
    name: cfg.deviceName,
    rightHandDrive: false,
    keepAliveLowPower: true,
    keepAliveSendStatsAsBody: false,
    modes: modes(),
    ...(cfg.disableAudioOutput
      ? {}
      : {
          audioLatencies: audioLatencies(),
          audioFormats: audioFormats(cfg.entertainmentSampleRate)
        }),
    extendedFeatures: ['vocoderInfo', 'enhancedRequestCarUI'],
    displays,
    hidDevices: [
      touchHidDevice(cfg.main.widthPixels, cfg.main.heightPixels, MAIN_UUID),
      knobHidDevice(MAIN_UUID),
      mediaHidDevice(MAIN_UUID),
      telephonyHidDevice(MAIN_UUID)
    ]
  }

  // Head-unit icon on the CarPlay homescreen.
  if (cfg.icons.length > 0) {
    info.oemIconVisible = true
    info.oemIconLabel = cfg.oemLabel
    info.oemIcons = cfg.icons.map((icon) => ({
      imageData: icon.data,
      widthPixels: icon.widthPixels,
      heightPixels: icon.heightPixels,
      prerendered: true
    }))
  }
  if (cfg.hevc) info.hevcInfo = {}
  return info
}
