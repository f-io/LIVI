/** CarlinKit-only sendables and the wire encoding for the generic commands. */

import { DEBUG } from '@main/constants'
import { buildServerCgiScript } from '@projection/assets/LIVI_cgi.js'
import { buildLiviWeb } from '@projection/assets/LIVI_web.js'
import {
  SendAutoConnectByBtAddress,
  SendableMessage,
  SendBluetoothPairedList,
  SendCloseDongle,
  SendClusterFocusRelease,
  SendClusterFocusRequest,
  SendCommand,
  SendDisconnectPhone,
  SendForgetBluetoothAddr,
  SendMultiTouch,
  SendTouch
} from '@projection/messages/sendable'
import type { Config } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import {
  clamp,
  computeAndroidAutoDpi,
  dongleDisplayName,
  getCurrentTimeInMs,
  isClusterDisplayed,
  matchFittingAAResolution,
  toEven
} from '@shared/utils'
import {
  DONGLE_CALL_QUALITY,
  DONGLE_DASHBOARD_INFO,
  DONGLE_GNSS_CAPABILITY,
  DONGLE_GPS,
  DONGLE_MEDIA_DELAY
} from '../dongleConfig'
import { MessageType } from './wire.js'

// Dongle Open wire payload (projection resolution sent to the dongle).
export type OpenConfig = { width: number; height: number; fps: number }

export abstract class DongleSendable extends SendableMessage {
  abstract type: MessageType

  payload(): Buffer {
    return Buffer.alloc(0)
  }
}

export abstract class DongleSendableWithPayload extends DongleSendable {
  abstract getPayload(): Buffer

  override payload(): Buffer {
    return this.getPayload()
  }
}

// HU GPS relayed to the phone as NMEA sentences.
export class SendGnssData extends DongleSendableWithPayload {
  type = MessageType.GnssData
  readonly nmeaText: string

  constructor(nmeaText: string) {
    super()
    this.nmeaText = String(nmeaText ?? '')
      .replace(/\r?\n/g, '\r\n')
      .trim()
  }

  getPayload(): Buffer {
    const withLineEnd = this.nmeaText.length > 0 ? this.nmeaText + '\r\n' : ''
    return Buffer.from(withLineEnd, 'ascii')
  }
}

export class SendFile extends DongleSendableWithPayload {
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
  HU_NAVISCREEN_VIEWAREA_INFO = '/etc/RiddleBoxData/HU_NAVISCREEN_VIEWAREA_INFO',
  HU_NAVISCREEN_SAFEAREA_INFO = '/etc/RiddleBoxData/HU_NAVISCREEN_SAFEAREA_INFO',
  TMP = '/tmp'
}

export type ViewAreaOptions = {
  insets?: Partial<ScreenInsets>
  address?: FileAddress
}

export type SafeAreaOptions = {
  insets?: Partial<ScreenInsets>
  drawOutside?: boolean
  address?: FileAddress
}

export type ScreenInsets = {
  top: number
  bottom: number
  left: number
  right: number
}

// 24 bytes LE: [screenW, screenH, viewW, viewH, originX, originY]
export class SendViewArea extends SendFile {
  constructor(screenW: number, screenH: number, options: ViewAreaOptions = {}) {
    const top = toEven(Math.max(0, options.insets?.top ?? 0))
    const left = toEven(Math.max(0, options.insets?.left ?? 0))
    const right = Math.max(0, options.insets?.right ?? 0)
    const bottom = Math.max(0, options.insets?.bottom ?? 0)
    const viewW = toEven(Math.max(0, toEven(screenW) - left - right))
    const viewH = toEven(Math.max(0, toEven(screenH) - top - bottom))

    const b = Buffer.alloc(24)
    b.writeUInt32LE(toEven(screenW), 0)
    b.writeUInt32LE(toEven(screenH), 4)
    b.writeUInt32LE(viewW, 8)
    b.writeUInt32LE(viewH, 12)
    b.writeUInt32LE(left, 16)
    b.writeUInt32LE(top, 20)

    super(b, options.address ?? FileAddress.HU_VIEWAREA_INFO)
  }
}

// 20 bytes LE: [safeW, safeH, originX, originY, drawOutside]
export class SendSafeArea extends SendFile {
  constructor(videoW: number, videoH: number, options: SafeAreaOptions = {}) {
    const top = toEven(Math.max(0, options.insets?.top ?? 0))
    const left = toEven(Math.max(0, options.insets?.left ?? 0))
    const right = Math.max(0, options.insets?.right ?? 0)
    const bottom = Math.max(0, options.insets?.bottom ?? 0)
    const safeW = toEven(Math.max(0, toEven(videoW) - left - right))
    const safeH = toEven(Math.max(0, toEven(videoH) - top - bottom))
    const hasInsets = (top | bottom | left | right) !== 0
    const drawOutside = options.drawOutside ?? hasInsets

    const b = Buffer.alloc(20)
    b.writeUInt32LE(safeW, 0)
    b.writeUInt32LE(safeH, 4)
    b.writeUInt32LE(left, 8)
    b.writeUInt32LE(top, 12)
    b.writeUInt32LE(drawOutside ? 1 : 0, 16)

    super(b, options.address ?? FileAddress.HU_SAFEAREA_INFO)
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
    let clean = content.normalize('NFKD').replace(/[^ -~]/g, '?')
    clean = clean.replace(/[\r\n]+/g, '').slice(0, 16)

    const message = Buffer.from(clean, 'ascii')
    super(message, file)
  }
}

