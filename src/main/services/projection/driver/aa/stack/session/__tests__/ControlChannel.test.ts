import { CH, CTRL_MSG, FRAME_FLAGS, STATUS_OK } from '../../constants'
import type { ProtoTypes } from '../../proto/index'
import { ControlChannel } from '../ControlChannel'

type ProtoType = {
  name: string
  verify: jest.Mock
  create: jest.Mock
  encode: jest.Mock
  decode: jest.Mock
  toObject: jest.Mock
}

function fakeProtoType(name: string, decoded: Record<string, unknown> = {}): ProtoType {
  return {
    name,
    verify: jest.fn(() => null),
    create: jest.fn((fields) => fields),
    encode: jest.fn(() => ({ finish: () => new Uint8Array([0xab, 0xcd]) })),
    decode: jest.fn(() => ({ ...decoded })),
    toObject: jest.fn((m) => m as Record<string, unknown>)
  }
}

function fakeProto(): ProtoTypes {
  return {
    ServiceDiscoveryRequest: fakeProtoType('ServiceDiscoveryRequest', {
      deviceName: 'phone',
      labelText: 'AA',
      phoneInfo: { make: 'Acme' }
    }),
    PingRequest: fakeProtoType('PingRequest', { timestamp: 1234 }),
    PingResponse: fakeProtoType('PingResponse'),
    ChannelOpenRequest: fakeProtoType('ChannelOpenRequest', { serviceId: 9 }),
    ChannelOpenResponse: fakeProtoType('ChannelOpenResponse', { status: STATUS_OK }),
    BindingRequest: fakeProtoType('BindingRequest', { scan_codes: [21] }),
    BindingResponse: fakeProtoType('BindingResponse')
  } as unknown as ProtoTypes
}

type Call = { channelId: number; flags: number; msgId: number; data: Buffer }
function makeSend(): { send: jest.Mock; calls: Call[] } {
  const calls: Call[] = []
  const send = jest.fn((channelId: number, flags: number, msgId: number, data: Buffer) => {
    calls.push({ channelId, flags, msgId, data })
  })
  return { send, calls }
}

describe('ControlChannel — ServiceDiscoveryRequest', () => {
  test('emits the decoded request', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('service-discovery-request', cb)

    ch.handleMessage(CTRL_MSG.SERVICE_DISCOVERY_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledTimes(1)
    const req = cb.mock.calls[0][0]
    expect(req.deviceName).toBe('phone')
  })

  test('proto parse failure → emits an empty object', () => {
    const proto = fakeProto()
    ;(proto.ServiceDiscoveryRequest as unknown as ProtoType).decode.mockImplementationOnce(() => {
      throw new Error('bad')
    })
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('service-discovery-request', cb)
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})

    ch.handleMessage(CTRL_MSG.SERVICE_DISCOVERY_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith({})
    warn.mockRestore()
  })
})

describe('ControlChannel — ping', () => {
  test('PING_REQUEST replies with PING_RESPONSE (plaintext) and emits ping', () => {
    const proto = fakeProto()
    const { send, calls } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('ping', cb)

    ch.handleMessage(CTRL_MSG.PING_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(1234)

    const resp = calls.find((c) => c.msgId === CTRL_MSG.PING_RESPONSE)!
    expect(resp.channelId).toBe(CH.CONTROL)
    expect(resp.flags).toBe(FRAME_FLAGS.PLAINTEXT)
  })

  test('PING_RESPONSE emits pong', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('pong', cb)
    ch.handleMessage(CTRL_MSG.PING_RESPONSE, Buffer.alloc(0))
    expect(cb).toHaveBeenCalled()
  })
})

