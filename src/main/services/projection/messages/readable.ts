import { MessageHeader } from './common.js'
import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'

export abstract class Message {
  header: MessageHeader

  constructor(header: MessageHeader) {
    this.header = header
  }
}

export class DongleReady extends Message {
  constructor(header: MessageHeader) {
    super(header)
  }
}

export class Command extends Message {
  value: CommandMapping

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.value = data.readUInt32LE(0)
  }
}

export class ManufacturerInfo extends Message {
  a: number
  b: number

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.a = data.readUInt32LE(0)
    this.b = data.readUInt32LE(4)
  }
}

export class SoftwareVersion extends Message {
  version: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.version = data
      .toString('ascii')
      .replace(/\0+$/g, '')
      .trim()
      .replace(/^(\d{4}\.\d{2}\.\d{2}\.\d{4}).*$/, '$1')
  }
}

export class GnssData extends Message {
  text: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.text = data.toString('ascii').replace(/\0+$/g, '')
  }
}

export class BluetoothAddress extends Message {
  address: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.address = data.toString('ascii')
  }
}

export class BluetoothPIN extends Message {
  pin: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.pin = data.toString('ascii')
  }
}

export class BluetoothDeviceName extends Message {
  name: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.name = data.toString('ascii')
  }
}

export class WifiDeviceName extends Message {
  name: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.name = data.toString('ascii')
  }
}

export class HiCarLink extends Message {
  link: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.link = data.toString('ascii')
  }
}

export class BluetoothPairedList extends Message {
  data: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.data = data.toString('utf8').replace(/\0+$/g, '')
  }
}

export enum PhoneType {
  AndroidMirror = 1,
  CarPlay = 3,
  iPhoneMirror = 4,
  AndroidAuto = 5,
  HiCar = 6
}

export class Plugged extends Message {
  phoneType: PhoneType
  wifi?: number

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    const wifiAvail = Buffer.byteLength(data) === 8
    if (wifiAvail) {
      this.phoneType = data.readUInt32LE(0)
      this.wifi = data.readUInt32LE(4)
    } else {
      this.phoneType = data.readUInt32LE(0)
    }
  }
}

export class Unplugged extends Message {
  constructor(header: MessageHeader) {
    super(header)
  }
}

export class AudioData extends Message {
  command?: AudioCommand
  decodeType: number
  volume: number
  volumeDuration?: number
  audioType: number
  data?: Int16Array

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.decodeType = data.readUInt32LE(0)
    this.volume = data.readFloatLE(4)
    this.audioType = data.readUInt32LE(8)

    const payloadBytes = data.length - 12
    if (payloadBytes <= 0) return

    if (payloadBytes === 1) {
      this.command = data.readUInt8(12)
    } else if (payloadBytes === 4) {
      this.volumeDuration = data.readFloatLE(12)
    } else if (payloadBytes > 0) {
      const byteOffset = data.byteOffset + 12
      const sampleCount = payloadBytes / Int16Array.BYTES_PER_ELEMENT
      this.data = new Int16Array(data.buffer, byteOffset, sampleCount)
    }
  }
}

export class VideoData extends Message {
  width: number
  height: number
  flags: number
  length: number
  unknown: number
  data: Buffer

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.width = data.readUInt32LE(0)
    this.height = data.readUInt32LE(4)
    this.flags = data.readUInt32LE(8)
    this.length = data.readUInt32LE(12)
    this.unknown = data.readUInt32LE(16)
    this.data = data.subarray(20)
  }
}

export enum MediaType {
  Data = 1,
  AlbumCover = 2,
  AlbumCoverAlt = 3,
  ControlAutoplayTrigger = 100
}

export enum NavigationMetaType {
  DashboardInfo = 200,
  DashboardImage = 201
}

function isAsciiBase64(buf: Buffer): boolean {
  // allow trailing NUL
  const s = buf.toString('ascii').replace(/\0+$/g, '').trim()
  if (!s) return false

  // reject if contains non-base64 chars
  // (we accept newlines just in case)
  return /^[A-Za-z0-9+/=\r\n]+$/.test(s)
}

