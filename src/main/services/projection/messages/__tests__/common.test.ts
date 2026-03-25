import { HeaderBuildError, MessageHeader, MessageType, setProjectionyMessageTap } from '../common'
import { DongleReady, Unplugged, VendorSessionInfo, VideoData } from '../readable'

const createVideoPayload = () => {
  const data = Buffer.alloc(20)
  data.writeUInt32LE(1920, 0)
  data.writeUInt32LE(1080, 4)
  data.writeUInt32LE(1, 8)
  data.writeUInt32LE(123, 12)
  data.writeUInt32LE(0, 16)
  return data
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
})
