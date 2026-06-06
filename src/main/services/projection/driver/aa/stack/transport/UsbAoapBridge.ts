/**
 * USB <-> TCP-loopback bridge for wired Android Auto.
 *
 * The phone, after the AOAP handshake, exposes two bulk USB endpoints
 * carrying the AA byte stream.
 *
 * Lifecycle:
 *   start()  → AOAP handshake (if needed) + claim accessory iface + open loopback server on :5278
 *   <client connects> → bidirectional pump runs until either side closes
 *   stop()   → drain pump, release iface, reset() (re-enumerates the phone), close
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'
import { usb } from 'usb'
import {
  ACCESSORY_PIDS,
  AOAP_LOOPBACK_HOST,
  AOAP_LOOPBACK_PORT,
  AOAP_RE_ENUMERATE_TIMEOUT_MS,
  GOOGLE_VID
} from '../aoap/constants.js'
import { isAccessoryMode, runAoapHandshake } from '../aoap/handshake.js'

type Device = USBDevice

const BULK_READ_CHUNK = 16 * 1024
const IN_TRANSFER_TIMEOUT_MS = 1_000

export class UsbAoapBridge extends EventEmitter {
  private _server: net.Server | null = null
  private _client: net.Socket | null = null
  private _accessoryDevice: Device | null = null
  private _ifaceNum: number | null = null
  private _inEpNum: number | null = null
  private _outEpNum: number | null = null
  private _pumpDone: Promise<void> = Promise.resolve()
  private _running = false
  private _pumping = false
  private _outChain: Promise<void> = Promise.resolve()

  constructor(
    private readonly _device: Device,
    private readonly _onWillReenumerate?: (durationMs: number) => void
  ) {
    super()
  }

  async start(port = AOAP_LOOPBACK_PORT): Promise<void> {
    if (this._running) return
    this._running = true

    try {
      await this._switchAndOpenAccessory()
      await this._startLoopbackServer(port)
    } catch (err) {
      this._running = false
      this.emit('error', err as Error)
      throw err
    }
  }

  async drain(timeoutMs = 500): Promise<void> {
    if (!this._running) return
    const yieldMs = Math.min(50, timeoutMs)
    await new Promise<void>((r) => setTimeout(r, yieldMs))
    if (!this._running) return

    const remaining = Math.max(0, timeoutMs - yieldMs)
    let timer: NodeJS.Timeout | null = null
    try {
      await Promise.race([
        this._outChain,
        new Promise<void>((resolve) => {
          timer = setTimeout(resolve, remaining)
        })
      ])
    } finally {
      if (timer) clearTimeout(timer)
    }
  }

  async forceReenum(): Promise<void> {
    // Stop the pump + loopback so stop() can release/reset cleanly.
    this._pumping = false
    try {
      this._client?.destroy()
    } catch {}
    this._client = null
    try {
      this._server?.close()
    } catch {}
    this._server = null
  }

  async stop(): Promise<void> {
    if (!this._running) return
    this._running = false
    this._pumping = false

    const dev = this._accessoryDevice
    const ifaceNum = this._ifaceNum
    const outChain = this._outChain
    this._accessoryDevice = null
    this._ifaceNum = null
    this._inEpNum = null
    this._outEpNum = null

    try {
      this._client?.destroy()
    } catch {
      /* already destroyed */
    }
    this._client = null

    try {
      this._server?.close()
    } catch {
      /* already closed */
    }
    this._server = null

    const withWatchdog = <T>(p: Promise<T>, ms: number): Promise<void> =>
      Promise.race([
        p.then(
          () => undefined,
          () => undefined
        ),
        new Promise<void>((r) => setTimeout(r, ms))
      ])

    // Drop the in-flight read so the interface is free for release/reset
    await withWatchdog(this._pumpDone, IN_TRANSFER_TIMEOUT_MS + 750)
    this._pumpDone = Promise.resolve()

    // Let any in-flight OUT settle (bounded)
    await withWatchdog(outChain, 500)

    if (dev) {
      if (ifaceNum != null) {
        await withWatchdog(dev.releaseInterface(ifaceNum), 750)
      }

      await withWatchdog(
        dev.reset().then(
          () => console.log('[UsbAoapBridge] device reset ok — phone should re-enumerate'),
          (err: unknown) => console.warn(`[UsbAoapBridge] device reset failed: ${String(err)}`)
        ),
        1500
      )

      await withWatchdog(
        dev.close().then(
          () => console.log('[UsbAoapBridge] accessory device closed'),
          (err: unknown) => console.warn(`[UsbAoapBridge] device close failed: ${String(err)}`)
        ),
        1500
      )
    }

    this.emit('closed')
  }

  private async _switchAndOpenAccessory(): Promise<void> {
    let accessoryDev: Device

    if (isAccessoryMode(this._device)) {
      accessoryDev = this._device
      await this._openWithRetry(accessoryDev, 'AOAP accessory device')
    } else {
      await this._openWithRetry(this._device, 'AOAP device')

      const reenumerated = waitForAccessoryAttach(AOAP_RE_ENUMERATE_TIMEOUT_MS)
      void reenumerated.catch(() => {}) // avoid an unhandled rejection if the handshake throws first
      this._onWillReenumerate?.(AOAP_RE_ENUMERATE_TIMEOUT_MS + 2_000)
      await runAoapHandshake(this._device)

      try {
        await this._device.close()
      } catch {
        /* ignore */
      }

      accessoryDev = await reenumerated
      await this._openWithRetry(accessoryDev, 'AOAP accessory device (post-handshake)')
    }

    // WebUSB does not auto-select a configuration on open.
    try {
      if (accessoryDev.configuration?.configurationValue !== 1) {
        await accessoryDev.selectConfiguration(1)
      }
    } catch (err) {
      console.warn(`[UsbAoapBridge] selectConfiguration(1) failed: ${(err as Error).message}`)
    }

    const eps = findBulkEndpoints(accessoryDev)
    if (!eps) {
      throw new Error('AOAP accessory: bulk IN/OUT endpoints not found')
    }

    // Right after enumeration the OS may briefly refuse the claim.
    let claimed = false
    let claimErr: unknown
    for (let attempt = 0; attempt < 5 && !claimed; attempt++) {
      try {
        await accessoryDev.claimInterface(eps.interfaceNumber)
        claimed = true
      } catch (err) {
        claimErr = err
        await new Promise((r) => setTimeout(r, 100))
      }
    }
    if (!claimed) {
      throw new Error(
        `Failed to claim AOAP accessory interface: ${(claimErr as Error)?.message ?? 'unknown'}`
      )
    }

    this._accessoryDevice = accessoryDev
    this._ifaceNum = eps.interfaceNumber
    this._inEpNum = eps.inEndpoint
    this._outEpNum = eps.outEndpoint
  }

  private async _openWithRetry(dev: Device, label: string): Promise<void> {
    let lastErr: unknown
    for (let attempt = 0; attempt < 5; attempt++) {
      try {
        await dev.open()
        return
      } catch (err) {
        lastErr = err
        await new Promise((r) => setTimeout(r, 100))
      }
    }
    throw new Error(`Failed to open ${label}: ${(lastErr as Error)?.message ?? 'unknown'}`)
  }

  private async _startLoopbackServer(port: number): Promise<void> {
    this._server = net.createServer({ allowHalfOpen: true }, (sock) => {
      if (this._client) {
        try {
          this._client.destroy()
        } catch {
          /* ignore */
        }
      }
      this._client = sock
      sock.setNoDelay(true)
      this._startPump(sock)
    })

    this._server.on('error', (err) => this.emit('error', err))

    await new Promise<void>((resolve, reject) => {
      const onErr = (err: Error) => reject(err)
      this._server!.once('error', onErr)
      this._server!.listen(port, AOAP_LOOPBACK_HOST, () => {
        this._server!.removeListener('error', onErr)
        resolve()
      })
    })

    this.emit('ready', { host: AOAP_LOOPBACK_HOST, port })
  }

  private _startPump(sock: net.Socket): void {
    const dev = this._accessoryDevice
    if (!dev || this._inEpNum == null || this._outEpNum == null) {
      sock.destroy(new Error('AOAP bridge endpoints not initialised'))
      return
    }
    const inEpNum = this._inEpNum
    const outEpNum = this._outEpNum

    this._pumping = true
    this._outChain = Promise.resolve()

    // USB IN -> socket. stop() awaits _pumpDone so the interface is free before reset().
    this._pumpDone = this._pumpIn(sock, dev, inEpNum)

    // socket -> USB OUT, serialised through _outChain.
    sock.on('data', (chunk: Buffer) => {
      if (!this._pumping) return
      this._outChain = this._outChain.then(async () => {
        if (!this._pumping || this._client !== sock) return
        try {
          await dev.transferOut(outEpNum, chunk)
        } catch (err) {
          this.emit('error', err as Error)
          try {
            sock.destroy(err as Error)
          } catch {
            /* ignore */
          }
        }
      })
    })

    sock.once('close', () => {
      if (this._client !== sock) return
      this._pumping = false
      this._client = null
    })

    sock.once('error', (err) => {
      this.emit('error', err)
    })
  }

  private async _pumpIn(sock: net.Socket, dev: Device, inEpNum: number): Promise<void> {
    // transferIn's 3rd arg (timeout) is outside the standard WebUSB typings.
    const transferIn = dev.transferIn.bind(dev) as (
      ep: number,
      len: number,
      timeoutMs?: number
    ) => Promise<USBInTransferResult>

    while (this._pumping && this._client === sock) {
      let data: DataView | null
      try {
        const r = await transferIn(inEpNum, BULK_READ_CHUNK, IN_TRANSFER_TIMEOUT_MS)
        if (r.status === 'stall') {
          try {
            await dev.clearHalt('in', inEpNum)
          } catch {
            /* ignore */
          }
          continue
        }
        data = r.data ?? null
      } catch (err) {
        if (!this._pumping || this._client !== sock) return
        const msg = err instanceof Error ? err.message : String(err)
        if (/disconnect|no[\s_-]?device|not[\s_-]?found|gone/i.test(msg)) {
          this.emit('error', err as Error)
          try {
            sock.destroy(err as Error)
          } catch {
            /* ignore */
          }
          return
        }
        // Timeout / cancel: re-issue the read (lossless for bulk).
        continue
      }

      if (!this._pumping || this._client !== sock) return
      if (!data || data.byteLength === 0) continue

      const buf = Buffer.from(data.buffer, data.byteOffset, data.byteLength)
      if (!sock.write(buf)) {
        await new Promise<void>((resolve) => sock.once('drain', resolve))
      }
    }
  }

  get device(): Device {
    return this._device
  }
}

