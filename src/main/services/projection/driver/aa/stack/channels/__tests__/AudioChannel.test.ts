import type { Mock } from 'vitest'
import { AV_MSG, CH, FRAME_FLAGS } from '../../constants'
import type { RawFrame } from '../../frame/codec'
import { AudioChannel, type AudioChannelType } from '../AudioChannel'
import { fieldVarint } from '../protoEnc'

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
  send: Mock
  calls: { channelId: number; msgId: number; data: Buffer }[]
} {
  const calls: { channelId: number; msgId: number; data: Buffer }[] = []
  const send = vi.fn(function (channelId: number, _flags: number, msgId: number, data: Buffer) {
    calls.push({ channelId, msgId, data })
  })
  return { send, calls }
}

describe('AudioChannel.channelType', () => {
  test.each([
    [CH.MEDIA_AUDIO, 'media'],
    [CH.SPEECH_AUDIO, 'speech'],
    [CH.SYSTEM_AUDIO, 'system']
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
  test('START_INDICATION decodes session_id and emits start', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const start = vi.fn()
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

  test('START_INDICATION without a session_id keeps the default session', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const start = vi.fn()
    ch.on('start', start)
    ch.handleMessage(
      AV_MSG.START_INDICATION,
      fieldVarint(2, 5),
      dummyFrame(CH.MEDIA_AUDIO, AV_MSG.START_INDICATION, Buffer.alloc(0))
    )
    expect(start).toHaveBeenCalledWith('media', CH.MEDIA_AUDIO)
  })

  test('STOP_INDICATION emits stop', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const stop = vi.fn()
    ch.on('stop', stop)

    ch.handleMessage(AV_MSG.STOP_INDICATION, Buffer.alloc(0), dummyFrame(0, 0, Buffer.alloc(0)))
    expect(stop).toHaveBeenCalledWith('media', CH.MEDIA_AUDIO)
  })

  test('unhandled msgId is logged at debug but does not throw', () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(function () {})
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
    const setup = vi.fn()
    ch.on('setup', setup)

    ch.handleSetupRequest(4, 44100, 2)
    expect(setup).toHaveBeenCalledWith(4, 44100, 2)
  })

  test('keeps previous sample rate / channels when called with 0s', () => {
    const { send } = freshSend()
    const ch = new AudioChannel(CH.MEDIA_AUDIO, send)
    const setup = vi.fn()
    ch.on('setup', setup)

    ch.handleSetupRequest(4, 0, 0)
    expect(setup).toHaveBeenCalledWith(4, 48000, 2)
  })
})
