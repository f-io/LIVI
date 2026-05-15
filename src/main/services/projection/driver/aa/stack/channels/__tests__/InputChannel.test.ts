import { CH, FRAME_FLAGS } from '../../constants'
import { BUTTON_KEY, INPUT_MSG, InputChannel, TOUCH_ACTION } from '../InputChannel'
import { decodeFields, decodeVarintValue } from '../protoEnc'

function freshSend() {
  const calls: { channelId: number; flags: number; msgId: number; data: Buffer }[] = []
  const send = jest.fn((channelId: number, flags: number, msgId: number, data: Buffer) => {
    calls.push({ channelId, flags, msgId, data })
  })
  return { send, calls }
}

// Walk the InputReport proto and pluck a single optional sub-message.
function subField(payload: Buffer, fieldNumber: number): Buffer | undefined {
  for (const f of decodeFields(payload)) {
    if (f.field === fieldNumber) return f.bytes
  }
  return undefined
}

describe('InputChannel.sendTouch', () => {
  test('emits an INPUT_REPORT on CH.INPUT with ENC_SIGNAL flags', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendTouch(TOUCH_ACTION.DOWN, [{ x: 100, y: 200, id: 0 }])
    expect(calls).toHaveLength(1)
    expect(calls[0].channelId).toBe(CH.INPUT)
    expect(calls[0].flags).toBe(FRAME_FLAGS.ENC_SIGNAL)
    expect(calls[0].msgId).toBe(INPUT_MSG.INPUT_REPORT)
  })

  test('does nothing for an empty pointer array', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendTouch(TOUCH_ACTION.MOVED, [])
    expect(calls).toHaveLength(0)
  })

  test('packs touch_event under InputReport field 3', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendTouch(TOUCH_ACTION.DOWN, [{ x: 100, y: 200, id: 0 }])
    expect(subField(calls[0].data, 3)).toBeDefined()
  })

  test('emits one pointer sub-message per finger', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendTouch(TOUCH_ACTION.POINTER_DOWN, [
      { x: 10, y: 20, id: 0 },
      { x: 30, y: 40, id: 1 }
    ])
    const touchEvent = subField(calls[0].data, 3)!
    const pointers = Array.from(decodeFields(touchEvent)).filter((f) => f.field === 1)
    expect(pointers).toHaveLength(2)

    // Each pointer carries x/y/id varints
    const p0Fields = Array.from(decodeFields(pointers[0].bytes))
    expect(decodeVarintValue(p0Fields[0].bytes)).toBe(10)
    expect(decodeVarintValue(p0Fields[1].bytes)).toBe(20)
    expect(decodeVarintValue(p0Fields[2].bytes)).toBe(0)
  })

  test('encodes action under TouchEvent field 3', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendTouch(TOUCH_ACTION.MOVED, [{ x: 1, y: 1, id: 0 }])
    const touchEvent = subField(calls[0].data, 3)!
    const actionField = Array.from(decodeFields(touchEvent)).find((f) => f.field === 3)!
    expect(decodeVarintValue(actionField.bytes)).toBe(TOUCH_ACTION.MOVED)
  })

  test('honours actionIndex', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendTouch(
      TOUCH_ACTION.POINTER_DOWN,
      [
        { x: 1, y: 1, id: 0 },
        { x: 2, y: 2, id: 1 }
      ],
      1
    )
    const touchEvent = subField(calls[0].data, 3)!
    const indexField = Array.from(decodeFields(touchEvent)).find((f) => f.field === 2)!
    expect(decodeVarintValue(indexField.bytes)).toBe(1)
  })
})

describe('InputChannel.sendButton', () => {
  test('emits a key_event under field 4', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendButton(BUTTON_KEY.HOME, true)
    expect(subField(calls[0].data, 4)).toBeDefined()
  })

  test('encodes keycode, down, metastate, and longpress', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendButton(BUTTON_KEY.HOME, true, true)
    const keyEvent = subField(calls[0].data, 4)!
    // field 1 = repeated Key — first one
    const keyEntry = Array.from(decodeFields(keyEvent)).find((f) => f.field === 1)!
    const keyFields = Array.from(decodeFields(keyEntry.bytes))
    expect(decodeVarintValue(keyFields[0].bytes)).toBe(BUTTON_KEY.HOME)
    expect(decodeVarintValue(keyFields[1].bytes)).toBe(1) // down=true
    expect(decodeVarintValue(keyFields[2].bytes)).toBe(0) // metastate=0
    expect(decodeVarintValue(keyFields[3].bytes)).toBe(1) // longpress=true
  })

  test('accepts an array of keycodes and packs all as repeated Key entries', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendButton([BUTTON_KEY.DPAD_LEFT, BUTTON_KEY.NAVIGATE_PREVIOUS], true)
    const keyEvent = subField(calls[0].data, 4)!
    const keys = Array.from(decodeFields(keyEvent)).filter((f) => f.field === 1)
    expect(keys).toHaveLength(2)
  })

  test('does nothing for an empty array', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendButton([], true)
    expect(calls).toHaveLength(0)
  })
})

describe('InputChannel.sendRotary', () => {
  test('emits a relative_event under field 6', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendRotary(1)
    expect(subField(calls[0].data, 6)).toBeDefined()
  })

  test('packs keycode=ROTARY_CONTROLLER + delta', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendRotary(1)
    const relEvent = subField(calls[0].data, 6)!
    const relData = Array.from(decodeFields(relEvent)).find((f) => f.field === 1)!
    const fields = Array.from(decodeFields(relData.bytes))
    expect(decodeVarintValue(fields[0].bytes)).toBe(BUTTON_KEY.ROTARY_CONTROLLER)
    expect(decodeVarintValue(fields[1].bytes)).toBe(1)
  })

  test('negative delta encodes as a multi-byte varint (signed two’s complement)', () => {
    const { send, calls } = freshSend()
    const ch = new InputChannel(send)
    ch.sendRotary(-1)
    const relEvent = subField(calls[0].data, 6)!
    const relData = Array.from(decodeFields(relEvent)).find((f) => f.field === 1)!
    const fields = Array.from(decodeFields(relData.bytes))
    // 10-byte varint for -1 as uint64
    expect(fields[1].bytes.length).toBe(10)
  })
})

describe('INPUT_MSG / TOUCH_ACTION / BUTTON_KEY constants', () => {
  test('INPUT_REPORT id matches the wire protocol', () => {
    expect(INPUT_MSG.INPUT_REPORT).toBe(0x8001)
  })
  test('TOUCH_ACTION enum-like values', () => {
    expect(TOUCH_ACTION.DOWN).toBe(0)
    expect(TOUCH_ACTION.UP).toBe(1)
    expect(TOUCH_ACTION.MOVED).toBe(2)
  })
  test('BUTTON_KEY contains rotary controller code', () => {
    expect(BUTTON_KEY.ROTARY_CONTROLLER).toBe(65536)
  })
})