interface BulkEndpoints {
  interfaceNumber: number
  inEndpoint: number
  outEndpoint: number
}

function findBulkEndpoints(dev: Device): BulkEndpoints | null {
  const config = dev.configuration
  if (!config) return null
  for (const iface of config.interfaces) {
    const alt = iface.alternate
    let inEp = -1
    let outEp = -1
    for (const ep of alt.endpoints) {
      if (ep.type !== 'bulk') continue
      if (ep.direction === 'in') inEp = ep.endpointNumber
      else if (ep.direction === 'out') outEp = ep.endpointNumber
    }
    if (inEp >= 0 && outEp >= 0) {
      return { interfaceNumber: iface.interfaceNumber, inEndpoint: inEp, outEndpoint: outEp }
    }
  }
  return null
}

function waitForAccessoryAttach(timeoutMs: number): Promise<Device> {
  return new Promise((resolve, reject) => {
    const onConnect = (ev: USBConnectionEvent): void => {
      const dev = ev.device as Device
      if (
        dev.vendorId === GOOGLE_VID &&
        (ACCESSORY_PIDS as readonly number[]).includes(dev.productId)
      ) {
        cleanup()
        resolve(dev)
      }
    }
    const t = setTimeout(() => {
      cleanup()
      reject(new Error('AOAP re-enumerate timeout'))
    }, timeoutMs)
    const cleanup = (): void => {
      clearTimeout(t)
      try {
        usb.removeEventListener('connect', onConnect)
      } catch {
        /* ignore */
      }
    }
    usb.addEventListener('connect', onConnect)
  })
}
