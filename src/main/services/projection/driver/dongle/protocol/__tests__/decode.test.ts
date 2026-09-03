import {
  AudioData,
  BluetoothAddress,
  BluetoothDeviceName,
  BluetoothPairedList,
  BluetoothPeerConnected,
  BluetoothPeerConnecting,
  BluetoothPIN,
  BoxInfo,
  BoxPhase,
  BoxUpdateProgress,
  BoxUpdateState,
  BoxUpdateStatus,
  Command,
  DongleReady,
  GnssData,
  HiCarLink,
  ManufacturerInfo,
  MediaData,
  MediaType,
  NavigationData,
  NavigationMetaType,
  Opened,
  Phase,
  PhoneType,
  Plugged,
  SoftwareVersion,
  Unplugged,
  VendorSessionInfo,
  WifiDeviceName
} from '@projection/messages'
import { decodeMessage, parseMetaMessage, parseNaviInfoFromBuffer } from '../decode.js'
import { MessageType } from '../wire.js'

function decode(type: MessageType, data: Buffer) {
  return decodeMessage(type, data)
}

function metaPayload(innerType: number, body: Buffer): Buffer {
  const head = Buffer.alloc(4)
  head.writeUInt32LE(innerType, 0)
  return Buffer.concat([head, body])
}

