import { AV_MSG, CH } from '../../constants'
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
  const send = vi.fn(function (channelId: number, flags: number, msgId: number, data: Buffer) {
    calls.push({ channelId, flags, msgId, data })
  })
  return { send, calls }
}

// MicrophoneRequest with open=true and a max_unacked the channel no longer reads
function openMic(channel: MicChannel): void {
  const openReq = Buffer.concat([fieldVarint(1, 1), fieldVarint(4, 2)])
  channel.handleMessage(AV_MSG.AV_INPUT_OPEN_REQUEST, openReq, dummyFrame())
}

describe('MicChannel — open/close', () => {
  test('OPEN_REQUEST(open=true) emits mic-start, sends OPEN_RESPONSE + START_INDICATION', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    const start = vi.fn()
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
    const stop = vi.fn()
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

    const stop = vi.fn()
    ch.on('mic-stop', stop)
    ch.handleMessage(AV_MSG.AV_INPUT_OPEN_REQUEST, fieldVarint(1, 0), dummyFrame())
    expect(stop).toHaveBeenCalledWith(MIC)
  })

  test('STOP_INDICATION while open emits mic-stop', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    openMic(ch)

    const stop = vi.fn()
    ch.on('mic-stop', stop)
    ch.handleMessage(AV_MSG.STOP_INDICATION, Buffer.alloc(0), dummyFrame())
    expect(stop).toHaveBeenCalledWith(MIC)
  })

  test('STOP_INDICATION while not open is a no-op', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    const stop = vi.fn()
    ch.on('mic-stop', stop)
    ch.handleMessage(AV_MSG.STOP_INDICATION, Buffer.alloc(0), dummyFrame())
    expect(stop).not.toHaveBeenCalled()
  })
})

describe('MicChannel: control side only', () => {
  test('AV_MEDIA_ACK is ignored before and after open', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    const stop = vi.fn()
    ch.on('mic-stop', stop)
    ch.handleMessage(AV_MSG.AV_MEDIA_ACK, Buffer.alloc(0), dummyFrame())
    expect(calls).toHaveLength(0)

    openMic(ch)
    calls.length = 0
    ch.handleMessage(AV_MSG.AV_MEDIA_ACK, Buffer.alloc(0), dummyFrame())
    expect(calls).toHaveLength(0)
    expect(stop).not.toHaveBeenCalled()
  })

  test('OPEN_REQUEST ignores fields other than open', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    const start = vi.fn()
    ch.on('mic-start', start)
    const req = Buffer.concat([fieldVarint(1, 1), fieldVarint(2, 9), fieldVarint(4, 3)])
    ch.handleMessage(AV_MSG.AV_INPUT_OPEN_REQUEST, req, dummyFrame())
    expect(start).toHaveBeenCalledWith(MIC)
    expect(calls.some((c) => c.msgId === AV_MSG.AV_INPUT_OPEN_RESPONSE)).toBe(true)
  })
})

describe('MicChannel.handleSetupRequest', () => {
  test('captures sampleRate/channels when non-zero', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    expect(() => ch.handleSetupRequest(5, 44100, 1)).not.toThrow()
  })

  test('keeps defaults when sampleRate/channels are zero', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    expect(() => ch.handleSetupRequest(5, 0, 0)).not.toThrow()
  })
})

describe('MicChannel — setup request message', () => {
  test('SETUP_REQUEST is accepted without side effects', () => {
    const { send, calls } = freshSend()
    const ch = new MicChannel(MIC, send)
    ch.handleMessage(AV_MSG.SETUP_REQUEST, Buffer.alloc(0), dummyFrame())
    expect(calls).toHaveLength(0)
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
    const debug = vi.spyOn(console, 'debug').mockImplementation(function () {})
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    ch.handleMessage(0xbeef, Buffer.alloc(0), dummyFrame())
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })
})

describe('MicChannel — negotiated format', () => {
  test('format exposes 16 kHz mono until a setup arrives', () => {
    const { send } = freshSend()
    const ch = new MicChannel(MIC, send)
    expect(ch.format).toEqual({ sampleRate: 16000, channels: 1 })
  })
})