export class HeartBeat extends DongleSendable {
  type = MessageType.HeartBeat
}

export class SendOpen extends DongleSendableWithPayload {
  type = MessageType.Open

  constructor(
    public config: OpenConfig,
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

export class SendBoxSettings extends DongleSendableWithPayload {
  type = MessageType.BoxSettings
  private syncTime: number | null
  private config: Config

  getPayload(): Buffer {
    const cfg = this.config
    const channel: number = Number.isFinite(cfg.wifiChannel)
      ? cfg.wifiChannel
      : cfg.wifiType === '5ghz'
        ? 36
        : 1

    const aaAdjusted = matchFittingAAResolution({
      width: cfg.projectionWidth,
      height: cfg.projectionHeight
    })

    const body: BoxSettingsBody = {
      mediaDelay: DONGLE_MEDIA_DELAY,
      syncTime: this.syncTime ?? getCurrentTimeInMs(),
      androidAutoSizeW: aaAdjusted.width,
      androidAutoSizeH: aaAdjusted.height,
      wifiChannel: channel,
      mediaSound: cfg.samplingFrequency,
      callQuality: DONGLE_CALL_QUALITY,
      gps: DONGLE_GPS,
      DashboardInfo: DONGLE_DASHBOARD_INFO,
      GNSSCapability: DONGLE_GNSS_CAPABILITY,
      autoConn: cfg.autoConn ? 1 : 0,
      UseBTPhone: cfg.UseBTPhone ? 1 : 0,
      wifiName: dongleDisplayName(cfg.carName),
      btName: dongleDisplayName(cfg.carName),
      boxName: cfg.oemName ?? cfg.carName,
      OemName: cfg.oemName ?? cfg.carName
    }

    if (isClusterDisplayed(cfg)) {
      const cW = cfg.clusterWidth
      const cH = cfg.clusterHeight
      const cF = cfg.clusterFps

      // `naviScreenInfo` registers the cluster screen. Its safearea sub-object is
      // kept full-size, the cluster view/safe area is delivered via the
      // HU_NAVISCREEN_*_INFO files instead (see dongleDriver).
      body.naviScreenInfo = {
        width: cW,
        height: cH,
        fps: cF,
        safearea: {
          width: cW,
          height: cH,
          x: 0,
          y: 0,
          outside: 0
        }
      }
    }
    if (DEBUG) {
      console.log('[SendBoxSettings]', JSON.stringify(body, null, 2))
    }
    return Buffer.from(JSON.stringify(body), 'ascii')
  }

  constructor(config: Config, syncTime: number | null = null) {
    super()
    this.config = config
    this.syncTime = syncTime
  }
}

export enum LogoType {
  HomeButton = 1,
  Siri = 2
}

export class SendLogoType extends DongleSendableWithPayload {
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

export type DongleMessage = { type: MessageType; payload: Buffer }

/** Message type and payload for any sendable, generic command or dongle-specific. */
export function encodeDongle(msg: SendableMessage): DongleMessage {
  if (msg instanceof DongleSendable) return { type: msg.type, payload: msg.payload() }
  if (msg instanceof SendCommand) {
    const data = Buffer.alloc(4)
    data.writeUInt32LE(msg.value)
    return { type: MessageType.Command, payload: data }
  }
  if (msg instanceof SendTouch) {
    const actionB = Buffer.alloc(4)
    const xB = Buffer.alloc(4)
    const yB = Buffer.alloc(4)
    const flags = Buffer.alloc(4)
    actionB.writeUInt32LE(msg.action)
    xB.writeUInt32LE(clamp(10000 * msg.x, 0, 10000))
    yB.writeUInt32LE(clamp(10000 * msg.y, 0, 10000))
    return { type: MessageType.Touch, payload: Buffer.concat([actionB, xB, yB, flags]) }
  }
  if (msg instanceof SendMultiTouch) {
    const items = msg.touches.map((t) => {
      const b = Buffer.alloc(16)
      b.writeFloatLE(t.x, 0)
      b.writeFloatLE(t.y, 4)
      b.writeUInt32LE(t.action, 8)
      b.writeUInt32LE(t.id, 12)
      return b
    })
    return { type: MessageType.MultiTouch, payload: Buffer.concat(items) }
  }
  if (msg instanceof SendBluetoothPairedList) {
    const withNul = msg.listText.endsWith('\0') ? msg.listText : msg.listText + '\0'
    return { type: MessageType.BluetoothPairedList, payload: Buffer.from(withNul, 'utf8') }
  }
  if (msg instanceof SendAutoConnectByBtAddress) {
    return { type: MessageType.WifiStatusData, payload: Buffer.from(msg.btMac, 'ascii') }
  }
  if (msg instanceof SendForgetBluetoothAddr) {
    return { type: MessageType.ForgetBluetoothAddr, payload: Buffer.from(msg.btMac, 'ascii') }
  }
  const empty = Buffer.alloc(0)
  if (msg instanceof SendCloseDongle) return { type: MessageType.CloseDongle, payload: empty }
  if (msg instanceof SendDisconnectPhone)
    return { type: MessageType.DisconnectPhone, payload: empty }
  if (msg instanceof SendClusterFocusRequest) {
    return { type: MessageType.ClusterFocusRequest, payload: empty }
  }
  if (msg instanceof SendClusterFocusRelease) {
    return { type: MessageType.ClusterFocusRelease, payload: empty }
  }
  throw new Error(`No dongle wire encoding for ${msg.constructor.name}`)
}
