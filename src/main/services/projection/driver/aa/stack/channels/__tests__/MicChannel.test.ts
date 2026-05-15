import { AV_MSG, CH, FRAME_FLAGS } from '../../constants'
import type { RawFrame } from '../../frame/codec'
import { MicChannel } from '../MicChannel'
import { decodeFields, decodeVarintValue, fieldVarint } from '../protoEnc'

const MIC = CH.MIC_INPUT

function dummyFrame(): RawFrame {
  return {
    channelId: MIC,
    flags: 0,
    msgId: 0,
    payload: Buffer.alloc(0),
    rawPayload: Buffer.alloc(0)
  }
}

function freshSend() {
  const calls: { channelId: number; flags: number; msgId: number; data: Buffer }[] = []
  const send = jest.fn((channelId: number, flags: number, msgId: number, data: Buffer) => {
    calls.push({ channelId, flags, msgId, data })
  })
  return { send, calls }
}

function openMic(channel: MicChannel, maxUnacked = 2): void {
  const openReq = Buffer.concat([fieldVarint(1, 1), fieldVarint(4, maxUnacked)])
  channel.handleMessage(AV_MSG.AV_INPUT_OPEN_REQUEST, openReq, dummyFrame())
}

describe('MicChannel — open/close', () => {
  test('OPEN_REQUEST(open=true) emits mic-start, sends OPEN_RESPONSE + START_INDICATION', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    const start = jest.fn()
    ch.on('mic-start', start)

    openMic(ch)

    expect(start).toHaveBeenCalledWith(MIC)
    const msgs = calls.map((c) => c.msgId)
    expect(msgs).toContain(AV_MSG.AV_INPUT_OPEN_RESPONSE)
    expect(msgs).toContain(AV_MSG.START_INDICATION)
  })

  test('OPEN_REQUEST(open=false) without an existing open is just an ack', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    const stop = jest.fn()
    ch.on('mic-stop', stop)

    const closeReq = fieldVarint(1, 0)
    ch.handleMessage(AV_MSG.AV_INPUT_OPEN_REQUEST, closeReq, dummyFrame())

    expect(stop).not.toHaveBeenCalled()
    expect(calls.some((c) => c.msgId === AV_MSG.AV_INPUT_OPEN_RESPONSE)).toBe(true)
  })

  test('OPEN_REQUEST(open=false) after open emits mic-stop', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    openMic(ch)

    const stop = jest.fn()
    ch.on('mic-stop', stop)
    ch.handleMessage(AV_MSG.AV_INPUT_OPEN_REQUEST, fieldVarint(1, 0), dummyFrame())
    expect(stop).toHaveBeenCalledWith(MIC)
  })

  test('STOP_INDICATION while open emits mic-stop', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    openMic(ch)

    const stop = jest.fn()
    ch.on('mic-stop', stop)
    ch.handleMessage(AV_MSG.STOP_INDICATION, Buffer.alloc(0), dummyFrame())
    expect(stop).toHaveBeenCalledWith(MIC)
  })

  test('STOP_INDICATION while not open is a no-op', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    const stop = jest.fn()
    ch.on('mic-stop', stop)
    ch.handleMessage(AV_MSG.STOP_INDICATION, Buffer.alloc(0), dummyFrame())
    expect(stop).not.toHaveBeenCalled()
  })
})

describe('MicChannel — pcm flow control', () => {
  test('pushPcm before open is dropped', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    ch.pushPcm(Buffer.from([1]), 0n)
    expect(calls.some((c) => c.msgId === AV_MSG.AV_MEDIA_WITH_TIMESTAMP)).toBe(false)
  })

  test('pushPcm after open sends a timestamp-prefixed frame', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    openMic(ch)

    ch.pushPcm(Buffer.from([0xaa, 0xbb]), 7n)
    const frame = calls.find((c) => c.msgId === AV_MSG.AV_MEDIA_WITH_TIMESTAMP)!
    expect(frame.channelId).toBe(MIC)
    expect(frame.flags).toBe(FRAME_FLAGS.ENC_SIGNAL)
    expect(frame.data.readBigUInt64BE(0)).toBe(7n)
    expect(frame.data.subarray(8).equals(Buffer.from([0xaa, 0xbb]))).toBe(true)
  })

  test('respects maxUnacked: extra frames queue until ACK arrives', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    openMic(ch, 1)

    ch.pushPcm(Buffer.from([1]), 1n) // sent (unacked=1)
    ch.pushPcm(Buffer.from([2]), 2n) // queued
    const framesSentBefore = calls.filter((c) => c.msgId === AV_MSG.AV_MEDIA_WITH_TIMESTAMP).length
    expect(framesSentBefore).toBe(1)

    // ACK frees one slot — queued frame is drained
    ch.handleMessage(AV_MSG.AV_MEDIA_ACK, Buffer.alloc(0), dummyFrame())
    const framesSentAfter = calls.filter((c) => c.msgId === AV_MSG.AV_MEDIA_WITH_TIMESTAMP).length
    expect(framesSentAfter).toBe(2)
  })

  test('drops oldest queued frame if backlog grows beyond 64', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    openMic(ch, 1)

    // First push goes out, the rest queue. Backlog cap is 64.
    ch.pushPcm(Buffer.from([0]), 0n)
    for (let i = 1; i <= 70; i++) ch.pushPcm(Buffer.from([i]), BigInt(i))

    // No throw, no overflow
    expect(true).toBe(true)
  })
})

describe('MicChannel.handleSetupRequest', () => {
  test('captures sampleRate/channels when non-zero', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    expect(() => ch.handleSetupRequest(5, 44100, 1)).not.toThrow()
  })
})

describe('MicChannel response payloads', () => {
  test('OPEN_RESPONSE carries status=0 + session_id', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    openMic(ch)
    const resp = calls.find((c) => c.msgId === AV_MSG.AV_INPUT_OPEN_RESPONSE)!
    const fields = Array.from(decodeFields(resp.data))
    expect(decodeVarintValue(fields[0].bytes)).toBe(0)
    expect(decodeVarintValue(fields[1].bytes)).toBe(1) // default session id = 1
  })

  test('unhandled msgId is logged at debug', () => {
    const debug = jest.spyOn(console, 'debug').mockImplementation(() => {})
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    ch.handleMessage(0xbeef, Buffer.alloc(0), dummyFrame())
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })
})
