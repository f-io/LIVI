import { DEBUG } from '@main/constants'
import type { DongleConfig } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import {
  CommandMapping,
  type CommandValue,
  MultiTouchAction,
  TouchAction
} from '@shared/types/ProjectionEnums'
import type { MultiTouchPoint } from '@shared/types/TouchTypes'
import {
  clamp,
  computeAndroidAutoDpi,
  getCurrentTimeInMs,
  matchFittingAAResolution
} from '@shared/utils'
import { buildServerCgiScript } from '../assets/LIVI_cgi.js'
import { buildLiviWeb } from '../assets/LIVI_web.js'
import { MessageHeader, MessageType } from './common.js'

export type OpenConfig = Pick<DongleConfig, 'width' | 'height' | 'fps'>

export abstract class SendableMessage {
  abstract type: MessageType

  serialise() {
    return MessageHeader.asBuffer(this.type, 0)
  }
}

export abstract class SendableMessageWithPayload extends SendableMessage {
  abstract type: MessageType

  abstract getPayload(): Buffer

  override serialise() {
    const data = this.getPayload()
    const byteLength = Buffer.byteLength(data)
    const header = MessageHeader.asBuffer(this.type, byteLength)
    return Buffer.concat([header, data])
  }
}

export class SendRawMessage extends SendableMessageWithPayload {
  type: MessageType
  private payload: Buffer

  constructor(type: number, payload: Uint8Array) {
    super()
    this.type = type as MessageType
    this.payload = Buffer.from(payload)
  }

  getPayload(): Buffer {
    return this.payload
  }
}

export class SendCommand extends SendableMessageWithPayload {
  type = MessageType.Command
  value: CommandMapping

  getPayload(): Buffer {
    const data = Buffer.alloc(4)
    data.writeUInt32LE(this.value)
    return data
  }

  constructor(value: CommandValue) {
    super()
    this.value = CommandMapping[value]
  }
}

export class SendBluetoothPairedList extends SendableMessageWithPayload {
  type = MessageType.BluetoothPairedList
  private readonly payload: Buffer

  constructor(listText: string) {
    super()
    const withNul = listText.endsWith('\0') ? listText : listText + '\0'
    this.payload = Buffer.from(withNul, 'utf8')
  }

  getPayload(): Buffer {
    return this.payload
  }
}

export class SendGnssData extends SendableMessageWithPayload {
  type = MessageType.GnssData
  private readonly payload: Buffer

  constructor(nmeaText: string) {
    super()

    const normalized = String(nmeaText ?? '')
      .replace(/\r?\n/g, '\r\n')
      .trim()

    const withLineEnd = normalized.length > 0 ? normalized + '\r\n' : ''
    this.payload = Buffer.from(withLineEnd, 'ascii')
  }

  getPayload(): Buffer {
    return this.payload
  }
}

export class SendTouch extends SendableMessageWithPayload {
  type = MessageType.Touch
  x: number
  y: number
  action: TouchAction

  getPayload(): Buffer {
    const actionB = Buffer.alloc(4)
    const xB = Buffer.alloc(4)
    const yB = Buffer.alloc(4)
    const flags = Buffer.alloc(4)

    actionB.writeUInt32LE(this.action)

    const finalX = clamp(10000 * this.x, 0, 10000)
    const finalY = clamp(10000 * this.y, 0, 10000)

    xB.writeUInt32LE(finalX)
    yB.writeUInt32LE(finalY)

    return Buffer.concat([actionB, xB, yB, flags])
  }

  constructor(x: number, y: number, action: TouchAction) {
    super()
    this.x = x
    this.y = y
    this.action = action
  }
}

class TouchItem {
  x: number
  y: number
  action: MultiTouchAction
  id: number

  constructor(x: number, y: number, action: MultiTouchAction, id: number) {
    this.x = x
    this.y = y
    this.action = action
    this.id = id
  }

  getPayload(): Buffer {
    const xB = Buffer.alloc(4)
    const yB = Buffer.alloc(4)
    const actionB = Buffer.alloc(4)
    const idB = Buffer.alloc(4)

    xB.writeFloatLE(this.x)
    yB.writeFloatLE(this.y)
    actionB.writeUInt32LE(this.action)
    idB.writeUInt32LE(this.id)

    return Buffer.concat([xB, yB, actionB, idB])
  }
}

export class SendMultiTouch extends SendableMessageWithPayload {
  type = MessageType.MultiTouch
  touches: TouchItem[]

  constructor(points: MultiTouchPoint[]) {
    super()
    this.touches = points.map((p) => new TouchItem(p.x, p.y, p.action, p.id))
  }

