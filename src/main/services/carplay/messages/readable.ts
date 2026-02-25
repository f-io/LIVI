import { MessageHeader, CommandMapping } from './common.js'

export enum AudioCommand {
  AudioOutputStart = 1,
  AudioOutputStop = 2,
  AudioInputConfig = 3,
  AudioPhonecallStart = 4,
  AudioPhonecallStop = 5,
  AudioNaviStart = 6,
  AudioNaviStop = 7,
  AudioSiriStart = 8,
  AudioSiriStop = 9,
  AudioMediaStart = 10,
  AudioMediaStop = 11,
  AudioAttentionStart = 12,
  AudioAttentionStop = 13,
  AudioAttentionRinging = 14,
  AudioTurnByTurnStart = 15,
  AudioTurnByTurnStop = 16
}

export abstract class Message {
  header: MessageHeader

  constructor(header: MessageHeader) {
    this.header = header
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
    this.data = data.toString('ascii')
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
      console.debug('wifi avail, phone type: ', PhoneType[this.phoneType], ' wifi: ', this.wifi)
    } else {
      this.phoneType = data.readUInt32LE(0)
      console.debug('no wifi avail, phone type: ', PhoneType[this.phoneType])
    }
  }
}

export class Unplugged extends Message {
  constructor(header: MessageHeader) {
    super(header)
  }
}

export type AudioFormat = {
  frequency: 48000 | 44100 | 24000 | 16000 | 8000
  channel: 1 | 2
  bitDepth: number
  format?: string
  mimeType?: string
}

type DecodeTypeMapping = {
  [key: number]: AudioFormat
}

export const decodeTypeMap: DecodeTypeMapping = {
  1: {
    frequency: 44100,
    channel: 2,
    bitDepth: 16,
    format: 'S16LE',
    mimeType: 'audio/L16; rate=44100; channels=2'
  },
  2: {
    frequency: 44100,
    channel: 2,
    bitDepth: 16,
    format: 'S16LE',
    mimeType: 'audio/L16; rate=44100; channels=2'
  },
  3: {
    frequency: 8000,
    channel: 1,
    bitDepth: 16,
    format: 'S16LE',
    mimeType: 'audio/L16; rate=8000; channels=1'
  },
  4: {
    frequency: 48000,
    channel: 2,
    bitDepth: 16,
    format: 'S16LE',
    mimeType: 'audio/L16; rate=48000; channels=2'
  },
  5: {
    frequency: 16000,
    channel: 1,
    bitDepth: 16,
    format: 'S16LE',
    mimeType: 'audio/L16; rate=16000; channels=1'
  },
  6: {
    frequency: 24000,
    channel: 1,
    bitDepth: 16,
    format: 'S16LE',
    mimeType: 'audio/L16; rate=24000; channels=1'
  },
  7: {
    frequency: 16000,
    channel: 2,
    bitDepth: 16,
    format: 'S16LE',
    mimeType: 'audio/L16; rate=16000; channels=2'
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
  AlbumCover = 3,
  ControlAutoplayTrigger = 100
}

export enum NavigationMetaType {
  DashboardInfo = 200
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
        }
      }
    | { type: MediaType.AlbumCover; base64Image: string }
    | { type: MediaType.ControlAutoplayTrigger }

  constructor(header: MessageHeader, mediaType: MediaType, payloadOnly: Buffer) {
    super(header)
    this.mediaType = mediaType

    if (mediaType === MediaType.AlbumCover) {
      this.payload = {
        type: mediaType,
        base64Image: payloadOnly.toString('base64')
      }
      return
    }

    if (mediaType === MediaType.Data) {
      const jsonBytes = payloadOnly.subarray(0, Math.max(0, payloadOnly.length - 1)) // drop trailing NUL-ish byte
      try {
        this.payload = {
          type: mediaType,
          media: JSON.parse(jsonBytes.toString('utf8'))
        }
      } catch {
        // keep payload undefined on parse error
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
    if (this.innerType === NavigationMetaType.DashboardInfo) {
      const msg = new NavigationData(header, NavigationMetaType.DashboardInfo, payloadOnly)
      this.inner = { kind: 'navigation', message: msg }
      return
    }

    // known media types
    if (
      this.innerType === MediaType.Data ||
      this.innerType === MediaType.AlbumCover ||
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

export class VendorCarPlaySessionBlob extends Message {
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