export class MediaData extends Message {
  mediaType: MediaType
  payload?:
    | {
        type: MediaType.Data
        media: {
          MediaSongName?: string
          MediaAlbumName?: string
          MediaArtistName?: string
          MediaAPPName?: string
          MediaSongDuration?: number
          MediaSongPlayTime?: number
          MediaPlayStatus?: number
        }
      }
    | { type: MediaType.AlbumCoverAlt; base64Image: string }
    | { type: MediaType.ControlAutoplayTrigger }

  constructor(header: MessageHeader, mediaType: MediaType, payloadOnly: Buffer) {
    super(header)
    this.mediaType = mediaType

    // innerType=2 and innerType=3 are both treated as album art.
    // If payload is ASCII base64, keep it as-is.
    // Otherwise encode raw binary as base64.
    if (mediaType === MediaType.AlbumCover || mediaType === MediaType.AlbumCoverAlt) {
      if (isAsciiBase64(payloadOnly)) {
        const b64 = payloadOnly.toString('ascii').replace(/\0+$/g, '').trim()
        this.payload = { type: MediaType.AlbumCoverAlt, base64Image: b64 }
        return
      }

      this.payload = {
        type: MediaType.AlbumCoverAlt,
        base64Image: payloadOnly.toString('base64')
      }
      return
    }

    if (mediaType === MediaType.Data) {
      const jsonBytes = payloadOnly.subarray(0, Math.max(0, payloadOnly.length - 1))
      try {
        this.payload = { type: mediaType, media: JSON.parse(jsonBytes.toString('utf8')) }
      } catch {
        // ignore
      }
      return
    }

    if (mediaType === MediaType.ControlAutoplayTrigger) {
      this.payload = { type: mediaType }
      return
    }
  }
}

export type NaviInfo = {
  NaviStatus?: number
  NaviTimeToDestination?: number
  NaviDestinationName?: string
  NaviDistanceToDestination?: number
  NaviAPPName?: string
  NaviRemainDistance?: number

  NaviRoadName?: string
  NaviOrderType?: number
  NaviManeuverType?: number
  NaviTurnAngle?: number
  NaviTurnSide?: number
  NaviImageBase64?: string
} & Record<string, unknown>

export function parseNaviInfoFromBuffer(buf: Buffer): NaviInfo | null {
  let s = buf.toString('utf8')
  const nul = s.indexOf('\u0000')
  if (nul !== -1) s = s.slice(0, nul)
  s = s.trim()
  if (!s) return null

  try {
    const parsed = JSON.parse(s)
    if (!parsed || typeof parsed !== 'object') return null
    return parsed as NaviInfo
  } catch {
    return null
  }
}

export class NavigationData extends Message {
  metaType: NavigationMetaType
  navi: NaviInfo | null
  rawUtf8: string

  constructor(header: MessageHeader, metaType: NavigationMetaType, payloadOnly: Buffer) {
    super(header)
    this.metaType = metaType

    if (metaType === NavigationMetaType.DashboardImage) {
      this.rawUtf8 = ''
      this.navi = {
        NaviImageBase64: payloadOnly.toString('base64')
      }
      return
    }

    let s = payloadOnly.toString('utf8')
    const nul = s.indexOf('\u0000')
    if (nul !== -1) s = s.slice(0, nul)
    this.rawUtf8 = s

    this.navi = parseNaviInfoFromBuffer(payloadOnly)
  }
}

export type MetaInner =
  | { kind: 'media'; message: MediaData }
  | { kind: 'navigation'; message: NavigationData }
  | { kind: 'unknown'; metaType: number; raw: Buffer }

export class MetaData extends Message {
  innerType: number
  inner: MetaInner

