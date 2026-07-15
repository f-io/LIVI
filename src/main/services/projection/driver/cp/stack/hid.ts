/**
 * hid — CarPlay multitouch HID device + report encoding.
 *
 * The accessory declares a touchscreen digitizer in /info's hidDevices and
 * delivers touch to the phone as HID reports wrapped in a "hidSendReport"
 * command over the event connection. The descriptor declares two Finger logical
 * collections, each carrying an
 * 8-bit Transducer Index, a 1-bit Touch (+7 pad), and absolute 16-bit X/Y. The
 * transducer index is the fixed slot number (0, 1); the phone tracks a finger by
 * the slot it consistently reports in. Report per finger: [index][touch][X LE][Y LE].
 */

import type { PlistValue } from './bplist'

/** Stable identifiers for our HID devices (hex, matches the "uuid" cmd field). */
export const TOUCH_HID_UID = 0x2a2a2a2a
export const KNOB_HID_UID = 0x2a2a2a2b
export const MEDIA_HID_UID = 0x2a2a2a2c
export const TELEPHONY_HID_UID = 0x2a2a2a2d

/** Number of simultaneous contacts (2 covers pinch-zoom and rotate). */
export const TOUCH_CONTACTS = 2

/** Bytes per finger in the input report: index, touch(+pad), X(16), Y(16). */
const BYTES_PER_FINGER = 6

/** One Finger logical collection. */
function fingerCollection(xMax: number, yMax: number): number[] {
  return [
    0x05,
    0x0d, //   Usage Page (Digitizers)
    0x09,
    0x22, //   Usage (Finger)
    0xa1,
    0x02, //   Collection (Logical)
    0x09,
    0x38, //     Usage (Transducer Index)
    0x75,
    0x08, //     Report Size (8)
    0x95,
    0x01, //     Report Count (1)
    0x81,
    0x02, //     Input (Data,Var,Abs)
    0x15,
    0x00, //     Logical Minimum (0)
    0x25,
    0x01, //     Logical Maximum (1)
    0x09,
    0x33, //     Usage (Touch)
    0x75,
    0x01, //     Report Size (1)
    0x95,
    0x01, //     Report Count (1)
    0x81,
    0x02, //     Input (Data,Var,Abs)
    0x95,
    0x07, //     Report Count (7)
    0x81,
    0x03, //     Input (Cnst,Var,Abs) — padding
    0x05,
    0x01, //     Usage Page (Generic Desktop)
    0x26,
    xMax & 0xff,
    (xMax >> 8) & 0xff, // Logical Maximum (xMax)
    0x09,
    0x30, //     Usage (X)
    0x75,
    0x10, //     Report Size (16)
    0x95,
    0x01, //     Report Count (1)
    0x81,
    0x02, //     Input (Data,Var,Abs)
    0x26,
    yMax & 0xff,
    (yMax >> 8) & 0xff, // Logical Maximum (yMax)
    0x09,
    0x31, //     Usage (Y)
    0x81,
    0x02, //     Input (Data,Var,Abs)
    0xc0 //   End Collection
  ]
}

function multitouchDescriptor(xMax: number, yMax: number): Buffer {
  const bytes: number[] = [
    0x05,
    0x0d, // Usage Page (Digitizers)
    0x09,
    0x04, // Usage (Touch Screen)
    0xa1,
    0x01 // Collection (Application)
  ]
  for (let i = 0; i < TOUCH_CONTACTS; i++) bytes.push(...fingerCollection(xMax, yMax))
  bytes.push(0xc0) // End Collection
  return Buffer.from(bytes)
}

/** The hidDevices entry for /info, sized to the display and linked to its UUID. */
export function touchHidDevice(xMax: number, yMax: number, displayUuid: string): PlistValue {
  return {
    hidProductID: 1,
    hidVendorID: 2,
    hidCountryCode: 0,
    uuid: TOUCH_HID_UID.toString(16),
    name: 'LIVI Touchscreen',
    displayUUID: displayUuid,
    hidDescriptor: multitouchDescriptor(xMax, yMax)
  }
}

export interface Contact {
  id: number
  x: number
  y: number
  down: boolean
}

/**
 * Encode a multitouch input report: TOUCH_CONTACTS finger slots, absolute pixels.
 * Each slot writes its fixed transducer index (the slot number) then the touch
 * bit and X/Y.
 */
