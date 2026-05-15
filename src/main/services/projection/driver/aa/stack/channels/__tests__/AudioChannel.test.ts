import { AV_MSG, CH, FRAME_FLAGS } from '../../constants'
import type { RawFrame } from '../../frame/codec'
import { AudioChannel, type AudioChannelType } from '../AudioChannel'
import { decodeFields, decodeVarintValue, fieldVarint } from '../protoEnc'

function dummyFrame(channelId: number, msgId: number, payload: Buffer): RawFrame {
  return {
    channelId,
    flags: FRAME_FLAGS.ENC_SIGNAL,
    msgId,
    payload,
    rawPayload: Buffer.concat([Buffer.alloc(2), payload])
  }
}

function freshSend(): {
  send: jest.Mock
  calls: { channelId: number; msgId: number; data: Buffer }[]
} {
  const calls: { channelId: number; msgId: number; data: Buffer }[] = []
  const send = jest.fn((channelId: number, _flags: number, msgId: number, data: Buffer) => {
    calls.push({ channelId, msgId, data })
  })
  return { send, calls }
}

describe('AudioChannel.channelType', () => {
  test.each([
    [CH.MEDIA_AUDIO, 'media'],
    [CH.SPEECH_AUDIO, 'speech'],
    [CH.SYSTEM_AUDIO, 'phone']
  ])('channelId %s → %s', (id, type) => {
    const { send } = freshSend()
    const ch = new AudioChannel(id, send)
    expect(ch.channelType).toBe(type as AudioChannelType)
  })

  test('unknown channelId falls back to "media"', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(999, send)
    expect(ch.channelType).toBe('media')
  })
})

describe('AudioChannel.handleMessage', () => {
  test('AV_MEDIA_INDICATION emits pcm + sends an ack', () => {
    const { send, calls } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const pcm = jest.fn()
    ch.on('pcm', pcm)

    const payload = Buffer.from([0x11, 0x22, 0x33, 0x44])
    ch.handleMessage(
      AV_MSG.AV_MEDIA_INDICATION,
      payload,
      dummyFrame(CH.MEDIA_AUDIO, AV_MSG.AV_MEDIA_INDICATION, payload)
    )

    expect(pcm).toHaveBeenCalledTimes(1)
    const [emitted, , channelType] = pcm.mock.calls[0]
    expect(Buffer.isBuffer(emitted)).toBe(true)
    expect((emitted as Buffer).equals(payload)).toBe(true)
    expect(channelType).toBe('media')
    expect(calls).toHaveLength(1)
    expect(calls[0].msgId).toBe(AV_MSG.AV_MEDIA_ACK)
  })

  test('AV_MEDIA_WITH_TIMESTAMP separates the leading 8-byte timestamp from the data', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const pcm = jest.fn()
    ch.on('pcm', pcm)

    const tsBuf = Buffer.alloc(8)
    tsBuf.writeBigUInt64BE(1234567890n, 0)
    const data = Buffer.from([0xaa, 0xbb])
    ch.handleMessage(
      AV_MSG.AV_MEDIA_WITH_TIMESTAMP,
      Buffer.concat([tsBuf, data]),
      dummyFrame(CH.MEDIA_AUDIO, AV_MSG.AV_MEDIA_WITH_TIMESTAMP, Buffer.alloc(0))
    )

    expect(pcm).toHaveBeenCalledTimes(1)
    const [emittedData, emittedTs] = pcm.mock.calls[0]
    expect((emittedData as Buffer).equals(data)).toBe(true)
    expect(emittedTs).toBe(1234567890n)
  })

  test('AV_MEDIA_WITH_TIMESTAMP without enough bytes falls back to wall-clock', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const pcm = jest.fn()
    ch.on('pcm', pcm)

    const tooShort = Buffer.from([1, 2, 3])
    ch.handleMessage(
      AV_MSG.AV_MEDIA_WITH_TIMESTAMP,
      tooShort,
      dummyFrame(CH.MEDIA_AUDIO, AV_MSG.AV_MEDIA_WITH_TIMESTAMP, Buffer.alloc(0))
    )
    expect(pcm).toHaveBeenCalledTimes(1)
    expect((pcm.mock.calls[0][0] as Buffer).equals(tooShort)).toBe(true)
  })

  test('START_INDICATION decodes session_id and emits start', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const start = jest.fn()
    ch.on('start', start)

    // Start proto: field 1 = sessionId
    const startPayload = fieldVarint(1, 42)
    ch.handleMessage(
      AV_MSG.START_INDICATION,
      startPayload,
      dummyFrame(CH.MEDIA_AUDIO, AV_MSG.START_INDICATION, Buffer.alloc(0))
    )
    expect(start).toHaveBeenCalledWith('media', CH.MEDIA_AUDIO)
  })

  test('STOP_INDICATION emits stop', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const stop = jest.fn()
    ch.on('stop', stop)

    ch.handleMessage(AV_MSG.STOP_INDICATION, Buffer.alloc(0), dummyFrame(0, 0, Buffer.alloc(0)))
    expect(stop).toHaveBeenCalledWith('media', CH.MEDIA_AUDIO)
  })

  test('unhandled msgId is logged at debug but does not throw', () => {
    const debug = jest.spyOn(console, 'debug').mockImplementation(() => {})
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    expect(() =>
      ch.handleMessage(0x1234, Buffer.alloc(0), dummyFrame(0, 0, Buffer.alloc(0)))
    ).not.toThrow()
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })
})

describe('AudioChannel.handleSetupRequest', () => {
  test('emits setup with the negotiated codec + format', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const setup = jest.fn()
    ch.on('setup', setup)

    ch.handleSetupRequest(4, 44100, 2)
    expect(setup).toHaveBeenCalledWith(4, 44100, 2)
  })

  test('keeps previous sample rate / channels when called with 0s', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const setup = jest.fn()
    ch.on('setup', setup)

    ch.handleSetupRequest(4, 0, 0)
    expect(setup).toHaveBeenCalledWith(4, 48000, 2)
  })
})

describe('AudioChannel ack payload', () => {
  test('ack contains session_id and ack_count = 1', () => {
    const { send, calls } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    ch.handleMessage(AV_MSG.START_INDICATION, fieldVarint(1, 9), dummyFrame(0, 0, Buffer.alloc(0)))
    ch.handleMessage(
      AV_MSG.AV_MEDIA_INDICATION,
      Buffer.from([1]),
      dummyFrame(CH.MEDIA_AUDIO, AV_MSG.AV_MEDIA_INDICATION, Buffer.from([1]))
    )
    const ack = calls.find((c) => c.msgId === AV_MSG.AV_MEDIA_ACK)!
    const fields = Array.from(decodeFields(ack.data))
    expect(decodeVarintValue(fields[0].bytes)).toBe(9)
    expect(decodeVarintValue(fields[1].bytes)).toBe(1)
  })
})
