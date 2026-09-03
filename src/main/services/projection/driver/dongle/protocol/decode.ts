/** CarlinKit wire payloads → internal projection events. */

import {
  AudioData,
  BluetoothAddress,
  BluetoothDeviceName,
  BluetoothPairedList,
  BluetoothPeerConnected,
  BluetoothPeerConnecting,
  BluetoothPIN,
  BoxInfo,
  type BoxInfoSettings,
  BoxUpdateProgress,
  BoxUpdateState,
  Command,
  DongleReady,
  GnssData,
  HiCarLink,
  ManufacturerInfo,
  MediaData,
  MediaType,
  type Message,
  NavigationData,
  NavigationMetaType,
  Opened,
  Phase,
  Plugged,
  SoftwareVersion,
  Unplugged,
  VendorSessionInfo,
  WifiDeviceName
} from '@projection/messages'
import type { NaviInfo } from '@shared/types/NavigationTypes'
import { MessageType } from './wire.js'

function ascii(data: Buffer): string {
  return data.toString('ascii')
}

// The helper keeps the samples, only the one byte commands reach this side.
function decodeAudioData(data: Buffer): AudioData | null {
  if (data.length !== 13) return null
  return new AudioData({
    decodeType: data.readUInt32LE(0),
    audioType: data.readUInt32LE(8),
    command: data.readUInt8(12)
  })
}

function isAsciiBase64(buf: Buffer): boolean {
  // allow trailing NUL
  const s = buf.toString('ascii').replace(/\0+$/g, '').trim()
  if (!s) return false

  // reject if contains non-base64 chars
  // (we accept newlines just in case)
  return /^[A-Za-z0-9+/=\r\n]+$/.test(s)
}

function decodeMediaData(mediaType: MediaType, payloadOnly: Buffer): MediaData {
  // innerType=2 and innerType=3 are both treated as album art.
  // If payload is ASCII base64, keep it as-is, otherwise encode raw binary.
  if (mediaType === MediaType.AlbumCover || mediaType === MediaType.AlbumCoverAlt) {
    const base64Image = isAsciiBase64(payloadOnly)
      ? payloadOnly.toString('ascii').replace(/\0+$/g, '').trim()
      : payloadOnly.toString('base64')
    return new MediaData(mediaType, { type: MediaType.AlbumCoverAlt, base64Image })
  }

  if (mediaType === MediaType.Data) {
    const jsonBytes = payloadOnly.subarray(0, Math.max(0, payloadOnly.length - 1))
    try {
      return new MediaData(mediaType, {
        type: mediaType,
        media: JSON.parse(jsonBytes.toString('utf8'))
      })
    } catch {
      return new MediaData(mediaType)
    }
  }

  // The caller only dispatches the four known types, so the trigger is the tail.
  return new MediaData(mediaType, { type: MediaType.ControlAutoplayTrigger })
}

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

function decodeNavigationData(metaType: NavigationMetaType, payloadOnly: Buffer): NavigationData {
  if (metaType === NavigationMetaType.DashboardImage) {
    return new NavigationData(metaType, { NaviImageBase64: payloadOnly.toString('base64') })
  }

  let s = payloadOnly.toString('utf8')
  const nul = s.indexOf('\u0000')
  if (nul !== -1) s = s.slice(0, nul)

  return new NavigationData(metaType, parseNaviInfoFromBuffer(payloadOnly), s)
}

export function parseMetaMessage(data: Buffer): MediaData | NavigationData | null {
  const innerType = data.readUInt32LE(0)
  const payloadOnly = data.subarray(4)

  if (
    innerType === NavigationMetaType.DashboardInfo ||
    innerType === NavigationMetaType.DashboardImage
  ) {
    return decodeNavigationData(innerType as NavigationMetaType, payloadOnly)
  }

  if (
    innerType === MediaType.Data ||
    innerType === MediaType.AlbumCover ||
    innerType === MediaType.AlbumCoverAlt ||
    innerType === MediaType.ControlAutoplayTrigger
  ) {
    return decodeMediaData(innerType as MediaType, payloadOnly)
  }

  const head = data.subarray(0, Math.min(64, data.length))
  console.info(
    `Unexpected meta innerType: ${innerType}, bytes=${data.length}, head=${head.toString('hex')}`
  )
  const text = payloadOnly.toString('utf8')
  const trimmed = text.replace(/\0+$/g, '').trim()
  if (trimmed.length > 0) {
    console.info(
      `Unexpected meta innerType: ${innerType}, utf8=${JSON.stringify(trimmed.slice(0, 200))}`
    )
  }
  return null
}

