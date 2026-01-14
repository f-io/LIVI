import { DongleConfig } from '../driver/DongleDriver.js'
import { MessageType, MessageHeader, CommandMapping, CommandValue } from './common.js'
import { clamp, getCurrentTimeInMs } from './utils.js'

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

export enum TouchAction {
  Down = 14,
  Move = 15,
  Up = 16
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

export enum MultiTouchAction {
  Down = 1,
  Move = 2,
  Up = 0
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

export type MultiTouchPoint = {
  id: number
  x: number
  y: number
  action: MultiTouchAction
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

  getPayload(): Buffer {
    const audioData = Buffer.alloc(12)
    audioData.writeUInt32LE(5, 0)
    audioData.writeFloatLE(0.0, 4)
    audioData.writeUInt32LE(3, 8)
    return Buffer.concat([audioData, Buffer.from(this.data.buffer)])
  }

  constructor(data: Int16Array) {
    super()
    this.data = data
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
  TMP = '/tmp'
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
  config: DongleConfig

  getPayload(): Buffer {
    const { config } = this
    const width = Buffer.alloc(4)
    width.writeUInt32LE(config.width)
    const height = Buffer.alloc(4)
    height.writeUInt32LE(config.height)
    const fps = Buffer.alloc(4)
    fps.writeUInt32LE(config.fps)
    const format = Buffer.alloc(4)
    format.writeUInt32LE(config.format)
    const packetMax = Buffer.alloc(4)
    packetMax.writeUInt32LE(config.packetMax)
    const iBox = Buffer.alloc(4)
    iBox.writeUInt32LE(config.iBoxVersion)
    const phoneMode = Buffer.alloc(4)
    phoneMode.writeUInt32LE(config.phoneWorkMode)
    return Buffer.concat([width, height, fps, format, packetMax, iBox, phoneMode])
  }

  constructor(config: DongleConfig) {
    super()
    this.config = config
  }
}

type BoxSettingsBody = {
  mediaDelay: number
  syncTime: number
  androidAutoSizeW: number
  androidAutoSizeH: number
  WiFiChannel: number
  wifiChannel: number
  mediaSound: 0 | 1
  callQuality: 0 | 1 | 2
  autoPlay: boolean
  autoConn: boolean
  wifiName: string
  btName: string
  boxName: string
  OemName: string
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

    const body: BoxSettingsBody = {
      mediaDelay: cfg.mediaDelay,
      syncTime: this.syncTime ?? getCurrentTimeInMs(),
      androidAutoSizeW: cfg.width,
      androidAutoSizeH: cfg.height,
      WiFiChannel: channel,
      wifiChannel: channel,
      mediaSound: cfg.mediaSound,
      callQuality: cfg.callQuality,
      autoPlay: cfg.autoPlay,
      autoConn: cfg.autoConn,
      wifiName: cfg.carName,
      btName: cfg.carName,
      boxName: cfg.oemName ?? cfg.carName,
      OemName: cfg.oemName ?? cfg.carName
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
