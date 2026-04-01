import {
  Message,
  AudioData,
  VideoData,
  MetaData,
  BluetoothAddress,
  BluetoothDeviceName,
  BluetoothPIN,
  ManufacturerInfo,
  SoftwareVersion,
  Command,
  Plugged,
  WifiDeviceName,
  HiCarLink,
  BluetoothPairedList,
  GnssData,
  Opened,
  DongleReady,
  BoxInfo,
  Unplugged,
  Phase,
  BluetoothPeerConnecting,
  BluetoothPeerConnected,
  BoxUpdateProgress,
  BoxUpdateState,
  VendorSessionInfo
} from './readable.js'

export enum MessageType {
  Open = 0x01,
  Plugged = 0x02,
  Phase = 0x03,
  Unplugged = 0x04,
  Touch = 0x05,
  VideoData = 0x06,
  AudioData = 0x07,
  Command = 0x08,
  LogoType = 0x09,
  BluetoothAddress = 0x0a,
  CarplayControl = 0x0b,
  BluetoothPIN = 0x0c,
  BluetoothDeviceName = 0x0d,
  WifiDeviceName = 0x0e,
  DisconnectPhone = 0x0f,
  DashboardData = 0x10,
  WifiStatusData = 0x11,
  BluetoothPairedList = 0x12,
  DiskInfo = 0x13,
  ManufacturerInfo = 0x14,
  CloseDongle = 0x15,
  CameraInfo = 0x16,
  MultiTouch = 0x17,
  HiCarLink = 0x18,
  BoxSettings = 0x19,
  ForgetBluetoothAddr = 0x22,
  PeerBluetoothAddress = 0x23,
  PeerBluetoothAddressAlt = 0x24,
  UiHidePeerInfo = 0x25,
  UiBringToForeground = 0x26,
  GnssData = 0x29,
  MetaData = 0x2a,
  ShowPinCode = 0x2b,
  NaviVideoData = 0x2c,
  NaviFocusRequest = 0x6e,
  NaviFocusRelease = 0x6f,
  FactorySetting = 0x77,
  DebugTest = 0x88,
  SendFile = 0x99,
  ExtendedManufacturerInfo = 0xa1,
  VendorSessionInfo = 0xa3,
  HeartBeat = 0xaa,
  UpdateProgress = 0xb1,
  UpdateState = 0xbb,
  SoftwareVersion = 0xcc,
  EnableCrypt = 0xf0,
  DebugTrace = 0xff
}

export type ProjectionyMessageTapPayload = {
  type: number
  length: number
  dataLength: number
  data?: Buffer
}

type ProjectionyMessageTap = (p: ProjectionyMessageTapPayload) => void
let projectionMessageTap: ProjectionyMessageTap | null = null

export function setProjectionyMessageTap(tap: ProjectionyMessageTap | null) {
  projectionMessageTap = tap
}

export class HeaderBuildError extends Error {}

export class MessageHeader {
  length: number
  type: MessageType

  public constructor(length: number, type: MessageType) {
    this.length = length
    this.type = type
  }

  static fromBuffer(data: Buffer): MessageHeader {
    if (data.length !== 16) {
      throw new HeaderBuildError(`Invalid buffer size - Expecting 16, got ${data.length}`)
    }
    const magic = data.readUInt32LE(0)
    if (magic !== MessageHeader.magic) {
      throw new HeaderBuildError(`Invalid magic number, received ${magic}`)
    }
    const length = data.readUInt32LE(4)
    const msgType: MessageType = data.readUInt32LE(8)
    const typeCheck = data.readUInt32LE(12)
    if (typeCheck != ((msgType ^ -1) & 0xffffffff) >>> 0) {
      throw new HeaderBuildError(`Invalid type check, received ${typeCheck}`)
    }
    return new MessageHeader(length, msgType)
  }

  static asBuffer(messageType: MessageType, byteLength: number): Buffer {
    const dataLen = Buffer.alloc(4)
    dataLen.writeUInt32LE(byteLength)
    const type = Buffer.alloc(4)
    type.writeUInt32LE(messageType)
    const typeCheck = Buffer.alloc(4)
    typeCheck.writeUInt32LE(((messageType ^ -1) & 0xffffffff) >>> 0)
    const magicNumber = Buffer.alloc(4)
    magicNumber.writeUInt32LE(MessageHeader.magic)
    return Buffer.concat([magicNumber, dataLen, type, typeCheck])
  }

  toMessage(data?: Buffer): Message | null {
    const { type } = this

    if (projectionMessageTap) {
      try {
        projectionMessageTap({
          type,
          length: this.length,
          dataLength: data?.length ?? 0,
          data
        })
      } catch {}
    }

    if (data) {
      switch (type) {
        case MessageType.AudioData:
          return new AudioData(this, data)
        case MessageType.VideoData:
          return new VideoData(this, data)
        case MessageType.NaviVideoData:
          return new VideoData(this, data)
        case MessageType.MetaData:
          return new MetaData(this, data)
        case MessageType.GnssData:
          return new GnssData(this, data)
        case MessageType.BluetoothAddress:
          return new BluetoothAddress(this, data)
        case MessageType.BluetoothDeviceName:
          return new BluetoothDeviceName(this, data)
        case MessageType.BluetoothPIN:
          return new BluetoothPIN(this, data)
        case MessageType.ManufacturerInfo:
          return new ManufacturerInfo(this, data)
        case MessageType.SoftwareVersion:
          return new SoftwareVersion(this, data)
        case MessageType.Command:
          return new Command(this, data)
        case MessageType.Plugged:
          return new Plugged(this, data)
        case MessageType.WifiDeviceName:
          return new WifiDeviceName(this, data)
        case MessageType.HiCarLink:
          return new HiCarLink(this, data)
        case MessageType.BluetoothPairedList:
          return new BluetoothPairedList(this, data)
        case MessageType.Open:
          return new Opened(this, data)
        case MessageType.BoxSettings:
          return new BoxInfo(this, data)
        case MessageType.Phase:
          return new Phase(this, data)
        case MessageType.UpdateProgress:
          return new BoxUpdateProgress(this, data)
        case MessageType.UpdateState:
          return new BoxUpdateState(this, data)
        case MessageType.PeerBluetoothAddress:
          return new BluetoothPeerConnecting(this, data)
        case MessageType.PeerBluetoothAddressAlt:
          return new BluetoothPeerConnected(this, data)
        case MessageType.VendorSessionInfo:
          return new VendorSessionInfo(this, data)
        default: {
          const head = data.subarray(0, Math.min(64, data.length))
          console.warn(
            `[CARPLAY][MSG] Unknown type=0x${type.toString(16)} (${type}) len=${this.length} dataLen=${data.length} head=${head.toString('hex')}`
          )
          const text = data.toString('utf8').replace(/\0+$/g, '').trim()
          if (text.length > 0) {
            console.warn(
              `[CARPLAY][MSG] Unknown type=0x${type.toString(16)} (${type}) utf8=${JSON.stringify(text.slice(0, 200))}`
            )
          }
          return null
        }
      }
    } else {
      switch (type) {
        case MessageType.Open:
          return new DongleReady(this)
        case MessageType.Unplugged:
          return new Unplugged(this)
        case MessageType.UiHidePeerInfo:
          return null
        case MessageType.UiBringToForeground:
          return null
        default: {
          console.warn(
            `[CARPLAY][MSG] Unknown type without payload=0x${type.toString(16)} (${type}) len=${this.length}`
          )
          return null
        }
      }
    }
  }

  static dataLength = 16
  static magic = 0x55aa55aa
}