describe('ControlChannel — channel open', () => {
  test('CHANNEL_OPEN_REQUEST emits channel-open-request with serviceId', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('channel-open-request', cb)
    ch.handleMessage(CTRL_MSG.CHANNEL_OPEN_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(9)
  })

  test('CHANNEL_OPEN_REQUEST falls back to channelId when serviceId is missing', () => {
    const proto = fakeProto()
    ;(proto.ChannelOpenRequest as unknown as ProtoType).decode.mockReturnValueOnce({
      channelId: 4
    })
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('channel-open-request', cb)
    ch.handleMessage(CTRL_MSG.CHANNEL_OPEN_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(4)
  })

  test('CHANNEL_OPEN_REQUEST falls back to 0 when both fields are missing', () => {
    const proto = fakeProto()
    ;(proto.ChannelOpenRequest as unknown as ProtoType).decode.mockReturnValueOnce({})
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('channel-open-request', cb)
    ch.handleMessage(CTRL_MSG.CHANNEL_OPEN_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(0)
  })

  test('CHANNEL_OPEN_REQUEST decode failure is silenced', () => {
    const proto = fakeProto()
    ;(proto.ChannelOpenRequest as unknown as ProtoType).decode.mockImplementationOnce(() => {
      throw new Error('bad')
    })
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('channel-open-request', cb)
    ch.handleMessage(CTRL_MSG.CHANNEL_OPEN_REQUEST, Buffer.alloc(0))
    expect(cb).not.toHaveBeenCalled()
  })

  test('CHANNEL_OPEN_RESPONSE decode failure is silenced', () => {
    const proto = fakeProto()
    ;(proto.ChannelOpenResponse as unknown as ProtoType).decode.mockImplementationOnce(() => {
      throw new Error('bad')
    })
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    expect(() => ch.handleMessage(CTRL_MSG.CHANNEL_OPEN_RESPONSE, Buffer.alloc(0))).not.toThrow()
  })

  test('Ping ts defaults to 0 when proto omits the field', () => {
    const proto = fakeProto()
    ;(proto.PingRequest as unknown as ProtoType).decode.mockReturnValueOnce({})
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('ping', cb)
    ch.handleMessage(CTRL_MSG.PING_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(0)
  })

  test('CHANNEL_OPEN_RESPONSE with non-OK status logs a warning', () => {
    const proto = fakeProto()
    ;(proto.ChannelOpenResponse as unknown as ProtoType).decode.mockReturnValueOnce({
      status: 999
    })
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    ch.handleMessage(CTRL_MSG.CHANNEL_OPEN_RESPONSE, Buffer.alloc(0))
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('sendChannelOpenResponse encodes status + sends on CH.CONTROL with ENC_CONTROL flags', () => {
    const proto = fakeProto()
    const { send, calls } = makeSend()
    const ch = new ControlChannel(proto, send)
    ch.sendChannelOpenResponse(3, STATUS_OK)
    expect(calls).toHaveLength(1)
    expect(calls[0].channelId).toBe(CH.CONTROL)
    expect(calls[0].flags).toBe(FRAME_FLAGS.ENC_CONTROL)
    expect(calls[0].msgId).toBe(CTRL_MSG.CHANNEL_OPEN_RESPONSE)
  })
})

describe('ControlChannel — audio focus', () => {
  function buildFocusReq(type: number): Buffer {
    return Buffer.from([0x08, type])
  }

  test.each([
    [1, 1], // GAIN → STATE_GAIN
    [2, 2], // GAIN_TRANSIENT → STATE_GAIN_TRANSIENT
    [3, 2], // GAIN_TRANSIENT_MAY_DUCK → STATE_GAIN_TRANSIENT
    [4, 3], // RELEASE → STATE_LOSS
    [0, 3] // unknown → STATE_LOSS
  ])('type=%s → state=%s', (focusType, expectedState) => {
    const proto = fakeProto()
    const { send, calls } = makeSend()
    const ch = new ControlChannel(proto, send)
    ch.handleMessage(CTRL_MSG.AUDIO_FOCUS_REQUEST, buildFocusReq(focusType))
    const resp = calls.find((c) => c.msgId === CTRL_MSG.AUDIO_FOCUS_RESPONSE)!
    expect(resp.data[1]).toBe(expectedState)
  })

  test('emits audio-focus-request', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('audio-focus-request', cb)
    ch.handleMessage(CTRL_MSG.AUDIO_FOCUS_REQUEST, buildFocusReq(1))
    expect(cb).toHaveBeenCalled()
  })
})

describe('ControlChannel — navigation focus', () => {
  test('NAVIGATION_FOCUS_REQUEST echoes the payload back as the response', () => {
    const proto = fakeProto()
    const { send, calls } = makeSend()
    const ch = new ControlChannel(proto, send)
    const payload = Buffer.from([0x08, 0x01])
    ch.handleMessage(CTRL_MSG.NAVIGATION_FOCUS_REQUEST, payload)
    const resp = calls.find((c) => c.msgId === CTRL_MSG.NAVIGATION_FOCUS_RESPONSE)!
    expect(resp.data.equals(payload)).toBe(true)
  })
})

describe('ControlChannel — voice session', () => {
  test('status=1 (START) → emits voice-session(true)', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('voice-session', cb)
    ch.handleMessage(CTRL_MSG.VOICE_SESSION_NOTIFICATION, Buffer.from([0x08, 0x01]))
    expect(cb).toHaveBeenCalledWith(true)
  })

  test('status=2 (END) → emits voice-session(false)', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('voice-session', cb)
    ch.handleMessage(CTRL_MSG.VOICE_SESSION_NOTIFICATION, Buffer.from([0x08, 0x02]))
    expect(cb).toHaveBeenCalledWith(false)
  })
})

describe('ControlChannel — shutdown', () => {
  test('SHUTDOWN_REQUEST with a payload emits shutdown(reason)', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('shutdown', cb)
    const payload = Buffer.alloc(4)
    payload.writeUInt32BE(7, 0)
    ch.handleMessage(CTRL_MSG.SHUTDOWN_REQUEST, payload)
    expect(cb).toHaveBeenCalledWith(7)
  })
})

describe('ControlChannel — binding', () => {
  test('BINDING_REQUEST is acknowledged with BINDING_RESPONSE(status=OK)', () => {
    const proto = fakeProto()
    const { send, calls } = makeSend()
    const ch = new ControlChannel(proto, send)
    ch.handleMessage(CTRL_MSG.BINDING_REQUEST, Buffer.alloc(0))
    expect(calls.some((c) => c.msgId === CTRL_MSG.BINDING_RESPONSE)).toBe(true)
  })
})

describe('ControlChannel — handleAVSetupRequest passthrough', () => {
  test('emits av-setup-request', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('av-setup-request', cb)
    ch.handleAVSetupRequest(3, Buffer.from([0x01]))
    expect(cb).toHaveBeenCalledWith(3, Buffer.from([0x01]))
  })
})