export function decodeMessage(type: MessageType, data?: Buffer): Message | null {
  if (data) {
    switch (type) {
      case MessageType.AudioData:
        return decodeAudioData(data)
      case MessageType.MetaData:
        return parseMetaMessage(data)
      case MessageType.GnssData:
        return new GnssData(data.toString('ascii').replace(/\0+$/g, ''))
      case MessageType.BluetoothAddress:
        return new BluetoothAddress(ascii(data))
      case MessageType.BluetoothDeviceName:
        return new BluetoothDeviceName(ascii(data))
      case MessageType.BluetoothPIN:
        return new BluetoothPIN(ascii(data))
      case MessageType.ManufacturerInfo:
        return new ManufacturerInfo(data.readUInt32LE(0), data.readUInt32LE(4))
      case MessageType.SoftwareVersion:
        return new SoftwareVersion(
          data
            .toString('ascii')
            .replace(/\0+$/g, '')
            .trim()
            .replace(/^(\d{4}\.\d{2}\.\d{2}\.\d{4}).*$/, '$1')
        )
      case MessageType.Command:
        return new Command(data.readUInt32LE(0))
      case MessageType.Plugged:
        return Buffer.byteLength(data) === 8
          ? new Plugged(data.readUInt32LE(0), data.readUInt32LE(4))
          : new Plugged(data.readUInt32LE(0))
      case MessageType.WifiDeviceName:
        return new WifiDeviceName(ascii(data))
      case MessageType.HiCarLink:
        return new HiCarLink(ascii(data))
      case MessageType.BluetoothPairedList:
        return new BluetoothPairedList(data.toString('utf8').replace(/\0+$/g, ''))
      case MessageType.Open:
        return new Opened({
          width: data.readUInt32LE(0),
          height: data.readUInt32LE(4),
          fps: data.readUInt32LE(8),
          format: data.readUInt32LE(12),
          packetMax: data.readUInt32LE(16),
          iBox: data.readUInt32LE(20),
          phoneMode: data.readUInt32LE(24)
        })
      case MessageType.BoxSettings:
        return new BoxInfo(JSON.parse(data.toString('utf8')) as BoxInfoSettings)
      case MessageType.Phase:
        return new Phase(data.readUInt32LE(0))
      case MessageType.UpdateProgress:
        return new BoxUpdateProgress(data.readInt32LE(0))
      case MessageType.UpdateState:
        return new BoxUpdateState(data.readInt32LE(0))
      case MessageType.PeerBluetoothAddress:
        return new BluetoothPeerConnecting(ascii(data))
      case MessageType.PeerBluetoothAddressAlt:
        return new BluetoothPeerConnected(ascii(data))
      case MessageType.VendorSessionInfo:
        return new VendorSessionInfo(data)
      default: {
        const head = data.subarray(0, Math.min(64, data.length))
        console.warn(
          `[PROJECTION][MSG] Unknown type=0x${type.toString(16)} (${type}) len=${data.length} head=${head.toString('hex')}`
        )
        const text = data.toString('utf8').replace(/\0+$/g, '').trim()
        if (text.length > 0) {
          console.warn(
            `[PROJECTION][MSG] Unknown type=0x${type.toString(16)} (${type}) utf8=${JSON.stringify(text.slice(0, 200))}`
          )
        }
        return null
      }
    }
  }

  switch (type) {
    case MessageType.Open:
      return new DongleReady()
    case MessageType.Unplugged:
      return new Unplugged()
    case MessageType.UiHidePeerInfo:
      return null
    case MessageType.UiBringToForeground:
      return null
    default: {
      console.warn(
        `[PROJECTION][MSG] Unknown type without payload=0x${type.toString(16)} (${type})`
      )
      return null
    }
  }
}