  getPayload(): Buffer {
    return Buffer.concat(this.touches.map((i) => i.getPayload()))
  }
}

export class SendAudio extends SendableMessageWithPayload {
  type = MessageType.AudioData
  data: Int16Array
  decodeType: number

  getPayload(): Buffer {
    const audioData = Buffer.alloc(12)
    audioData.writeUInt32LE(this.decodeType, 0)
    audioData.writeFloatLE(0.0, 4)
    audioData.writeUInt32LE(3, 8)
    return Buffer.concat([audioData, Buffer.from(this.data.buffer)])
  }

  constructor(data: Int16Array, decodeType: number) {
    super()
    this.data = data
    this.decodeType = decodeType
  }
}

export class SendFile extends SendableMessageWithPayload {
  type = MessageType.SendFile
  content: Buffer
  fileName: string

  private getFileName = (name: string) => Buffer.from(name + '\0', 'ascii')

  private getLength = (data: Buffer) => {
    const buffer = Buffer.alloc(4)
    buffer.writeUInt32LE(Buffer.byteLength(data))
    return buffer
  }

  getPayload(): Buffer {
    const newFileName = this.getFileName(this.fileName)
    const nameLength = this.getLength(newFileName)
    const contentLength = this.getLength(this.content)
    return Buffer.concat([nameLength, newFileName, contentLength, this.content])
  }

  constructor(content: Buffer, fileName: string) {
    super()
    this.content = content
    this.fileName = fileName
  }
}

export enum FileAddress {
  DPI = '/tmp/screen_dpi',
  NIGHT_MODE = '/tmp/night_mode',
  HAND_DRIVE_MODE = '/tmp/hand_drive_mode',
  CHARGE_MODE = '/tmp/charge_mode',
  OEM_ICON = '/etc/oem_icon.png',
  AIRPLAY_CONFIG = '/etc/airplay.conf',
  BOX_NAME = '/etc/box_name',
  AIRPLAY_CAR_CONFIG = '/etc/airplay_car.conf',
  CARPLAY_LOGO_TYPE = '/etc/carplay_logo_type',
  ICON_120 = '/etc/icon_120x120.png',
  ICON_180 = '/etc/icon_180x180.png',
  ICON_256 = '/etc/icon_256x256.png',
  ANDROID_WORK_MODE = '/etc/android_work_mode',
  LIVI_CGI = '/tmp/boa/cgi-bin/server.cgi',
  LIVI_WEB = '/tmp/boa/www/index.html',
  HU_VIEWAREA_INFO = '/etc/RiddleBoxData/HU_VIEWAREA_INFO',
  HU_SAFEAREA_INFO = '/etc/RiddleBoxData/HU_SAFEAREA_INFO',
  TMP = '/tmp'
}

export type ViewAreaOptions = Record<string, never>

export type SafeAreaOptions = {
  insets?: Partial<ScreenInsets>
  drawOutside?: boolean
}

export type ScreenInsets = {
  top: number
  bottom: number
  left: number
  right: number
}

/** 24 bytes LE: [screenW, screenH, viewW, viewH, originX, originY] */
export class SendViewArea extends SendFile {
  constructor(screenW: number, screenH: number, _options: ViewAreaOptions = {}) {
    const b = Buffer.alloc(24)
    b.writeUInt32LE(screenW, 0)
    b.writeUInt32LE(screenH, 4)
    b.writeUInt32LE(screenW, 8)
    b.writeUInt32LE(screenH, 12)
    b.writeUInt32LE(0, 16)
    b.writeUInt32LE(0, 20)

    super(b, FileAddress.HU_VIEWAREA_INFO)
  }
}

/** 20 bytes LE: [safeW, safeH, originX, originY, drawOutside] */
export class SendSafeArea extends SendFile {
  constructor(videoW: number, videoH: number, options: SafeAreaOptions = {}) {
    const insets: ScreenInsets = {
      top: options.insets?.top ?? 0,
      bottom: options.insets?.bottom ?? 0,
      left: options.insets?.left ?? 0,
      right: options.insets?.right ?? 0
    }

    const safeW = Math.max(0, videoW - insets.left - insets.right)
    const safeH = Math.max(0, videoH - insets.top - insets.bottom)
    const hasInsets = (insets.top | insets.bottom | insets.left | insets.right) !== 0
    const drawOutside = options.drawOutside ?? hasInsets

    const b = Buffer.alloc(20)
    b.writeUInt32LE(safeW, 0)
    b.writeUInt32LE(safeH, 4)
    b.writeUInt32LE(insets.left, 8)
    b.writeUInt32LE(insets.top, 12)
    b.writeUInt32LE(drawOutside ? 1 : 0, 16)

    super(b, FileAddress.HU_SAFEAREA_INFO)
  }
}

