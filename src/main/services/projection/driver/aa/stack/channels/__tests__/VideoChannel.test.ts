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
  const send = vi.fn(function (channelId: number, _f: number, msgId: number, data: Buffer) {
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

  test('START_INDICATION with a session id adopts it', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    ch.handleMessage(
      AV_MSG.START_INDICATION,
      fieldVarint(1, 7),
      dummyFrame(CH.VIDEO, AV_MSG.START_INDICATION, Buffer.alloc(0))
    )
    expect((ch as unknown as { _session: number })._session).toBe(7)
  })

  test('START_INDICATION with an empty payload keeps the default session', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    expect(() =>
      ch.handleMessage(
        AV_MSG.START_INDICATION,
        Buffer.alloc(0),
        dummyFrame(CH.VIDEO, AV_MSG.START_INDICATION, Buffer.alloc(0))
      )
    ).not.toThrow()
  })

  test('START_INDICATION without a session_id keeps the default session', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    expect(() =>
      ch.handleMessage(
        AV_MSG.START_INDICATION,
        fieldVarint(2, 3),
        dummyFrame(CH.VIDEO, AV_MSG.START_INDICATION, Buffer.alloc(0))
      )
    ).not.toThrow()
  })

  test('VIDEO_FOCUS_REQUEST mode=PROJECTED responds with focus indication + emits "video-focus-projected"', () => {
    const { send, calls } = freshSend()
    const ch = new VideoChannel(send)
    const projected = vi.fn()
    const host = vi.fn()
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
    const host = vi.fn()
    ch.on('host-ui-requested', host)

    ch.handleMessage(
      AV_MSG.VIDEO_FOCUS_REQUEST,
      fieldVarint(2, 2),
      dummyFrame(CH.VIDEO, AV_MSG.VIDEO_FOCUS_REQUEST, Buffer.alloc(0))
    )

    expect(host).toHaveBeenCalled()
  })

  test('VIDEO_FOCUS_REQUEST mode=NATIVE_TRANSIENT skips unknown fields and requests host UI', () => {
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    const host = vi.fn()
    ch.on('host-ui-requested', host)

    const payload = Buffer.concat([fieldVarint(1, 5), fieldVarint(2, 3)])
    ch.handleMessage(
      AV_MSG.VIDEO_FOCUS_REQUEST,
      payload,
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
    const debug = vi.spyOn(console, 'debug').mockImplementation(function () {})
    const { send } = freshSend()
    const ch = new VideoChannel(send)
    ch.handleMessage(0xdead, Buffer.alloc(0), dummyFrame(0, 0, Buffer.alloc(0)))
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })

  test('VIDEO_FOCUS_INDICATION is acknowledged at debug-level only', () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(function () {})
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
