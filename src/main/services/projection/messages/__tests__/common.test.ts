import { HeaderBuildError, MessageHeader, MessageType, setProjectionyMessageTap } from '../common'
import {
  AudioData,
  BluetoothAddress,
  BluetoothDeviceName,
  BluetoothPairedList,
  BluetoothPeerConnected,
  BluetoothPeerConnecting,
  BluetoothPIN,
  BoxInfo,
  BoxUpdateProgress,
  BoxUpdateState,
  Command,
  DongleReady,
  GnssData,
  HiCarLink,
  ManufacturerInfo,
  MetaData,
  Opened,
  Phase,
  Plugged,
  SoftwareVersion,
  Unplugged,
  VendorSessionInfo,
  VideoData,
  WifiDeviceName
} from '../readable'

const createVideoPayload = () => {
  const data = Buffer.alloc(20)
  data.writeUInt32LE(1920, 0)
  data.writeUInt32LE(1080, 4)
  data.writeUInt32LE(1, 8)
  data.writeUInt32LE(123, 12)
  data.writeUInt32LE(0, 16)
  return data
}

const createAudioPayload = () => {
  const data = Buffer.alloc(12)
  data.writeUInt32LE(1, 0) // decodeType
  data.writeFloatLE(0.5, 4) // volume
  data.writeUInt32LE(2, 8) // audioType
  return data
}

const createTwoUInt32Payload = (a: number, b: number) => {
  const data = Buffer.alloc(8)
  data.writeUInt32LE(a, 0)
  data.writeUInt32LE(b, 4)
  return data
}

const createUInt32Payload = (value: number) => {
  const data = Buffer.alloc(4)
  data.writeUInt32LE(value, 0)
  return data
}

const createInt32Payload = (value: number) => {
  const data = Buffer.alloc(4)
  data.writeInt32LE(value, 0)
  return data
}

const createOpenedPayload = () => {
  const data = Buffer.alloc(28)
  data.writeUInt32LE(1920, 0)
  data.writeUInt32LE(1080, 4)
  data.writeUInt32LE(60, 8)
  data.writeUInt32LE(5, 12)
  data.writeUInt32LE(49152, 16) // packetMax
  data.writeUInt32LE(2, 20) // iBox
  data.writeUInt32LE(2, 24) // phoneMode
  return data
}

const createBoxSettingsPayload = () => {
  return Buffer.from(JSON.stringify({}), 'utf8')
}