export function boxTmpPath(fileName: string): string {
  const base = (fileName.split(/[\\/]/).pop() || fileName).trim()
  const safe = base.length > 0 ? base : 'update.img'
  return `${FileAddress.TMP}/${safe}`
}

export class SendTmpFile extends SendFile {
  constructor(content: Buffer, fileName: string) {
    super(content, boxTmpPath(fileName))
  }
}

export class SendNumber extends SendFile {
  constructor(content: number, file: FileAddress) {
    const message = Buffer.alloc(4)
    message.writeUInt32LE(content)
    super(message, file)
  }
}

export class SendBoolean extends SendNumber {
  constructor(content: boolean, file: FileAddress) {
    super(Number(content), file)
  }
}

export class SendAndroidAutoDpi extends SendNumber {
  constructor(width: number, height: number) {
    super(computeAndroidAutoDpi(width, height), FileAddress.DPI)
  }
}

export class SendString extends SendFile {
  constructor(content: string, file: FileAddress) {
    let clean = content.normalize('NFKD').replace(/[^\u0020-\u007E]/g, '?')
    clean = clean.replace(/[\r\n]+/g, '').slice(0, 16)

    const message = Buffer.from(clean, 'ascii')
    super(message, file)
  }
}

export class HeartBeat extends SendableMessage {
  type = MessageType.HeartBeat
}

export class SendOpen extends SendableMessageWithPayload {
  type = MessageType.Open

  constructor(
    public config: Pick<DongleConfig, 'width' | 'height' | 'fps'>,
    public phoneWorkMode: PhoneWorkMode.CarPlay | PhoneWorkMode.Android
  ) {
    super()
  }

  getPayload(): Buffer {
    const { width, height, fps } = this.config

    const FORMAT = 5
    const PACKET_MAX = 49152
    const IBOX_VERSION = 2

    const b = Buffer.alloc(28)
    b.writeUInt32LE(width, 0)
    b.writeUInt32LE(height, 4)
    b.writeUInt32LE(fps, 8)
    b.writeUInt32LE(FORMAT, 12)
    b.writeUInt32LE(PACKET_MAX, 16)
    b.writeUInt32LE(IBOX_VERSION, 20)
    b.writeUInt32LE(this.phoneWorkMode, 24)
    return b
  }
}

type NaviScreenInfo = {
  width: number
  height: number
  fps: number
  safearea?: {
    width: number
    height: number
    x: number
    y: number
    outside: number
  }
}

type BoxSettingsBody = {
  mediaDelay: number
  syncTime: number
  androidAutoSizeW: number
  androidAutoSizeH: number
  wifiChannel: number
  mediaSound: 0 | 1
  callQuality: 0 | 1 | 2
  gps: 0 | 1
  DashboardInfo: number
  GNSSCapability: number
  autoConn: 0 | 1
  UseBTPhone: 0 | 1
  wifiName: string
  btName: string
  boxName: string
  OemName: string
  naviScreenInfo?: NaviScreenInfo
}

export class SendBoxSettings extends SendableMessageWithPayload {
  type = MessageType.BoxSettings
  private syncTime: number | null
  private config: DongleConfig

