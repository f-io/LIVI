import { AV_MSG, CH, FRAME_FLAGS } from '../../constants'
import type { RawFrame } from '../../frame/codec'
import { decodeFields, decodeVarintValue, fieldVarint } from '../protoEnc'
import { VideoChannel } from '../VideoChannel'

function dummyFrame(channelId: number, msgId: number, payload: Buffer): RawFrame {
  return {
    channelId,
    flags: FRAME_FLAGS.ENC_SIGNAL,
    msgId,
    payload,
    rawPayload: Buffer.concat([Buffer.alloc(2), payload])
  }
}

function freshSend() {
  const calls: { channelId: number; msgId: number; data: Buffer }[] = []
  const send = jest.fn((channelId: number, _f: number, msgId: number, data: Buffer) => {
    calls.push({ channelId, msgId, data })
  })
  return { send, calls }
}

describe('VideoChannel', () => {
  test('defaults to CH.VIDEO when no channelId is passed', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    expect(ch.channelId).toBe(CH.VIDEO)
  })

  test('uses CH.CLUSTER_VIDEO label when constructed for cluster', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send, CH.CLUSTER_VIDEO)
    expect(ch.channelId).toBe(CH.CLUSTER_VIDEO)
  })

  test('AV_MEDIA_INDICATION emits frame + sends ack + increments frameCount', () => {
    const { send, calls } = freshSend()
    const ch = new VideoChannel(send)
    const frame = jest.fn()
    ch.on('frame', frame)

    ch.handleMessage(
      AV_MSG.AV_MEDIA_INDICATION,
      Buffer.from([0x00, 0x00, 0x00, 0x01]),
      dummyFrame(CH.VIDEO, AV_MSG.AV_MEDIA_INDICATION, Buffer.from([]))
    )
    expect(frame).toHaveBeenCalledTimes(1)
    expect(calls.find((c) => c.msgId === AV_MSG.AV_MEDIA_ACK)).toBeDefined()
    expect(ch.frameCount).toBe(1)
  })

  test('AV_MEDIA_WITH_TIMESTAMP separates the leading 8-byte timestamp', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    const frame = jest.fn()
    ch.on('frame', frame)

    const ts = Buffer.alloc(8)
    ts.writeBigUInt64BE(7n, 0)
    const data = Buffer.from([0xaa])
    ch.handleMessage(
      AV_MSG.AV_MEDIA_WITH_TIMESTAMP,
      Buffer.concat([ts, data]),
      dummyFrame(CH.VIDEO, AV_MSG.AV_MEDIA_WITH_TIMESTAMP, Buffer.alloc(0))
    )
    const [emittedData, emittedTs] = frame.mock.calls[0]
    expect((emittedData as Buffer).equals(data)).toBe(true)
    expect(emittedTs).toBe(7n)
  })

  test('START_INDICATION captures session_id for subsequent acks', () => {
    const { send, calls } = freshSend()
    const ch = new VideoChannel(send)
    ch.handleMessage(
      AV_MSG.START_INDICATION,
      fieldVarint(1, 11),
      dummyFrame(CH.VIDEO, AV_MSG.START_INDICATION, Buffer.alloc(0))
    )
    ch.handleMessage(
      AV_MSG.AV_MEDIA_INDICATION,
      Buffer.from([1]),
      dummyFrame(CH.VIDEO, AV_MSG.AV_MEDIA_INDICATION, Buffer.from([1]))
    )
    const ack = calls.find((c) => c.msgId === AV_MSG.AV_MEDIA_ACK)!
    const fields = Array.from(decodeFields(ack.data))
    expect(decodeVarintValue(fields[0].bytes)).toBe(11)
  })

  test('VIDEO_FOCUS_REQUEST mode=PROJECTED responds with focus indication + emits "video-focus-projected"', () => {
    const { send, calls } = freshSend()
    const ch = new VideoChannel(send)
    const projected = jest.fn()
    const host = jest.fn()
    ch.on('video-focus-projected', projected)
    ch.on('host-ui-requested', host)

    // field 2 (mode) varint = 1 (PROJECTED)
    const payload = fieldVarint(2, 1)
    ch.handleMessage(
      AV_MSG.VIDEO_FOCUS_REQUEST,
      payload,
      dummyFrame(CH.VIDEO, AV_MSG.VIDEO_FOCUS_REQUEST, Buffer.alloc(0))
    )

    expect(projected).toHaveBeenCalled()
    expect(host).not.toHaveBeenCalled()
    expect(calls.some((c) => c.msgId === AV_MSG.VIDEO_FOCUS_INDICATION)).toBe(true)
  })

  test('VIDEO_FOCUS_REQUEST mode=NATIVE emits "host-ui-requested"', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    const host = jest.fn()
    ch.on('host-ui-requested', host)

    ch.handleMessage(
      AV_MSG.VIDEO_FOCUS_REQUEST,
      fieldVarint(2, 2),
      dummyFrame(CH.VIDEO, AV_MSG.VIDEO_FOCUS_REQUEST, Buffer.alloc(0))
    )

    expect(host).toHaveBeenCalled()
  })

  test('STOP_INDICATION is logged, no further emits', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    expect(() =>
      ch.handleMessage(AV_MSG.STOP_INDICATION, Buffer.alloc(0), dummyFrame(0, 0, Buffer.alloc(0)))
    ).not.toThrow()
  })

  test('unhandled msgId is logged at debug', () => {
    const debug = jest.spyOn(console, 'debug').mockImplementation(() => {})
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    ch.handleMessage(0xdead, Buffer.alloc(0), dummyFrame(0, 0, Buffer.alloc(0)))
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })

  test('VIDEO_FOCUS_INDICATION is acknowledged at debug-level only', () => {
    const debug = jest.spyOn(console, 'debug').mockImplementation(() => {})
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    expect(() =>
      ch.handleMessage(
        AV_MSG.VIDEO_FOCUS_INDICATION,
        Buffer.alloc(0),
        dummyFrame(0, 0, Buffer.alloc(0))
      )
    ).not.toThrow()
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })
})
