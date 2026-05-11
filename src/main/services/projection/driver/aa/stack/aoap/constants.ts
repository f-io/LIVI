/**
 * Android Open Accessory Protocol (AOAP) constants.
 * Spec: https://source.android.com/docs/core/interaction/accessories/aoa
 */

// Vendor IDs
export const GOOGLE_VID = 0x18d1

// Accessory-mode product IDs (phone re-enumerates after startAccessoryMode)
export const ACCESSORY_PIDS = [
  0x2d00, // Accessory
  0x2d01, // Accessory + ADB
  0x2d02, // Audio
  0x2d03, // Audio + ADB
  0x2d04, // Accessory + Audio
  0x2d05 // Accessory + Audio + ADB
] as const

// Vendor-specific USB control transfer requests
export const REQ_GET_PROTOCOL = 51
export const REQ_SEND_STRING = 52
export const REQ_START = 53

// String indices for REQ_SEND_STRING
export const STRING_MANUFACTURER = 0
export const STRING_MODEL = 1
export const STRING_DESCRIPTION = 2
export const STRING_VERSION = 3
export const STRING_URI = 4
export const STRING_SERIAL = 5

// AOAP-host identification advertised to the phone via SEND_STRING.
//
// IMPORTANT: `MANUFACTURER` and `MODEL` are NOT cosmetic
//
// DESCRIPTION / VERSION / URI / SERIAL are free-form and only surface
// in the connection dialog.
export const AOAP_MANUFACTURER = 'Android'
export const AOAP_MODEL = 'Android Auto'
export const AOAP_DESCRIPTION = 'LIVI Wired Android Auto host'
export const AOAP_VERSION = '2.0.1'
export const AOAP_URI = 'https://github.com/f-io/LIVI'
export const AOAP_SERIAL = 'LIVI-0001'

// Loopback address the bridge advertises for the AA TcpServer to connect to.
// Random-ish high port to avoid clashing with the wireless 5277.
export const AOAP_LOOPBACK_HOST = '127.0.0.1'
export const AOAP_LOOPBACK_PORT = 5278

// AOAP handshake timing. Values are conservative — increase if real phones
// need more time on slower buses.
export const AOAP_RE_ENUMERATE_TIMEOUT_MS = 5_000
export const AOAP_BULK_TRANSFER_TIMEOUT_MS = 5_000