  constructor(header: MessageHeader, data: Buffer) {
    super(header)

    this.innerType = data.readUInt32LE(0)
    const payloadOnly = data.subarray(4)

    // Navigation
    if (
      this.innerType === NavigationMetaType.DashboardInfo ||
      this.innerType === NavigationMetaType.DashboardImage
    ) {
      const msg = new NavigationData(header, this.innerType as NavigationMetaType, payloadOnly)
      this.inner = { kind: 'navigation', message: msg }
      return
    }

    // known media types
    if (
      this.innerType === MediaType.Data ||
      this.innerType === MediaType.AlbumCover ||
      this.innerType === MediaType.AlbumCoverAlt ||
      this.innerType === MediaType.ControlAutoplayTrigger
    ) {
      const msg = new MediaData(header, this.innerType as MediaType, payloadOnly)
      this.inner = { kind: 'media', message: msg }
      return
    }

    // Unknown
    this.inner = { kind: 'unknown', metaType: this.innerType, raw: payloadOnly }

    const head = data.subarray(0, Math.min(64, data.length))
    console.info(
      `Unexpected meta innerType: ${this.innerType}, bytes=${data.length}, head=${head.toString('hex')}`
    )
    const text = payloadOnly.toString('utf8')
    const trimmed = text.replace(/\0+$/g, '').trim()
    if (trimmed.length > 0) {
      console.info(
        `Unexpected meta innerType: ${this.innerType}, utf8=${JSON.stringify(trimmed.slice(0, 200))}`
      )
    }
  }
}

export class BluetoothPeerConnecting extends Message {
  address: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.address = data.toString('ascii')
  }
}

export class BluetoothPeerConnected extends Message {
  address: string

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.address = data.toString('ascii')
  }
}

export class Opened extends Message {
  width: number
  height: number
  fps: number
  format: number
  packetMax: number
  iBox: number
  phoneMode: number

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.width = data.readUInt32LE(0)
    this.height = data.readUInt32LE(4)
    this.fps = data.readUInt32LE(8)
    this.format = data.readUInt32LE(12)
    this.packetMax = data.readUInt32LE(16)
    this.iBox = data.readUInt32LE(20)
    this.phoneMode = data.readUInt32LE(24)
  }
}

export type BoxDeviceEntry = {
  id?: string
  type?: string
  name?: string
  index?: string | number
  time?: string
  rfcomm?: string | number
} & Record<string, unknown>

export type BoxInfoSettings = {
  uuid?: string
  MFD?: string
  boxType?: string
  OemName?: string
  productType?: string
  HiCar?: number
  supportLinkType?: string
  supportFeatures?: string
  hwVersion?: string
  wifiChannel?: number
  CusCode?: string
  DevList?: BoxDeviceEntry[]
} & Record<string, unknown>

export class BoxInfo extends Message {
  settings: BoxInfoSettings

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.settings = JSON.parse(data.toString('utf8')) as BoxInfoSettings
  }
}

export class VendorSessionInfo extends Message {
  public readonly raw: Buffer

  public constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.raw = data
  }
}

export class Phase extends Message {
  value: number

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.value = data.readUInt32LE(0)
  }
}

export enum BoxPhase {
  EVT_ANDROID_PLUG_OUT = 0,
  EVT_ANDROID_PLUG_IN = 1,
  EVT_IPHONE_PLUG_OUT = 2,
  EVT_IPHONE_PLUG_IN = 3,

  EVT_PHONE_PLUG_IN = 4,
  EVT_WAIT_HOTPOT = 5,
  EVT_WAIT_AIRPLAY = 6,
  EVT_PERMMISION_ASKING = 7,
  EVT_NOT_REGIST = 8,
  EVT_REG = 9,
  EVT_SCREEN_ON = 10,
  EVT_SCREEN_OFF = 11,

  EVT_OTG_PLUG_OUT = 12,
  EVT_OTG_PLUG_IN = 13,

  EVT_ANDROID_WORKING = 14,
  EVT_IPHONE_WORKING = 15,
  EVT_CARLIFE_DOWNLOAD = 16,
  EVT_SET_PERMISSION = 17,

  EVT_DECODE_CONFIGURE_ERR = 18,
  EVT_DECODE_OUTPUT_ERR = 19,

  EVT_SETTINGS_PAGE_ENTER = 20,
  EVT_SETTINGS_PAGE_BACK = 21,

  EVT_FAKE_OTG_PLUG_IN = 22,
  EVT_FAKE_OTG_PLUG_OUT = 23,

  EVT_BOX_ENTER_U_MODE = 24,
  EVT_MANUAL_DISCONNECT_PHONE = 25,

  EVT_BOX_READY = 116,

