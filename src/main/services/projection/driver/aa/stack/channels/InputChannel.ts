/**
 * Input channel handler (CH.INPUT = 8). HU → Phone direction.
 *
 * Wire protocol (aasdk InputMessageId):
 *   INPUT_MESSAGE_INPUT_REPORT = 32769 (0x8001) — touch / key / etc. events
 *
 * Proto schema (aap_protobuf.service.inputsource.message):
 *
 *   message InputReport {
 *     required uint64 timestamp        = 1;   // microseconds
 *     optional TouchEvent touch_event  = 3;
 *     optional KeyEvent   key_event    = 4;
 *     // (absolute_event=5, relative_event=6, touchpad_event=7 — unused here)
 *   }
 *
 *   message TouchEvent {
 *     repeated Pointer    pointer_data = 1;
 *     message Pointer {
 *       required uint32 x          = 1;
 *       required uint32 y          = 2;
 *       required uint32 pointer_id = 3;        // NO pressure field
 *     }
 *     optional uint32        action_index = 2;
 *     optional PointerAction action       = 3;
 *   }
 *
 *   enum PointerAction { ACTION_DOWN=0; ACTION_UP=1; ACTION_MOVED=2;
 *                        ACTION_POINTER_DOWN=5; ACTION_POINTER_UP=6; }
 *
 *   message KeyEvent {
 *     repeated Key keys = 1;
 *     message Key {
 *       required uint32 keycode   = 1;
 *       required bool   down      = 2;
 *       required uint32 metastate = 3;          // required — must be set
 *       optional bool   longpress = 4;
 *     }
 *   }
 *
 * Reference encoder: openauto InputSourceService.cpp::onTouchEvent /
 * onButtonEvent. Timestamps are MICROSECONDS as varint.
 */

import { EventEmitter } from 'node:events'
import { CH, FRAME_FLAGS } from '../constants.js'
import { fieldLenDelim, fieldVarint } from './protoEnc.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export const INPUT_MSG = {
  INPUT_REPORT: 0x8001, // = 32769 (INPUT_MESSAGE_INPUT_REPORT)
  KEY_BINDING_REQUEST: 0x8002, // = 32770 (INPUT_MESSAGE_KEY_BINDING_REQUEST, phone → HU)
  KEY_BINDING_RESPONSE: 0x8003, // = 32771 (INPUT_MESSAGE_KEY_BINDING_RESPONSE, HU → phone)
  INPUT_FEEDBACK: 0x8004 // = 32772 (INPUT_MESSAGE_INPUT_FEEDBACK)
} as const

/** PointerAction enum values per aasdk PointerAction.proto. */
export const TOUCH_ACTION = {
  DOWN: 0, // ACTION_DOWN
  UP: 1, // ACTION_UP
  MOVED: 2, // ACTION_MOVED
  POINTER_DOWN: 5, // ACTION_POINTER_DOWN
  POINTER_UP: 6 // ACTION_POINTER_UP
} as const

// Android KeyEvent.KEYCODE_* values + AA-specific extensions, mirroring
// `aap_protobuf/oaa/av/AndroidKeycodeEnum.proto`. The LIVI key-mapper layer
// resolves user-bound LIVI commands (CommandMapping) to one of these.
//
// IMPORTANT: a key only reaches the phone if its code is in the
// `keycodesSupported` list we advertise in the SDR InputSourceService. Keep
// the SDR list in Session._buildServiceDiscoveryResponse aligned with this
// table — anything missing from the SDR is silently dropped phone-side.
//
// Volume / mute are advertised even though they don't make sense in the
// default config (phone is audio source, HU is the sink). When the user
// enables `audioTransferMode` in settings the phone takes over playback via
// A2DP / analog out, in which case the user might bind hardware volume
// keys to control the phone's own stream gain — just like the dongle path.
export const BUTTON_KEY = {
  // System
  UNKNOWN: 0,
  HOME: 3,
  BACK: 4,
  // Phone call (KEYCODE_CALL / KEYCODE_ENDCALL)
  PHONE_ACCEPT: 5,
  PHONE_DECLINE: 6,
  // Numeric (DTMF / dialer pad)
  KEY_0: 7,
  KEY_1: 8,
  KEY_2: 9,
  KEY_3: 10,
  KEY_4: 11,
  KEY_5: 12,
  KEY_6: 13,
  KEY_7: 14,
  KEY_8: 15,
  KEY_9: 16,
  KEY_STAR: 17,
  KEY_POUND: 18,
  // D-PAD navigation
  DPAD_UP: 19,
  DPAD_DOWN: 20,
  DPAD_LEFT: 21,
  DPAD_RIGHT: 22,
  DPAD_CENTER: 23,
  // Volume (informational — NOT advertised)
  VOLUME_UP: 24,
  VOLUME_DOWN: 25,
  POWER: 26,
  // General
  ENTER: 66,
  HEADSETHOOK: 79,
  MENU: 82,
  SEARCH: 84,
  // Media transport
  MEDIA_PLAY_PAUSE: 85,
  MEDIA_STOP: 86,
  MEDIA_NEXT: 87,
  MEDIA_PREV: 88,
  MEDIA_REWIND: 89,
  MEDIA_FAST_FWD: 90,
  MUTE: 91,
  ESCAPE: 111,
  MEDIA_PLAY: 126,
  MEDIA_PAUSE: 127,
  VOLUME_MUTE: 164,
  // Voice / assistant
  ASSIST: 219,
  VOICE_ASSIST: 231,
  // Map / list navigation
  NAVIGATE_PREVIOUS: 260,
  NAVIGATE_NEXT: 261,
  NAVIGATE_IN: 262,
  NAVIGATE_OUT: 263,
  // AA-specific extensions
  ROTARY_CONTROLLER: 65536,
  MEDIA: 65537,
  TERTIARY_BUTTON: 65543,
  TURN_CARD: 65544
} as const