describe('ControlChannel — unhandled and defensive paths', () => {
  test('unknown msgId is logged at debug', () => {
    const debug = jest.spyOn(console, 'debug').mockImplementation(() => {})
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    ch.handleMessage(0xbeef, Buffer.alloc(0))
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })

  test('AV SETUP_REQUEST on the control channel is ignored at debug-level', () => {
    const debug = jest.spyOn(console, 'debug').mockImplementation(() => {})
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    ch.handleMessage(0x8000 /* AV_MSG.SETUP_REQUEST */, Buffer.alloc(0))
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })

  test('SHUTDOWN_REQUEST with short payload defaults reason to 0', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('shutdown', cb)
    ch.handleMessage(CTRL_MSG.SHUTDOWN_REQUEST, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(0)
  })

  test('SHUTDOWN_REQUEST with 2 or 3 bytes (too short for uint32) falls into the catch', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('shutdown', cb)
    // 3 bytes — readUInt32BE will throw
    ch.handleMessage(CTRL_MSG.SHUTDOWN_REQUEST, Buffer.from([1, 2, 3]))
    expect(cb).toHaveBeenCalledWith(0)
  })

  test('Ping parse error is logged but does not throw', () => {
    const proto = fakeProto()
    ;(proto.PingRequest as unknown as { decode: jest.Mock }).decode.mockImplementationOnce(() => {
      throw new Error('bad varint')
    })
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    expect(() => ch.handleMessage(CTRL_MSG.PING_REQUEST, Buffer.alloc(0))).not.toThrow()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('NavigationFocusRequest send failure is swallowed', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    send.mockImplementation(() => {
      throw new Error('write failed')
    })
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const ch = new ControlChannel(proto, send)
    expect(() =>
      ch.handleMessage(CTRL_MSG.NAVIGATION_FOCUS_REQUEST, Buffer.from([0x08, 0x01]))
    ).not.toThrow()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('BindingRequest decode failure is logged but does not throw', () => {
    const proto = fakeProto()
    ;(proto.BindingRequest as unknown as { decode: jest.Mock }).decode.mockImplementationOnce(
      () => {
        throw new Error('proto err')
      }
    )
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const { send } = makeSend()
    const ch = new ControlChannel(proto, send)
    expect(() => ch.handleMessage(CTRL_MSG.BINDING_REQUEST, Buffer.alloc(0))).not.toThrow()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('AudioFocus send failure is logged but does not stop the emit', () => {
    const proto = fakeProto()
    const { send } = makeSend()
    send.mockImplementation(() => {
      throw new Error('not ready')
    })
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const ch = new ControlChannel(proto, send)
    const cb = jest.fn()
    ch.on('audio-focus-request', cb)
    ch.handleMessage(CTRL_MSG.AUDIO_FOCUS_REQUEST, Buffer.from([0x08, 0x01]))
    expect(cb).toHaveBeenCalled()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })
})