  EVT_BOXMIC_DETECTED = 117,
  EVT_BOXMIC_CONNECTED = 118,
  EVT_BOXMIC_DISCONNECTED = 119,

  EVT_BOX_UPDATE = 120,
  EVT_BOX_UPDATE_SUCCESS = 121,
  EVT_BOX_UPDATE_FAILED = 122,
  EVT_BOX_VERSION_ERROR = 123,
  EVT_BOX_VERSION_SHOW = 124,

  EVT_BOX_OTA_UPDATE = 125,
  EVT_BOX_OTA_UPDATE_SUCCESS = 126,
  EVT_BOX_OTA_UPDATE_FAILED = 127,

  EVT_BOX_SUPPORT_AUTO_CONNECT = 200,
  EVT_BOX_SCANING_DEVICES = 201,
  EVT_BOX_DEVICE_FOUND = 202,
  EVT_BOX_DEVICE_NOT_FOUND = 203,
  EVT_BOX_CONNECT_DEVICE_FAILED = 204,

  EVT_BOX_BLUETOOTH_CONNECTED = 205,
  EVT_BOX_BLUETOOTH_DISCONNECTED = 206,

  EVT_BOX_WIFI_CONNECTED = 207,
  EVT_BOX_WIFI_DISCONNECTED = 208,

  EVT_BOX_BLUETOOTH_PAIR_START = 209,
  EVT_UPDATE_BLUETOOTH_PAIRED_LIST = 210,
  EVT_UPDATE_BLUETOOTH_ONLINE_LIST = 211,

  EVT_BOX_REQUEST_VIDEO_FOCUS = 212,
  EVT_BOX_RELEASE_VIDEO_FOCUS = 213,

  EVT_UPDATE_CONNECTION_URL = 214
}

export function boxPhaseToString(v: number): string {
  const byValue = BoxPhase as unknown as Record<number, string>
  return byValue[v] ?? `UNKNOWN_PHASE_${v}`
}

export enum BoxUpdateStatus {
  BoxUpdateStart = 1,
  BoxUpdateSuccess = 2,
  BoxUpdateFailed = 3,

  BoxOtaUpdateStart = 5,
  BoxOtaUpdateSuccess = 6,
  BoxOtaUpdateFailed = 7
}

export function boxUpdateStatusToString(status: number): string {
  switch (status) {
    case BoxUpdateStatus.BoxUpdateStart:
      return 'EVT_BOX_UPDATE'
    case BoxUpdateStatus.BoxUpdateSuccess:
      return 'EVT_BOX_UPDATE_SUCCESS'
    case BoxUpdateStatus.BoxUpdateFailed:
      return 'EVT_BOX_UPDATE_FAILED'
    case BoxUpdateStatus.BoxOtaUpdateStart:
      return 'EVT_BOX_OTA_UPDATE'
    case BoxUpdateStatus.BoxOtaUpdateSuccess:
      return 'EVT_BOX_OTA_UPDATE_SUCCESS'
    case BoxUpdateStatus.BoxOtaUpdateFailed:
      return 'EVT_BOX_OTA_UPDATE_FAILED'
    default:
      return `EVT_BOX_UPDATE_UNKNOWN(${status})`
  }
}

// CMD_UPDATE_PROGRESS (177), payload: int32 progress
export class BoxUpdateProgress extends Message {
  progress: number

  constructor(header: MessageHeader, data: Buffer) {
    super(header)
    this.progress = data.readInt32LE(0)
  }
}

// CMD_UPDATE (187), payload: int32 status
export class BoxUpdateState extends Message {
  status: BoxUpdateStatus | number
  statusText: string
  isOta: boolean
  isTerminal: boolean
  ok?: boolean

  constructor(header: MessageHeader, data: Buffer) {
    super(header)

    const raw = data.readInt32LE(0)
    this.status = raw
    this.statusText = boxUpdateStatusToString(raw)
    this.isOta = raw === 5 || raw === 6 || raw === 7
    this.isTerminal = raw === 2 || raw === 3 || raw === 6 || raw === 7

    if (raw === 2 || raw === 6) this.ok = true
    if (raw === 3 || raw === 7) this.ok = false
  }
}