describe('dongle protocol decode', () => {
  test('SoftwareVersion keeps normalized yyyy.mm.dd.hhmm form', () => {
    const msg = decode(
      MessageType.SoftwareVersion,
      Buffer.from('2025.03.19.1126-beta\0', 'ascii')
    ) as SoftwareVersion
    expect(msg.version).toBe('2025.03.19.1126')
  })

  test('SoftwareVersion keeps plain version when no beta suffix exists', () => {
    const msg = decode(
      MessageType.SoftwareVersion,
      Buffer.from('2025.03.19.1126\0', 'ascii')
    ) as SoftwareVersion
    expect(msg.version).toBe('2025.03.19.1126')
  })

  test('SoftwareVersion trims trailing whitespace and NUL bytes', () => {
    const msg = decode(
      MessageType.SoftwareVersion,
      Buffer.from('2025.03.19.1126   \0\0', 'ascii')
    ) as SoftwareVersion
    expect(msg.version).toBe('2025.03.19.1126')
  })

  test('parseNaviInfoFromBuffer parses json and strips trailing NUL', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('{"NaviStatus":1}\0\0', 'utf8'))
    expect(info).toEqual({ NaviStatus: 1 })
  })

  test('parseNaviInfoFromBuffer returns null for invalid json', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('{not-json}\0', 'utf8'))
    expect(info).toBeNull()
  })

  test('parseNaviInfoFromBuffer returns null for empty payload', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('\0\0', 'utf8'))
    expect(info).toBeNull()
  })

  test('parseNaviInfoFromBuffer cuts off content at first NUL byte', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('{"NaviStatus":1}\0TRAILING', 'utf8'))
    expect(info).toEqual({ NaviStatus: 1 })
  })

  test('parseNaviInfoFromBuffer returns null for valid non-object json', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('123', 'utf8'))
    expect(info).toBeNull()
  })

  test('NavigationData stores rawUtf8 and parsed navi', () => {
    const body = Buffer.from('{"NaviDestinationName":"Home"}\0', 'utf8')
    const msg = parseMetaMessage(metaPayload(200, body)) as NavigationData

    expect(msg).toBeInstanceOf(NavigationData)
    expect(msg.rawUtf8).toContain('NaviDestinationName')
    expect(msg.navi).toEqual({ NaviDestinationName: 'Home' })
  })

  test('NavigationData handles invalid json and preserves rawUtf8', () => {
    const body = Buffer.from('{oops}\0', 'utf8')
    const msg = parseMetaMessage(metaPayload(200, body)) as NavigationData

    expect(msg).toBeInstanceOf(NavigationData)
    expect(msg.rawUtf8).toContain('{oops}')
    expect(msg.navi).toBeNull()
  })

  test('NavigationData dashboard image stores base64 image and empty rawUtf8', () => {
    const raw = Buffer.from([1, 2, 3, 4])
    const msg = parseMetaMessage(
      metaPayload(NavigationMetaType.DashboardImage, raw)
    ) as NavigationData

    expect(msg).toBeInstanceOf(NavigationData)
    expect(msg.rawUtf8).toBe('')
    expect(msg.navi).toEqual({
      NaviImageBase64: raw.toString('base64')
    })
  })

  test('NavigationData cuts off text at first NUL byte', () => {
    const body = Buffer.from('{"NaviStatus":1}\0TRAILING', 'utf8')
    const msg = parseMetaMessage(
      metaPayload(NavigationMetaType.DashboardInfo, body)
    ) as NavigationData

    expect(msg.rawUtf8).toBe('{"NaviStatus":1}')
    expect(msg.navi).toEqual({ NaviStatus: 1 })
  })

  test('NavigationData keeps rawUtf8 unchanged when payload has no NUL byte', () => {
    const body = Buffer.from('{"NaviDestinationName":"Home"}', 'utf8')
    const msg = parseMetaMessage(
      metaPayload(NavigationMetaType.DashboardInfo, body)
    ) as NavigationData

    expect(msg.rawUtf8).toBe('{"NaviDestinationName":"Home"}')
    expect(msg.navi).toEqual({ NaviDestinationName: 'Home' })
  })

  test('MediaData handles album cover ascii-base64 payload', () => {
    const b64 = Buffer.from('abcd', 'utf8').toString('base64')
    const msg = parseMetaMessage(
      metaPayload(MediaType.AlbumCoverAlt, Buffer.from(b64 + '\0', 'ascii'))
    ) as MediaData

    expect(msg).toBeInstanceOf(MediaData)
    expect(msg.payload).toEqual({ type: MediaType.AlbumCoverAlt, base64Image: b64 })
  })

  test('MediaData handles standard album cover ascii-base64 payload', () => {
    const b64 = Buffer.from('cover', 'utf8').toString('base64')
    const msg = parseMetaMessage(
      metaPayload(MediaType.AlbumCover, Buffer.from(b64 + '\0'))
    ) as MediaData

    expect(msg.payload).toEqual({
      type: MediaType.AlbumCoverAlt,
      base64Image: b64
    })
  })

  test('MediaData encodes raw binary album cover as base64', () => {
    const raw = Buffer.from([0xde, 0xad, 0xbe, 0xef])
    const msg = parseMetaMessage(metaPayload(MediaType.AlbumCoverAlt, raw)) as MediaData

    expect(msg.payload).toEqual({
      type: MediaType.AlbumCoverAlt,
      base64Image: raw.toString('base64')
    })
  })

  test('MediaData treats empty album cover ascii payload as non-base64 and encodes raw bytes', () => {
    const raw = Buffer.from('\0\0', 'ascii')
    const msg = parseMetaMessage(metaPayload(MediaType.AlbumCoverAlt, raw)) as MediaData

    expect(msg.payload).toEqual({
      type: MediaType.AlbumCoverAlt,
      base64Image: raw.toString('base64')
    })
  })

  test('MediaData parses json media payload', () => {
    const json = JSON.stringify({ MediaSongName: 'Song' }) + '\0'
    const msg = parseMetaMessage(
      metaPayload(MediaType.Data, Buffer.from(json, 'utf8'))
    ) as MediaData

    expect(msg.payload).toEqual({
      type: MediaType.Data,
      media: { MediaSongName: 'Song' }
    })
  })

  test('MediaData does not expose payload for json media payload with trailing double NUL bytes', () => {
    const json = JSON.stringify({ MediaSongName: 'Song', MediaArtistName: 'Artist' }) + '\0\0'
    const msg = parseMetaMessage(
      metaPayload(MediaType.Data, Buffer.from(json, 'utf8'))
    ) as MediaData

    expect(msg.payload).toBeUndefined()
  })

  test('MediaData does not expose payload for invalid json data payload', () => {
    const msg = parseMetaMessage(
      metaPayload(MediaType.Data, Buffer.from('{bad}\0', 'utf8'))
    ) as MediaData

    expect(msg.payload).toBeUndefined()
  })

  test('MediaData handles autoplay trigger payload', () => {
    const msg = parseMetaMessage(
      metaPayload(MediaType.ControlAutoplayTrigger, Buffer.alloc(0))
    ) as MediaData

    expect(msg.payload).toEqual({ type: MediaType.ControlAutoplayTrigger })
  })

  test('decodeMessage routes MetaData through parseMetaMessage', () => {
    const body = Buffer.from(JSON.stringify({ MediaSongName: 'x' }), 'utf8')
    const msg = decode(
      MessageType.MetaData,
      metaPayload(1, Buffer.concat([body, Buffer.from([0])]))
    )
    expect(msg).not.toBeNull()
  })

  test('parseMetaMessage returns null for unknown payloads', () => {
    const infoSpy = vi.spyOn(console, 'info').mockImplementation(function () {})
    const body = Buffer.from('mystery\0', 'utf8')

    const msg = parseMetaMessage(metaPayload(999, body))

    expect(msg).toBeNull()
    expect(infoSpy).toHaveBeenCalled()

    infoSpy.mockRestore()
  })

  test('parseMetaMessage returns null for unknown payloads without logging empty utf8', () => {
    const infoSpy = vi.spyOn(console, 'info').mockImplementation(function () {})
    const body = Buffer.from('\0\0', 'utf8')

    const msg = parseMetaMessage(metaPayload(999, body))

    expect(msg).toBeNull()
    expect(infoSpy).toHaveBeenCalledTimes(1)

    infoSpy.mockRestore()
  })

  test('Command reads uint32 command value', () => {
    const buf = Buffer.alloc(4)
    buf.writeUInt32LE(123, 0)

    const msg = decode(MessageType.Command, buf) as Command

    expect(msg.value).toBe(123)
  })

  test('ManufacturerInfo reads two uint32 values', () => {
    const buf = Buffer.alloc(8)
    buf.writeUInt32LE(11, 0)
    buf.writeUInt32LE(22, 4)

    const msg = decode(MessageType.ManufacturerInfo, buf) as ManufacturerInfo

    expect(msg.a).toBe(11)
    expect(msg.b).toBe(22)
  })

  test('GnssData strips trailing NUL bytes', () => {
    const msg = decode(MessageType.GnssData, Buffer.from('$GPGGA\0\0', 'ascii')) as GnssData
    expect(msg.text).toBe('$GPGGA')
  })

  test('Bluetooth/Wifi/HiCar messages keep ascii text', () => {
    expect(
      (decode(MessageType.BluetoothAddress, Buffer.from('AA:BB', 'ascii')) as BluetoothAddress)
        .address
    ).toBe('AA:BB')
    expect(
      (decode(MessageType.BluetoothPIN, Buffer.from('1234', 'ascii')) as BluetoothPIN).pin
    ).toBe('1234')
    expect(
      (
        decode(
          MessageType.BluetoothDeviceName,
          Buffer.from('My Phone', 'ascii')
        ) as BluetoothDeviceName
      ).name
    ).toBe('My Phone')
    expect(
      (decode(MessageType.WifiDeviceName, Buffer.from('Car Wifi', 'ascii')) as WifiDeviceName).name
    ).toBe('Car Wifi')
    expect(
      (decode(MessageType.HiCarLink, Buffer.from('link://abc', 'ascii')) as HiCarLink).link
    ).toBe('link://abc')
  })

  test('BluetoothPairedList decodes utf8 and strips trailing NUL', () => {
    const msg = decode(
      MessageType.BluetoothPairedList,
      Buffer.from('Gerät A\0', 'utf8')
    ) as BluetoothPairedList
    expect(msg.data).toBe('Gerät A')
  })

  test('Plugged reads phone type only when payload is 4 bytes', () => {
    const buf = Buffer.alloc(4)
    buf.writeUInt32LE(PhoneType.CarPlay, 0)

    const msg = decode(MessageType.Plugged, buf) as Plugged

    expect(msg.phoneType).toBe(PhoneType.CarPlay)
    expect(msg.wifi).toBeUndefined()
  })

  test('Plugged reads phone type and wifi when payload is 8 bytes', () => {
    const buf = Buffer.alloc(8)
    buf.writeUInt32LE(PhoneType.AndroidAuto, 0)
    buf.writeUInt32LE(1, 4)

    const msg = decode(MessageType.Plugged, buf) as Plugged

    expect(msg.phoneType).toBe(PhoneType.AndroidAuto)
    expect(msg.wifi).toBe(1)
  })

  test('AudioData carries a one byte command', () => {
    const buf = Buffer.alloc(13)
    buf.writeUInt32LE(5, 0)
    buf.writeFloatLE(0.5, 4)
    buf.writeUInt32LE(2, 8)
    buf.writeUInt8(4, 12)
    const msg = decode(MessageType.AudioData, buf) as AudioData
    expect(msg).toBeInstanceOf(AudioData)
    expect(msg.decodeType).toBe(5)
    expect(msg.audioType).toBe(2)
    expect(msg.command).toBe(4)
  })

  test('AudioData without a command is nothing for this side', () => {
    const head = Buffer.alloc(12)
    head.writeUInt32LE(5, 0)
    head.writeUInt32LE(2, 8)
    expect(decode(MessageType.AudioData, head)).toBeNull()
    const ramp = Buffer.concat([head, Buffer.alloc(4)])
    expect(decode(MessageType.AudioData, ramp)).toBeNull()
    const pcm = Buffer.concat([head, Buffer.from([1, 0, 2, 0, 3, 0])])
    expect(decode(MessageType.AudioData, pcm)).toBeNull()
  })

  test('Opened reads open payload fields', () => {
    const buf = Buffer.alloc(28)
    buf.writeUInt32LE(800, 0)
    buf.writeUInt32LE(480, 4)
    buf.writeUInt32LE(60, 8)
    buf.writeUInt32LE(5, 12)
    buf.writeUInt32LE(49152, 16)
    buf.writeUInt32LE(2, 20)
    buf.writeUInt32LE(3, 24)

    const msg = decode(MessageType.Open, buf) as Opened

    expect(msg.width).toBe(800)
    expect(msg.height).toBe(480)
    expect(msg.fps).toBe(60)
    expect(msg.format).toBe(5)
    expect(msg.packetMax).toBe(49152)
    expect(msg.iBox).toBe(2)
    expect(msg.phoneMode).toBe(3)
  })

  test('BoxInfo parses json settings', () => {
    const msg = decode(
      MessageType.BoxSettings,
      Buffer.from(
        JSON.stringify({
          uuid: 'u1',
          MFD: 'm1',
          productType: 'A15W',
          DevList: [{ id: '1', name: 'Phone' }]
        }),
        'utf8'
      )
    ) as BoxInfo

    expect(msg.settings).toEqual({
      uuid: 'u1',
      MFD: 'm1',
      productType: 'A15W',
      DevList: [{ id: '1', name: 'Phone' }]
    })
  })

  test('VendorSessionInfo keeps raw buffer', () => {
    const raw = Buffer.from('secret')
    const msg = decode(MessageType.VendorSessionInfo, raw) as VendorSessionInfo

    expect(msg.raw).toBe(raw)
  })

  test('Phase reads uint32 value', () => {
    const buf = Buffer.alloc(4)
    buf.writeUInt32LE(BoxPhase.EVT_BOX_READY, 0)

    const msg = decode(MessageType.Phase, buf) as Phase

    expect(msg.value).toBe(BoxPhase.EVT_BOX_READY)
  })

  test('BoxUpdateProgress reads signed progress int32', () => {
    const buf = Buffer.alloc(4)
    buf.writeInt32LE(-12, 0)

    const msg = decode(MessageType.UpdateProgress, buf) as BoxUpdateProgress

    expect(msg.progress).toBe(-12)
  })

  test('BoxUpdateState reads status from signed int32 payload', () => {
    const buf = Buffer.alloc(4)
    buf.writeInt32LE(BoxUpdateStatus.BoxUpdateSuccess, 0)

    const msg = decode(MessageType.UpdateState, buf) as BoxUpdateState

    expect(msg.status).toBe(BoxUpdateStatus.BoxUpdateSuccess)
    expect(msg.statusText).toBe('EVT_BOX_UPDATE_SUCCESS')
  })
  test('peer bluetooth addresses become connecting and connected events', () => {
    const connecting = decode(MessageType.PeerBluetoothAddress, Buffer.from('AA:BB', 'ascii'))
    const connected = decode(MessageType.PeerBluetoothAddressAlt, Buffer.from('CC:DD', 'ascii'))
    expect(connecting).toBeInstanceOf(BluetoothPeerConnecting)
    expect((connecting as BluetoothPeerConnecting).address).toBe('AA:BB')
    expect(connected).toBeInstanceOf(BluetoothPeerConnected)
    expect((connected as BluetoothPeerConnected).address).toBe('CC:DD')
  })

  test('an unknown type with a payload is logged and returns null', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(decode(0x7777 as MessageType, Buffer.from('hello', 'ascii'))).toBeNull()
    expect(decode(0x7777 as MessageType, Buffer.from([0, 0, 0]))).toBeNull()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('a payloadless Open is a DongleReady and Unplugged an Unplugged', () => {
    expect(decodeMessage(MessageType.Open)).toBeInstanceOf(DongleReady)
    expect(decodeMessage(MessageType.Unplugged)).toBeInstanceOf(Unplugged)
  })

  test('the payloadless UI hints and unknown types are ignored', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(decodeMessage(MessageType.UiHidePeerInfo)).toBeNull()
    expect(decodeMessage(MessageType.UiBringToForeground)).toBeNull()
    expect(decodeMessage(0x7777 as MessageType)).toBeNull()
    expect(warn).toHaveBeenCalledTimes(1)
    warn.mockRestore()
  })
})
