/**
 * AOAP handshake helpers — switch a stock Android phone into accessory mode
 * Spec: https://source.android.com/docs/core/interaction/accessories/aoa
 */

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

type Device = USBDevice

const TRANSFER_TIMEOUT_MS = 2_000

export function isAccessoryMode(device: Device): boolean {
  return (
    device.vendorId === GOOGLE_VID &&
    (ACCESSORY_PIDS as readonly number[]).includes(device.productId)
  )
}

function withTimeout<T>(p: Promise<T>, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(
      () => reject(new Error(`AOAP control transfer timeout (${label})`)),
      TRANSFER_TIMEOUT_MS
    )
    p.then(
      (v) => {
        clearTimeout(t)
        resolve(v)
      },
      (e) => {
        clearTimeout(t)
        reject(e as Error)
      }
    )
  })
}

// AOAP uses vendor/device control transfers. WebUSB models direction via the In/Out method.
async function controlIn(
  device: Device,
  request: number,
  value: number,
  index: number,
  length: number
): Promise<Buffer> {
  const r = await withTimeout(
    device.controlTransferIn(
      { requestType: 'vendor', recipient: 'device', request, value, index },
      length
    ),
    `req=${request}`
  )
  if (r.status !== 'ok' || !r.data) {
    throw new Error(`AOAP control IN req=${request} status=${r.status ?? 'no-data'}`)
  }
  return Buffer.from(r.data.buffer, r.data.byteOffset, r.data.byteLength)
}

async function controlOut(
  device: Device,
  request: number,
  value: number,
  index: number,
  data: Buffer
): Promise<void> {
  // Always pass a (possibly empty) buffer: the WebUSB layer's toUint8Array dereferences `.buffer`,
  // so `undefined` for a zero-length payload (e.g. AOAP START) would throw.
  const r = await withTimeout(
    device.controlTransferOut(
      { requestType: 'vendor', recipient: 'device', request, value, index },
      data
    ),
    `req=${request}`
  )
  if (r.status !== 'ok') {
    throw new Error(`AOAP control OUT req=${request} status=${r.status}`)
  }
}

async function getProtocol(device: Device): Promise<number> {
  const data = await controlIn(device, REQ_GET_PROTOCOL, 0, 0, 2)
  if (data.length < 2) {
    throw new Error('AOAP getProtocol returned no data')
  }
  return data.readUInt16LE(0)
}

export async function probeAaCapable(
  device: Device,
  opts?: { resetOnBusy?: boolean }
): Promise<number> {
  let opened = false
  try {
    try {
      await device.open()
      opened = true
    } catch (err) {
      console.log(`[AOAP] probe open failed: ${(err as Error).message}`)
      return 0
    }
    try {
      const proto = await withClaimedInterface(device, () => getProtocol(device))
      console.log(`[AOAP] probe getProtocol → ${proto}`)
      return Number.isFinite(proto) && proto >= 1 ? proto : 0
    } catch (err) {
      // Interface still held exclusively after retries (a previous run's reset() can leave macOS
      // holding it). Force a clean re-enumeration so the hotplug path re-probes a fresh handle —
      // self-healing, no physical replug. Scoped to startup so it can't loop on the fresh device.
      if (opts?.resetOnBusy && isExclusiveAccessError(err)) {
        console.log('[AOAP] interface still busy after retries — resetting device to recover')
        await settleWithin(device.reset(), 2000)
        return 0
      }
      throw err
    }
  } catch (err) {
    console.log(`[AOAP] probe threw: ${(err as Error).message}`)
    return 0
  } finally {
    if (opened) {
      try {
        await device.close()
      } catch {}
    }
  }
}

async function sendString(device: Device, index: number, value: string): Promise<void> {
  const buf = Buffer.from(`${value}\0`, 'utf8')
  await controlOut(device, REQ_SEND_STRING, 0, index, buf)
}

async function startAccessory(device: Device): Promise<void> {
  await controlOut(device, REQ_START, 0, 0, Buffer.alloc(0))
}

// macOS reports a held interface as kIOReturnExclusiveAccess (0xe00002c5).
function isExclusiveAccessError(err: unknown): boolean {
  const msg = (err as Error)?.message ?? ''
  return msg.includes('exclusive') || msg.includes('0xe00002c5')
}

// Resolves once `p` settles or after `ms`, whichever comes first (never rejects).
function settleWithin<T>(p: Promise<T>, ms: number): Promise<void> {
  return Promise.race([
    p.then(
      () => undefined,
      () => undefined
    ),
    new Promise<void>((r) => setTimeout(r, ms))
  ])
}

async function claimInterfaceWithRetry(device: Device, ifaceNum: number): Promise<void> {
  const MAX_ATTEMPTS = 8
  const DELAY_MS = 500
  for (let attempt = 1; ; attempt++) {
    try {
      await device.claimInterface(ifaceNum)
      return
    } catch (err) {
      if (!isExclusiveAccessError(err) || attempt >= MAX_ATTEMPTS) throw err
      console.log(`[AOAP] interface busy (exclusive access), retry ${attempt}/${MAX_ATTEMPTS - 1}`)
      await new Promise((r) => setTimeout(r, DELAY_MS))
    }
  }
}

async function withClaimedInterface<T>(device: Device, fn: () => Promise<T>): Promise<T> {
  if (!device.configuration && device.configurations.length > 0) {
    try {
      await device.selectConfiguration(device.configurations[0]!.configurationValue)
    } catch {
      /* ignore */
    }
  }
  const ifaceNum = device.configuration?.interfaces[0]?.interfaceNumber ?? 0
  await claimInterfaceWithRetry(device, ifaceNum)
  try {
    return await fn()
  } finally {
    try {
      await device.releaseInterface(ifaceNum)
    } catch {
      /* device may have re-enumerated (after START) */
    }
  }
}

export async function runAoapHandshake(device: Device): Promise<void> {
  if (isAccessoryMode(device)) {
    // Already in accessory mode
    return
  }

  await withClaimedInterface(device, async () => {
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
  })
}
