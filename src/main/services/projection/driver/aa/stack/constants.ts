/**
 * Android Auto wire-protocol constants.
 */

// ── Frame flags (byte 1 of the 4-byte frame header) ─────────────────────────
// Observed in captures from open-android-auto/captures/:
//   0x03 – plaintext, first+last (version exchange + SSL_HANDSHAKE frames)
//   0x0b – encrypted, first+last, signaling (post-TLS control messages)
//   0x0f – encrypted, first+last, lifecycle (channel-open/close)
//   0x08 – encrypted, first fragment of multi-frame (large payloads)
//   0x0a – encrypted, continuation/last fragment
export const FRAME_FLAGS = {
  PLAINTEXT: 0x03, // version negotiation + SSL handshake
  ENC_SIGNAL: 0x0b, // encrypted single-frame signaling
  ENC_CONTROL: 0x0f, // encrypted channel-lifecycle messages
  ENC_FIRST_FRAG: 0x08, // encrypted first fragment
  ENC_CONT_FRAG: 0x0a // encrypted continuation/last fragment
} as const

// ── Control-channel message IDs (channel 0) ──────────────────────────────────
// Source: oaa/control/ControlMessageIdsEnum.proto
export const CTRL_MSG = {
  VERSION_REQUEST: 0x0001,
  VERSION_RESPONSE: 0x0002,
  SSL_HANDSHAKE: 0x0003,
  AUTH_COMPLETE: 0x0004,
  SERVICE_DISCOVERY_REQUEST: 0x0005, // phone → HU (despite the name)
  SERVICE_DISCOVERY_RESPONSE: 0x0006, // HU → phone, sent in reply to REQUEST
  CHANNEL_OPEN_REQUEST: 0x0007, // phone → HU (phone initiates each channel)
  CHANNEL_OPEN_RESPONSE: 0x0008, // HU → phone
  CHANNEL_CLOSE_NOTIFICATION: 0x0009,
  PING_REQUEST: 0x000b,
  PING_RESPONSE: 0x000c,
  NAVIGATION_FOCUS_REQUEST: 0x000d,
  NAVIGATION_FOCUS_RESPONSE: 0x000e,
  SHUTDOWN_REQUEST: 0x000f,
  SHUTDOWN_RESPONSE: 0x0010,
  AUDIO_FOCUS_REQUEST: 0x0012,
  AUDIO_FOCUS_RESPONSE: 0x0013,
  BINDING_REQUEST: 0x0019, // phone → HU (scan codes)
  BINDING_RESPONSE: 0x001a // HU → phone
} as const

// ── AV-channel message IDs (channels 2–9) ────────────────────────────────────
// Source: oaa/av/AVChannelMessageIdsEnum.proto
export const AV_MSG = {
  // Raw media data (low IDs, no high bit)
  AV_MEDIA_WITH_TIMESTAMP: 0x0000,
  AV_MEDIA_INDICATION: 0x0001, // H.264 / PCM frames
  // Signaling (high bit set = Specific message type)
  SETUP_REQUEST: 0x8000, // phone → HU
  START_INDICATION: 0x8001, // phone → HU (audio) / HU → phone (video)
  STOP_INDICATION: 0x8002,
  SETUP_RESPONSE: 0x8003, // HU → phone (audio) / phone → HU (video)
  AV_MEDIA_ACK: 0x8004, // HU → phone (flow control)
  AV_INPUT_OPEN_REQUEST: 0x8005,
  AV_INPUT_OPEN_RESPONSE: 0x8006,
  VIDEO_FOCUS_REQUEST: 0x8007,
  VIDEO_FOCUS_INDICATION: 0x8008
} as const

// ── Channel IDs (GAL service types) ──────────────────────────────────────────
// Source: aasdk (opencardev / f1x.studio) messenger::ChannelId enum.
export const CH = {
  CONTROL: 0,
  SENSOR: 1, // driving status, GPS, night mode
  // (2 = MEDIA_SINK group header, not used as a wire channel by aasdk)
  VIDEO: 3, // main display H.264/H.265 (MEDIA_SINK_VIDEO)
  MEDIA_AUDIO: 4, // music / podcast PCM       (MEDIA_SINK_MEDIA_AUDIO)
  SPEECH_AUDIO: 5, // navigation prompts        (MEDIA_SINK_GUIDANCE_AUDIO)
  SYSTEM_AUDIO: 6, // system sounds             (MEDIA_SINK_SYSTEM_AUDIO)
  // (7 = MEDIA_SINK_TELEPHONY_AUDIO, not advertised)
  INPUT: 8, // touch + keycodes (INPUT_SOURCE)
  MIC_INPUT: 9, // microphone from phone    (MEDIA_SOURCE_MICROPHONE)
  BLUETOOTH: 10,
  // (11 = RADIO, not advertised)
  NAVIGATION: 12, // NAVIGATION_STATUS
  MEDIA_INFO: 13, // MEDIA_PLAYBACK_STATUS
  PHONE_STATUS: 14,
  // (15 MEDIA_BROWSER, 16 VENDOR_EXTENSION, 17 GENERIC_NOTIFICATION — not advertised)
  WIFI: 18 // WIFI_PROJECTION (kept here for existing RFCOMM WiFi handover)
} as const

// ── Version negotiation ───────────────────────────────────────────────────────
export const VERSION = {
  MAJOR: 1,
  MINOR: 1, // we advertise 1.1; phone typically negotiates to 1.7
  STATUS_MATCH: 0x0000,
  STATUS_MISMATCH: 0xffff
} as const

// ── TCP ───────────────────────────────────────────────────────────────────────
export const TCP_PORT = 5277

// ── Proto enum values (inline to avoid loading protos for simple checks) ─────
export const STATUS_OK = 0

export const VIDEO_RESOLUTION = {
  _800x480: 1,
  _1280x720: 2,
  _1920x1080: 3
} as const

export const VIDEO_FPS = {
  _60: 1,
  _30: 2
} as const

export const MEDIA_CODEC = {
  AUDIO_PCM: 1,
  AUDIO_AAC_LC: 2,
  VIDEO_H264_BP: 3,
  VIDEO_VP9: 5,
  VIDEO_AV1: 6,
  VIDEO_H265: 7
} as const

export const AV_STREAM_TYPE = {
  AUDIO: 1,
  VIDEO: 3
} as const

export const DISPLAY_TYPE = {
  MAIN: 1,
  CLUSTER: 2,
  AUXILIARY: 3
} as const

export const SENSOR_TYPE = {
  DRIVING_STATUS: 13,
  NIGHT_DATA: 10,
  PARKING_BRAKE: 7,
  GPS_LOCATION: 1,
  CAR_SPEED: 3,
  RPM: 4
} as const

export const AUDIO_TYPE = {
  SPEECH: 1,
  SYSTEM: 2,
  MEDIA: 3
} as const

export const BT_PAIRING_METHOD = {
  NUMERIC_COMPARISON: 2,
  PIN: 4
} as const

export const COLOR_SCHEME = {
  BASIC: 0,
  MATERIAL_YOU_V2: 2,
  MATERIAL_YOU_V3: 3
} as const

export const AV_SETUP_STATUS = {
  NONE: 0, // not a valid wire value — treated as FAIL by phone
  FAIL: 1,
  OK: 2 // correct OK value per AVChannelSetupStatus proto
} as const