export interface TouchPointer {
  /** Absolute X in advertised touchscreen pixel space. */
  x: number
  /** Absolute Y in advertised touchscreen pixel space. */
  y: number
  /** Per-finger identifier (0 for single-touch; stable across DOWN→MOVED→UP). */
  id: number
}

export class InputChannel extends EventEmitter {
  constructor(private readonly _send: SendFn) {
    super()
  }

  /**
   * Send a touch event to the phone.
   * @param action      one of TOUCH_ACTION.* (PointerAction enum value)
   * @param pointers    absolute pixel coordinates in advertised touchscreen space
   * @param actionIndex index in `pointers` of the pointer that triggered
   *                    `action` — only meaningful for POINTER_DOWN/POINTER_UP,
   *                    ignored by AA for plain DOWN/MOVED/UP. Defaults to 0.
   */
  sendTouch(action: number, pointers: TouchPointer[], actionIndex = 0): void {
    if (pointers.length === 0) return
    const tsMicros = BigInt(Date.now()) * 1_000n

    // TouchEvent.Pointer (field 1, repeated)
    const pointerSubmsgs = pointers.map((p) =>
      fieldLenDelim(
        1,
        Buffer.concat([fieldVarint(1, p.x), fieldVarint(2, p.y), fieldVarint(3, p.id)])
      )
    )

    // TouchEvent.action_index (field 2) + action (field 3)
    const touchEventBuf = Buffer.concat([
      ...pointerSubmsgs,
      fieldVarint(2, actionIndex),
      fieldVarint(3, action)
    ])

    // InputReport.timestamp (field 1, uint64 varint) + touch_event (field 3)
    const msgBuf = Buffer.concat([fieldVarint(1, tsMicros), fieldLenDelim(3, touchEventBuf)])

    this._send(CH.INPUT, FRAME_FLAGS.ENC_SIGNAL, INPUT_MSG.INPUT_REPORT, msgBuf)
  }

  /**
   * Send a rotary-controller delta event. AA's list/picker views consume
   * RelativeEvent.delta to scroll through items — this is the path openauto
   * takes for KEYCODE_ROTARY_CONTROLLER rotation. DPAD_UP/DOWN/LEFT/RIGHT
   * key events typically only navigate between focusable regions, NOT inside
   * a list. For in-list scrolling, send rotary deltas.
   *
   * @param direction -1 = previous (left/up), +1 = next (right/down)
   */
  sendRotary(direction: -1 | 1): void {
    const tsMicros = BigInt(Date.now()) * 1_000n
    // RelativeEvent.Rel: keycode (field 1), delta (field 2). int32 -1 is
    // sign-extended to 10-byte varint by encodeVarint via two's-complement
    // wrap to uint64.
    const relBuf = Buffer.concat([
      fieldVarint(1, 65536), // KEYCODE_ROTARY_CONTROLLER
      fieldVarint(2, direction) // int32 delta (-1 or +1)
    ])
    // RelativeEvent.data (field 1, repeated Rel)
    const relativeEventBuf = fieldLenDelim(1, relBuf)
    // InputReport.timestamp (field 1) + relative_event (field 6)
    const msgBuf = Buffer.concat([fieldVarint(1, tsMicros), fieldLenDelim(6, relativeEventBuf)])
    this._send(CH.INPUT, FRAME_FLAGS.ENC_SIGNAL, INPUT_MSG.INPUT_REPORT, msgBuf)
  }

  /**
   * Send a HW key event to the phone.
   *
   * Accepts either a single keycode or an array. When called with an array,
   * all keycodes are packed into the same `KeyEvent.keys` repeated field —
   * phone-side this looks like several simultaneous physical keys (think
   * Shift+A). AA's nav handlers don't treat extra keys as modifiers though;
   * they pick whichever keycode they understand for the current focus
   * context. We exploit that to send navigation events that work both in
   * tab-strip contexts (DPAD_LEFT/RIGHT) and inside lists/pickers
   * (NAVIGATE_PREVIOUS/NEXT) without having to know which one is on screen.
   *
   * @param keyCode   Android KeyEvent.KEYCODE_*, or array of such
   * @param down      true on press, false on release. Phone expects DOWN+UP pair.
   * @param longpress optional KeyEvent.longpress flag (default false).
   */
  sendButton(keyCode: number | readonly number[], down: boolean, longpress = false): void {
    const codes = Array.isArray(keyCode) ? keyCode : [keyCode as number]
    if (codes.length === 0) return
    const tsMicros = BigInt(Date.now()) * 1_000n

    // KeyEvent.keys (field 1, repeated Key). All four fields written
    // explicitly to match openauto's InputSourceService::onButtonEvent —
    // some AA versions require longpress to be present even when false.
    const keyEntries = codes.map((kc) => {
      const keyBuf = Buffer.concat([
        fieldVarint(1, kc), // keycode
        fieldVarint(2, down ? 1 : 0), // down (bool wire = varint)
        fieldVarint(3, 0), // metastate — no modifiers
        fieldVarint(4, longpress ? 1 : 0) // longpress
      ])
      return fieldLenDelim(1, keyBuf)
    })
    const keyEventBuf = Buffer.concat(keyEntries)

    // InputReport.timestamp (field 1) + key_event (field 4)
    const msgBuf = Buffer.concat([fieldVarint(1, tsMicros), fieldLenDelim(4, keyEventBuf)])

    this._send(CH.INPUT, FRAME_FLAGS.ENC_SIGNAL, INPUT_MSG.INPUT_REPORT, msgBuf)
  }
}
