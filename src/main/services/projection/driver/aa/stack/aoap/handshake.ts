/**
 * AOAP handshake helpers — switch a stock Android phone into accessory mode
 * Spec: https://source.android.com/docs/core/interaction/accessories/aoa
 */

import type { Device } from 'usb'
import {
  ACCESSORY_PIDS,
  AOAP_DESCRIPTION,
  AOAP_MANUFACTURER,
  AOAP_MODEL,
  AOAP_SERIAL,
  AOAP_URI,
  AOAP_VERSION,
  GOOGLE_VID,
  REQ_GET_PROTOCOL,
  REQ_SEND_STRING,
  REQ_START,
  STRING_DESCRIPTION,
  STRING_MANUFACTURER,
  STRING_MODEL,
  STRING_SERIAL,
  STRING_URI,
  STRING_VERSION
} from './constants.js'

// libusb bmRequestType nibbles — vendor/device endpoints.
const REQ_TYPE_VENDOR_DEVICE_IN = 0xc0 // host-to-device, vendor, device, IN
const REQ_TYPE_VENDOR_DEVICE_OUT = 0x40 // host-to-device, vendor, device, OUT

const TRANSFER_TIMEOUT_MS = 2_000

export function isAccessoryMode(device: Device): boolean {
  const d = device.deviceDescriptor
  return d.idVendor === GOOGLE_VID && (ACCESSORY_PIDS as readonly number[]).includes(d.idProduct)
}

function controlTransfer(
  device: Device,
  bmRequestType: number,
  bRequest: number,
  wValue: number,
  wIndex: number,
  dataOrLength: Buffer | number
): Promise<Buffer | undefined> {
  return new Promise((resolve, reject) => {
    let settled = false
    const t = setTimeout(() => {
      if (settled) return
      settled = true
      reject(new Error(`AOAP control transfer timeout (req=${bRequest})`))
    }, TRANSFER_TIMEOUT_MS)

    try {
      device.controlTransfer(bmRequestType, bRequest, wValue, wIndex, dataOrLength, (err, data) => {
        if (settled) return
        settled = true
        clearTimeout(t)
        if (err) return reject(err)
        resolve(data instanceof Buffer ? data : undefined)
      })
    } catch (err) {
      settled = true
      clearTimeout(t)
      reject(err as Error)
    }
  })
}

async function getProtocol(device: Device): Promise<number> {
  const data = await controlTransfer(device, REQ_TYPE_VENDOR_DEVICE_IN, REQ_GET_PROTOCOL, 0, 0, 2)
  if (!data || data.length < 2) {
    throw new Error('AOAP getProtocol returned no data')
  }
  return data.readUInt16LE(0)
}

export async function probeAaCapable(device: Device): Promise<number> {
  let opened = false
  try {
    try {
      device.open()
      opened = true
    } catch {
      return 0
    }
    const proto = await getProtocol(device)
    return Number.isFinite(proto) && proto >= 1 ? proto : 0
  } catch {
    return 0
  } finally {
    if (opened) {
      try {
        device.close()
      } catch {}
    }
  }
}

async function sendString(device: Device, index: number, value: string): Promise<void> {
  const buf = Buffer.from(`${value}\0`, 'utf8')
  await controlTransfer(device, REQ_TYPE_VENDOR_DEVICE_OUT, REQ_SEND_STRING, 0, index, buf)
}

async function startAccessory(device: Device): Promise<void> {
  await controlTransfer(device, REQ_TYPE_VENDOR_DEVICE_OUT, REQ_START, 0, 0, Buffer.alloc(0))
}

export async function runAoapHandshake(device: Device): Promise<void> {
  if (isAccessoryMode(device)) {
    // Already in accessory mode
    return
  }

  const proto = await getProtocol(device)
  if (proto < 1) {
    throw new Error(`AOAP protocol version ${proto} not supported by device`)
  }

  await sendString(device, STRING_MANUFACTURER, AOAP_MANUFACTURER)
  await sendString(device, STRING_MODEL, AOAP_MODEL)
  await sendString(device, STRING_DESCRIPTION, AOAP_DESCRIPTION)
  await sendString(device, STRING_VERSION, AOAP_VERSION)
  await sendString(device, STRING_URI, AOAP_URI)
  await sendString(device, STRING_SERIAL, AOAP_SERIAL)

  await startAccessory(device)
}