export function touchReport(contacts: Contact[]): Buffer {
  const r = Buffer.alloc(BYTES_PER_FINGER * TOUCH_CONTACTS)
  for (let i = 0; i < TOUCH_CONTACTS; i++) {
    const c = contacts[i]
    const off = i * BYTES_PER_FINGER
    r[off] = i // transducer index = fixed slot number
    if (!c) continue
    r[off + 1] = c.down ? 0x01 : 0x00
    r.writeUInt16LE(Math.max(0, Math.round(c.x)), off + 2)
    r.writeUInt16LE(Math.max(0, Math.round(c.y)), off + 4)
  }
  return r
}

const knobDescriptor = Buffer.from([
  0x05,
  0x01, // Usage Page (Generic Desktop)
  0x09,
  0x08, // Usage (MultiAxisController)
  0xa1,
  0x01, // Collection (Application)
  0x05,
  0x09, // Usage Page (Button)
  0x09,
  0x01, // Usage (Button 1 — primary/trigger)
  0x15,
  0x00, // Logical Minimum (0)
  0x25,
  0x01, // Logical Maximum (1)
  0x75,
  0x01, // Report Size (1)
  0x95,
  0x01, // Report Count (1)
  0x81,
  0x02, // Input (Data,Var,Abs) — bit0 Select
  0x05,
  0x0c, // Usage Page (Consumer)
  0x0a,
  0x23,
  0x02, // Usage (AC Home)
  0x0a,
  0x24,
  0x02, // Usage (AC Back)
  0x95,
  0x02, // Report Count (2)
  0x81,
  0x02, // Input (Data,Var,Abs) — bit1 Home, bit2 Back
  0x95,
  0x05, // Report Count (5)
  0x81,
  0x01, // Input (Constant) — pad bits 3-7
  0x05,
  0x01, // Usage Page (Generic Desktop)
  0x09,
  0x01, // Usage (Pointer)
  0xa1,
  0x00, // Collection (Physical)
  0x09,
  0x30, // Usage (X)
  0x09,
  0x31, // Usage (Y)
  0x15,
  0x81, // Logical Minimum (-127)
  0x25,
  0x7f, // Logical Maximum (127)
  0x75,
  0x08, // Report Size (8)
  0x95,
  0x02, // Report Count (2)
  0x81,
  0x02, // Input (Data,Var,Abs) — X, Y
  0xc0, // End Collection
  0x09,
  0x38, // Usage (Wheel)
  0x15,
  0x81, // Logical Minimum (-127)
  0x25,
  0x7f, // Logical Maximum (127)
  0x75,
  0x08, // Report Size (8)
  0x95,
  0x01, // Report Count (1)
  0x81,
  0x06, // Input (Data,Var,Rel) — Wheel
  0xc0 // End Collection
])

const mediaDescriptor = Buffer.from([
  0x05,
  0x0c, // Usage Page (Consumer)
  0x09,
  0x01, // Usage (Consumer Control)
  0xa1,
  0x01, // Collection (Application)
  0x15,
  0x00, // Logical Minimum (0)
  0x25,
  0x06, // Logical Maximum (6)
  0x05,
  0x0c, // Usage Page (Consumer)
  0x0a,
  0x00,
  0x00, // Usage (Unassigned) — index 0
  0x0a,
  0xb0,
  0x00, // Usage (Play) — 1
  0x0a,
  0xb1,
  0x00, // Usage (Pause) — 2
  0x0a,
  0xcd,
  0x00, // Usage (Play/Pause) — 3
  0x0a,
  0xb5,
  0x00, // Usage (Scan Next Track) — 4
  0x0a,
  0xb6,
  0x00, // Usage (Scan Previous Track) — 5
  0x0a,
  0x9e,
  0x02, // Usage (AC Navigation Guidance) — 6
  0x75,
  0x08, // Report Size (8)
  0x95,
  0x01, // Report Count (1)
  0x81,
  0x00, // Input (Data,Array,Abs)
  0xc0 // End Collection
])