describe('projection messages common', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    setProjectionyMessageTap(null)
  })

  test('MessageHeader.asBuffer builds a valid 16-byte header', () => {
    const header = MessageHeader.asBuffer(MessageType.VideoData, 123)

    expect(header).toHaveLength(16)
    expect(header.readUInt32LE(0)).toBe(MessageHeader.magic)
    expect(header.readUInt32LE(4)).toBe(123)
    expect(header.readUInt32LE(8)).toBe(MessageType.VideoData)

    const typeCheck = header.readUInt32LE(12)
    expect(typeCheck).toBe(((MessageType.VideoData ^ -1) & 0xffffffff) >>> 0)
  })

  test('MessageHeader.fromBuffer parses a valid header buffer', () => {
    const buffer = MessageHeader.asBuffer(MessageType.AudioData, 42)

    const header = MessageHeader.fromBuffer(buffer)

    expect(header).toBeInstanceOf(MessageHeader)
    expect(header.type).toBe(MessageType.AudioData)
    expect(header.length).toBe(42)
  })

  test('MessageHeader.fromBuffer throws on invalid buffer size', () => {
    expect(() => MessageHeader.fromBuffer(Buffer.alloc(8))).toThrow(HeaderBuildError)
    expect(() => MessageHeader.fromBuffer(Buffer.alloc(8))).toThrow(
      'Invalid buffer size - Expecting 16, got 8'
    )
  })

  test('MessageHeader.fromBuffer throws on invalid magic number', () => {
    const buffer = Buffer.alloc(16)
    buffer.writeUInt32LE(0x12345678, 0)
    buffer.writeUInt32LE(12, 4)
    buffer.writeUInt32LE(MessageType.VideoData, 8)
    buffer.writeUInt32LE(((MessageType.VideoData ^ -1) & 0xffffffff) >>> 0, 12)

    expect(() => MessageHeader.fromBuffer(buffer)).toThrow(HeaderBuildError)
    expect(() => MessageHeader.fromBuffer(buffer)).toThrow('Invalid magic number')
  })

  test('MessageHeader.fromBuffer throws on invalid type check', () => {
    const buffer = Buffer.alloc(16)
    buffer.writeUInt32LE(MessageHeader.magic, 0)
    buffer.writeUInt32LE(12, 4)
    buffer.writeUInt32LE(MessageType.VideoData, 8)
    buffer.writeUInt32LE(0, 12)

    expect(() => MessageHeader.fromBuffer(buffer)).toThrow(HeaderBuildError)
    expect(() => MessageHeader.fromBuffer(buffer)).toThrow('Invalid type check')
  })

  test.each([
    [MessageType.AudioData, AudioData, createAudioPayload()],
    [MessageType.NaviVideoData, VideoData, createVideoPayload()],
    [MessageType.MetaData, MetaData, Buffer.from('meta')],
    [MessageType.GnssData, GnssData, Buffer.from('gnss')],
    [MessageType.BluetoothAddress, BluetoothAddress, Buffer.from('aa:bb:cc')],
    [MessageType.BluetoothDeviceName, BluetoothDeviceName, Buffer.from('phone-name\0')],
    [MessageType.BluetoothPIN, BluetoothPIN, Buffer.from('1234\0')],
    [MessageType.ManufacturerInfo, ManufacturerInfo, createTwoUInt32Payload(1, 2)],
    [MessageType.SoftwareVersion, SoftwareVersion, Buffer.from('1.0.0\0')],
    [MessageType.Command, Command, Buffer.from([1, 0, 0, 0])],
    [MessageType.Plugged, Plugged, createTwoUInt32Payload(1, 2)],
    [MessageType.WifiDeviceName, WifiDeviceName, Buffer.from('wifi-name\0')],
    [MessageType.HiCarLink, HiCarLink, Buffer.from([1])],
    [MessageType.BluetoothPairedList, BluetoothPairedList, Buffer.from('paired')],
    [MessageType.Open, Opened, createOpenedPayload()],
    [MessageType.BoxSettings, BoxInfo, createBoxSettingsPayload()],
    [MessageType.Phase, Phase, createUInt32Payload(2)],
    [MessageType.UpdateProgress, BoxUpdateProgress, createInt32Payload(50)],
    [MessageType.UpdateState, BoxUpdateState, createInt32Payload(1)],
    [MessageType.PeerBluetoothAddress, BluetoothPeerConnecting, Buffer.from('11:22:33')],
    [MessageType.PeerBluetoothAddressAlt, BluetoothPeerConnected, Buffer.from('44:55:66')]
  ])('toMessage maps payload type %s to the expected readable message', (type, Klass, data) => {
    const header = new MessageHeader(data.length, type as MessageType)
    const message = header.toMessage(data)
    expect(message).toBeInstanceOf(Klass as any)
  })

  test('toMessage returns DongleReady for open message without payload', () => {
    const header = new MessageHeader(0, MessageType.Open)

    const message = header.toMessage()

    expect(message).toBeInstanceOf(DongleReady)
  })

  test('toMessage returns Unplugged for unplugged message without payload', () => {
    const header = new MessageHeader(0, MessageType.Unplugged)

    const message = header.toMessage()

    expect(message).toBeInstanceOf(Unplugged)
  })

  test('toMessage returns null for UI-only messages without payload', () => {
    expect(new MessageHeader(0, MessageType.UiHidePeerInfo).toMessage()).toBeNull()
    expect(new MessageHeader(0, MessageType.UiBringToForeground).toMessage()).toBeNull()
  })

  test('toMessage returns null and warns for unknown type without payload', () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})

    const message = new MessageHeader(0, 0xdead as MessageType).toMessage()

    expect(message).toBeNull()
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('Unknown type without payload=0xdead')
    )

    warnSpy.mockRestore()
  })

  test('toMessage returns VideoData for video payload messages', () => {
    const data = createVideoPayload()

    const header = new MessageHeader(data.length, MessageType.VideoData)

    const message = header.toMessage(data)

    expect(message).toBeInstanceOf(VideoData)
  })

  test('toMessage returns VendorSessionInfo for vendor session payload messages', () => {
    const data = Buffer.from('abcd')
    const header = new MessageHeader(data.length, MessageType.VendorSessionInfo)

    const message = header.toMessage(data)

    expect(message).toBeInstanceOf(VendorSessionInfo)
  })

  test('toMessage returns null and warns for unknown type with binary payload', () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const header = new MessageHeader(4, 0xbeef as MessageType)
    const data = Buffer.from([0xde, 0xad, 0xbe, 0xef])

    const message = header.toMessage(data)

    expect(message).toBeNull()
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('Unknown type=0xbeef'))

    warnSpy.mockRestore()
  })

  test('toMessage also logs trimmed utf8 text for unknown text payloads', () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const header = new MessageHeader(6, 0xbeef as MessageType)
    const data = Buffer.from('hello\0\0', 'utf8')

    const message = header.toMessage(data)

    expect(message).toBeNull()
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('Unknown type=0xbeef'))
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('utf8="hello"'))

    warnSpy.mockRestore()
  })

  test('projection message tap receives payload metadata', () => {
    const tap = jest.fn()
    setProjectionyMessageTap(tap)

    const data = createVideoPayload()

    const header = new MessageHeader(data.length, MessageType.VideoData)

    header.toMessage(data)

    expect(tap).toHaveBeenCalledWith({
      type: MessageType.VideoData,
      length: data.length,
      dataLength: data.length,
      data
    })
  })

  test('projection message tap errors are swallowed', () => {
    const tap = jest.fn(() => {
      throw new Error('boom')
    })
    setProjectionyMessageTap(tap)

    const header = new MessageHeader(0, MessageType.Open)

    expect(() => header.toMessage()).not.toThrow()
    expect(tap).toHaveBeenCalledWith({
      type: MessageType.Open,
      length: 0,
      dataLength: 0,
      data: undefined
    })
  })

  test('toMessage does not log utf8 text for unknown payloads with empty trimmed text', () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const header = new MessageHeader(4, 0xbeef as MessageType)
    const data = Buffer.from('\0\0\0\0', 'utf8')

    const message = header.toMessage(data)

    expect(message).toBeNull()
    expect(warnSpy).toHaveBeenCalledTimes(1)
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('Unknown type=0xbeef'))

    warnSpy.mockRestore()
  })
})
