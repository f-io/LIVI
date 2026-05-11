/**
 * USB <-> TCP-loopback bridge for wired Android Auto.
 *
 * The phone, after the AOAP handshake, exposes two bulk USB endpoints
 * carrying the AA byte stream.
 *
 * Lifecycle:
 *   start()  → AOAP handshake (if needed) + claim accessory iface +
 *              open loopback server on 127.0.0.1:5278
 *   <client connects>
 *              → bidirectional pump runs until either side closes
 *   stop()   → close socket, release iface, close server
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'
import { type Device, type InEndpoint, type Interface, type OutEndpoint, usb } from 'usb'
import {
  ACCESSORY_PIDS,
  AOAP_LOOPBACK_HOST,
  AOAP_LOOPBACK_PORT,
  AOAP_RE_ENUMERATE_TIMEOUT_MS,
  GOOGLE_VID
} from '../aoap/constants.js'
import { isAccessoryMode, runAoapHandshake } from '../aoap/handshake.js'

const BULK_READ_CHUNK = 16 * 1024
// libusb endpoint flags
const USB_ENDPOINT_IN = 0x80
const USB_TRANSFER_TYPE_BULK = 0x02

export class UsbAoapBridge extends EventEmitter {
  private _server: net.Server | null = null
  private _client: net.Socket | null = null
  private _accessoryDevice: Device | null = null
  private _iface: Interface | null = null
  private _inEp: InEndpoint | null = null
  private _outEp: OutEndpoint | null = null
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
    const dev = this._accessoryDevice
    const iface = this._iface
    if (!dev) return

    this._pumping = false
    try {
      this._inEp?.stopPoll()
    } catch {}
    try {
      this._client?.destroy()
    } catch {}
    this._client = null
    try {
      this._server?.close()
    } catch {}
    this._server = null

    if (iface) {
      await new Promise<void>((resolve) => {
        try {
          iface.release(true, () => resolve())
        } catch {
          resolve()
        }
      })
    }
    this._iface = null
    this._inEp = null
    this._outEp = null

    await new Promise<void>((resolve) => {
      try {
        dev.controlTransfer(0x00, 0x09, 0, 0, Buffer.alloc(0), (err: unknown) => {
          if (err) {
            console.warn(`[UsbAoapBridge] SET_CONFIGURATION(0) failed: ${String(err)}`)
          } else {
            console.log('[UsbAoapBridge] SET_CONFIGURATION(0) ok')
          }
          resolve()
        })
      } catch (err) {
        console.warn(`[UsbAoapBridge] SET_CONFIGURATION(0) threw: ${(err as Error).message}`)
        resolve()
      }
    })
  }

  async stop(): Promise<void> {
    if (!this._running) return
    this._running = false
    this._pumping = false

    const inEp = this._inEp
    const outChain = this._outChain
    const iface = this._iface
    const dev = this._accessoryDevice
    this._iface = null
    this._inEp = null
    this._outEp = null
    this._accessoryDevice = null

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

    const withWatchdog = (op: (done: () => void) => void, ms: number): Promise<void> =>
      new Promise<void>((resolve) => {
        let settled = false
        const finish = (): void => {
          if (settled) return
          settled = true
          resolve()
        }
        const t = setTimeout(finish, ms)
        try {
          op(() => {
            clearTimeout(t)
            finish()
          })
        } catch {
          clearTimeout(t)
          finish()
        }
      })

    if (inEp && inEp.pollActive) {
      await withWatchdog((done) => inEp.stopPoll(done), 750)
    }

    await Promise.race([
      outChain.catch(() => undefined),
      new Promise<void>((r) => setTimeout(r, 500))
    ])

    if (iface) {
      await withWatchdog((done) => iface.release(false, done), 750)
    }

    // Kick the phone back out of accessory mode
    if (dev) {
      await withWatchdog((done) => {
        try {
          dev.controlTransfer(0x00, 0x09, 0, 0, Buffer.alloc(0), () => done())
        } catch {
          done()
        }
      }, 500)
    }

    try {
      dev?.close()
    } catch {
      /* ignore */
    }

    this.emit('closed')
  }

  private async _switchAndOpenAccessory(): Promise<void> {
    let accessoryDev: Device

    if (isAccessoryMode(this._device)) {
      console.log(
        `[UsbAoapBridge] accessory-mode start path: vid=0x${this._device.deviceDescriptor.idVendor.toString(16)} pid=0x${this._device.deviceDescriptor.idProduct.toString(16)} — claiming endpoints directly`
      )
      accessoryDev = this._device
      // libusb sometimes fails the first open with "Couldn't find matching udev
      // device" if the kernel/udev hasn't finished publishing /dev/bus/usb yet
      let opened = false
      let lastErr: unknown
      for (let attempt = 0; attempt < 5 && !opened; attempt++) {
        try {
          accessoryDev.open()
          opened = true
        } catch (err) {
          lastErr = err
          await new Promise((r) => setTimeout(r, 100))
        }
      }
      if (!opened) {
        throw new Error(
          `Failed to open AOAP accessory device: ${(lastErr as Error)?.message ?? 'unknown'}`
        )
      }
    } else {
      try {
        this._device.open()
      } catch (err) {
        throw new Error(`Failed to open AOAP device: ${(err as Error).message}`)
      }

      const reenumerated = waitForAccessoryAttach(AOAP_RE_ENUMERATE_TIMEOUT_MS)
      this._onWillReenumerate?.(AOAP_RE_ENUMERATE_TIMEOUT_MS + 2_000)
      await runAoapHandshake(this._device)

      try {
        this._device.close()
      } catch {
        /* ignore */
      }

      accessoryDev = await reenumerated
      accessoryDev.open()
    }

    const iface = accessoryDev.interface(0)
    if (!iface) {
      throw new Error('AOAP accessory: interface 0 missing')
    }

    // Same udev race as open(): immediately after enumeration libusb may
    // refuse claim() with "could not find matching udev device". Retry.
    let claimed = false
    let claimErr: unknown
    for (let attempt = 0; attempt < 5 && !claimed; attempt++) {
      try {
        iface.claim()
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

    let inEp: InEndpoint | null = null
    let outEp: OutEndpoint | null = null
    for (const ep of iface.endpoints) {
      if (ep.transferType !== USB_TRANSFER_TYPE_BULK) continue
      if ((ep.address & USB_ENDPOINT_IN) === USB_ENDPOINT_IN) {
        inEp = ep as InEndpoint
      } else {
        outEp = ep as OutEndpoint
      }
    }

    if (!inEp || !outEp) {
      throw new Error('AOAP accessory: bulk IN/OUT endpoints not found')
    }

    this._accessoryDevice = accessoryDev
    this._iface = iface
    this._inEp = inEp
    this._outEp = outEp
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
    if (!this._inEp || !this._outEp) {
      sock.destroy(new Error('AOAP bridge endpoints not initialised'))
      return
    }
    const inEp = this._inEp
    const outEp = this._outEp

    this._pumping = true

    // USB IN -> socket
    inEp.on('data', (chunk: Buffer) => {
      if (!this._pumping) return
      const ok = sock.write(chunk)
      if (!ok) {
        try {
          inEp.stopPoll()
        } catch {
          /* ignore */
        }
        sock.once('drain', () => {
          if (!this._pumping) return
          try {
            inEp.startPoll(2, BULK_READ_CHUNK)
          } catch {
            /* device may have been torn down */
          }
        })
      }
    })

    inEp.on('error', (err: Error) => {
      this.emit('error', err)
      try {
        sock.destroy(err)
      } catch {
        /* ignore */
      }
    })

    inEp.startPoll(2, BULK_READ_CHUNK)

    // socket -> USB OUT
    this._outChain = Promise.resolve()
    sock.on('data', (chunk: Buffer) => {
      if (!this._pumping) return
      this._outChain = this._outChain.then(
        () =>
          new Promise<void>((resolve) => {
            outEp.transfer(chunk, (err) => {
              if (err) {
                this.emit('error', err)
                try {
                  sock.destroy(err)
                } catch {
                  /* ignore */
                }
              }
              resolve()
            })
          })
      )
    })

    sock.once('close', () => {
      this._pumping = false
      try {
        inEp.stopPoll()
      } catch {
        /* ignore */
      }
      this._client = null
    })

    sock.once('error', (err) => {
      this.emit('error', err)
    })
  }

  get device(): Device {
    return this._device
  }
}

function waitForAccessoryAttach(timeoutMs: number): Promise<Device> {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => {
      usb.removeListener('attach', onAttach)
      reject(new Error('AOAP re-enumerate timeout'))
    }, timeoutMs)

    const onAttach = (dev: Device) => {
      const desc = dev.deviceDescriptor
      if (
        desc.idVendor === GOOGLE_VID &&
        (ACCESSORY_PIDS as readonly number[]).includes(desc.idProduct)
      ) {
        clearTimeout(t)
        usb.removeListener('attach', onAttach)
        resolve(dev)
      }
    }
    usb.on('attach', onAttach)
  })
}