const telephonyDescriptor = Buffer.from([
  0x05,
  0x0b, // Usage Page (Telephony)
  0x09,
  0x07, // Usage (Telephony Keypad)
  0xa1,
  0x01, // Collection (Application)
  0x15,
  0x00, // Logical Minimum (0)
  0x25,
  0x11, // Logical Maximum (17)
  0x05,
  0x0b, // Usage Page (Telephony)
  0x09,
  0x00, // Usage (Unassigned) — index 0
  0x09,
  0x20, // Usage (Hook Switch) — 1
  0x09,
  0x21, // Usage (Flash) — 2
  0x09,
  0x26, // Usage (Drop) — 3
  0x09,
  0x2f, // Usage (Mute) — 4
  0x09,
  0xb0, // Usage (Phone Key 0) — 5
  0x09,
  0xb1, // Usage (Phone Key 1) — 6
  0x09,
  0xb2, // Usage (Phone Key 2) — 7
  0x09,
  0xb3, // Usage (Phone Key 3) — 8
  0x09,
  0xb4, // Usage (Phone Key 4) — 9
  0x09,
  0xb5, // Usage (Phone Key 5) — 10
  0x09,
  0xb6, // Usage (Phone Key 6) — 11
  0x09,
  0xb7, // Usage (Phone Key 7) — 12
  0x09,
  0xb8, // Usage (Phone Key 8) — 13
  0x09,
  0xb9, // Usage (Phone Key 9) — 14
  0x09,
  0xba, // Usage (Phone Key Star) — 15
  0x09,
  0xbb, // Usage (Phone Key Pound) — 16
  0x05,
  0x07, // Usage Page (Keyboard/Keypad)
  0x09,
  0x2a, // Usage (Keyboard DELETE) — 17
  0x75,
  0x08, // Report Size (8)
  0x95,
  0x01, // Report Count (1)
  0x81,
  0x00, // Input (Data,Array,Abs)
  0xc0 // End Collection
])

function hidDeviceEntry(
  uid: number,
  name: string,
  descriptor: Buffer,
  displayUuid: string
): PlistValue {
  return {
    hidProductID: 1,
    hidVendorID: 2,
    hidCountryCode: 0,
    uuid: uid.toString(16),
    name,
    displayUUID: displayUuid,
    hidDescriptor: descriptor
  }
}

export function knobHidDevice(displayUuid: string): PlistValue {
  return hidDeviceEntry(KNOB_HID_UID, 'LIVI Knob', knobDescriptor, displayUuid)
}

export function mediaHidDevice(displayUuid: string): PlistValue {
  return hidDeviceEntry(MEDIA_HID_UID, 'LIVI Media', mediaDescriptor, displayUuid)
}

export function telephonyHidDevice(displayUuid: string): PlistValue {
  return hidDeviceEntry(TELEPHONY_HID_UID, 'LIVI Telephony', telephonyDescriptor, displayUuid)
}

export const MediaButton = {
  none: 0,
  play: 1,
  pause: 2,
  playPause: 3,
  next: 4,
  prev: 5,
  navGuidance: 6
} as const

export const TelephonyButton = {
  none: 0,
  hookSwitch: 1,
  flash: 2,
  drop: 3,
  mute: 4,
  key0: 5,
  key1: 6,
  key2: 7,
  key3: 8,
  key4: 9,
  key5: 10,
  key6: 11,
  key7: 12,
  key8: 13,
  key9: 14,
  star: 15,
  pound: 16,
  del: 17
} as const

export interface KnobState {
  select?: boolean
  home?: boolean
  back?: boolean
  x?: number
  y?: number
  wheel?: number
}

function clampAxis(v: number): number {
  const n = Math.round(v)
  return n < -127 ? -127 : n > 127 ? 127 : n
}

export function knobReport(s: KnobState): Buffer {
  const r = Buffer.alloc(4)
  r[0] = (s.select ? 0x01 : 0) | (s.home ? 0x02 : 0) | (s.back ? 0x04 : 0)
  r.writeInt8(clampAxis(s.x ?? 0), 1)
  r.writeInt8(clampAxis(s.y ?? 0), 2)
  r.writeInt8(clampAxis(s.wheel ?? 0), 3)
  return r
}

export function mediaReport(index: number): Buffer {
  return Buffer.from([index & 0xff])
}

export function telephonyReport(index: number): Buffer {
  return Buffer.from([index & 0xff])
}