  getPayload(): Buffer {
    const cfg = this.config
    const channel: number = Number.isFinite(cfg.wifiChannel)
      ? cfg.wifiChannel
      : cfg.wifiType === '5ghz'
        ? 36
        : 1

    const aaAdjusted = matchFittingAAResolution({
      width: cfg.width,
      height: cfg.height
    })

    const dashboardInfo =
      (cfg.dashboardMediaInfo ? 1 : 0) |
      (cfg.dashboardVehicleInfo ? 2 : 0) |
      (cfg.dashboardRouteInfo ? 4 : 0)

    const gnssCapability =
      (cfg.gnssGps ? 1 : 0) |
      (cfg.gnssGlonass ? 2 : 0) |
      (cfg.gnssGalileo ? 4 : 0) |
      (cfg.gnssBeiDou ? 8 : 0)

    const body: BoxSettingsBody = {
      mediaDelay: cfg.mediaDelay,
      syncTime: this.syncTime ?? getCurrentTimeInMs(),
      androidAutoSizeW: aaAdjusted.width,
      androidAutoSizeH: aaAdjusted.height,
      wifiChannel: channel,
      mediaSound: cfg.mediaSound,
      callQuality: cfg.callQuality,
      gps: cfg.gps ? 1 : 0,
      DashboardInfo: dashboardInfo,
      GNSSCapability: gnssCapability,
      autoConn: cfg.autoConn ? 1 : 0,
      UseBTPhone: cfg.UseBTPhone ? 1 : 0,
      wifiName: cfg.carName,
      btName: cfg.carName,
      boxName: cfg.oemName ?? cfg.carName,
      OemName: cfg.oemName ?? cfg.carName
    }

    if (cfg.mapsEnabled) {
      const insets = {
        top: cfg.naviSafeAreaTop ?? 0,
        bottom: cfg.naviSafeAreaBottom ?? 0,
        left: cfg.naviSafeAreaLeft ?? 0,
        right: cfg.naviSafeAreaRight ?? 0
      }

      const safeW = Math.max(0, cfg.naviWidth - insets.left - insets.right)
      const safeH = Math.max(0, cfg.naviHeight - insets.top - insets.bottom)

      const hasInsets = (insets.top | insets.bottom | insets.left | insets.right) !== 0
      const drawOutside = cfg.naviSafeAreaDrawOutside ?? hasInsets

      body.naviScreenInfo = {
        width: cfg.naviWidth,
        height: cfg.naviHeight,
        fps: cfg.naviFps,
        safearea: {
          width: safeW,
          height: safeH,
          x: insets.left,
          y: insets.top,
          outside: drawOutside ? 1 : 0
        }
      }
    }
    if (DEBUG) {
      console.log('[SendBoxSettings]', JSON.stringify(body, null, 2))
    }
    return Buffer.from(JSON.stringify(body), 'ascii')
  }

  constructor(config: DongleConfig, syncTime: number | null = null) {
    super()
    this.config = config
    this.syncTime = syncTime
  }
}

export enum LogoType {
  HomeButton = 1,
  Siri = 2
}

export class SendLogoType extends SendableMessageWithPayload {
  type = MessageType.LogoType
  logoType: LogoType

  getPayload(): Buffer {
    const data = Buffer.alloc(4)
    data.writeUInt32LE(this.logoType)
    return data
  }

  constructor(logoType: LogoType) {
    super()
    this.logoType = logoType
  }
}

export class SendIconConfig extends SendFile {
  constructor(config: { oemName?: string }) {
    const valueMap: {
      oemIconVisible: number
      name: string
      model: string
      oemIconPath: string
      oemIconLabel?: string
    } = {
      oemIconVisible: 1,
      name: 'AutoBox',
      model: 'Magic-Car-Link-1.00',
      oemIconPath: FileAddress.OEM_ICON
    }

    const label = (config.oemName ?? '').trim()
    if (label) {
      valueMap.oemIconLabel = label
    }

    const fileData = Object.entries(valueMap)
      .map(([k, v]) => `${k} = ${v}`)
      .join('\n')

    super(Buffer.from(fileData + '\n', 'ascii'), FileAddress.AIRPLAY_CONFIG)
  }
}

export class SendCloseDongle extends SendableMessage {
  type = MessageType.CloseDongle
}

export class SendDisconnectPhone extends SendableMessage {
  type = MessageType.DisconnectPhone
}

export class SendAutoConnectByBtAddress extends SendableMessageWithPayload {
  type = MessageType.WifiStatusData
  private readonly payload: Buffer

  constructor(btMac: string) {
    super()
    this.payload = Buffer.from(btMac, 'ascii')
  }

  getPayload(): Buffer {
    return this.payload
  }
}

export class SendForgetBluetoothAddr extends SendableMessageWithPayload {
  type = MessageType.ForgetBluetoothAddr
  private readonly payload: Buffer

  constructor(btMac: string) {
    super()
    this.payload = Buffer.from(btMac, 'ascii')
  }

  getPayload(): Buffer {
    return this.payload
  }
}

export class SendNaviFocusRequest extends SendableMessage {
  type = MessageType.NaviFocusRequest
}

export class SendNaviFocusRelease extends SendableMessage {
  type = MessageType.NaviFocusRelease
}

export class SendServerCgiScript extends SendFile {
  constructor() {
    const script = buildServerCgiScript()
    const payload = Buffer.from(script, 'utf8')
    super(payload, FileAddress.LIVI_CGI)
  }
}

export class SendLiviWeb extends SendFile {
  constructor() {
    const html = buildLiviWeb()
    const payload = Buffer.from(html, 'utf8')
    super(payload, FileAddress.LIVI_WEB)
  }
}
