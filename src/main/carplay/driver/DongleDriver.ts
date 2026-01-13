import EventEmitter from 'events'
import { MessageHeader, HeaderBuildError } from '../messages/common.js'
import {
  PhoneType,
  BoxInfo,
  SoftwareVersion,
  VendorCarPlaySessionBlob,
  type BoxInfoSettings
} from '../messages/readable.js'
import {
  SendableMessage,
  SendNumber,
  FileAddress,
  SendOpen,
  SendBoolean,
  SendBoxSettings,
  SendIconConfig,
  SendCommand,
  SendString,
  HeartBeat
} from '../messages/sendable.js'

const CONFIG_NUMBER = 1
const MAX_ERROR_COUNT = 5

export enum HandDriveType {
  LHD = 0,
  RHD = 1
}

export type PhoneTypeConfig = { frameInterval: number | null }
type PhoneTypeConfigMap = { [K in PhoneType]: PhoneTypeConfig }

export type DongleConfig = {
  androidWorkMode?: boolean
  width: number
  height: number
  fps: number
  dpi: number
  format: number
  iBoxVersion: number
  packetMax: number
  phoneWorkMode: number
  nightMode: boolean
  carName: string
  oemName: string
  hand: HandDriveType
  mediaDelay: number
  mediaSound: 0 | 1
  callQuality: 0 | 1 | 2
  autoPlay: boolean
  autoConn: boolean
  audioTransferMode: boolean
  wifiType: '2.4ghz' | '5ghz'
  wifiChannel: number
  micType: 'box' | 'os'
  phoneConfig: Partial<PhoneTypeConfigMap>
}

export const DEFAULT_CONFIG: DongleConfig = {
  width: 800,
  height: 480,
  fps: 60,
  dpi: 160,
  format: 5,
  iBoxVersion: 2,
  phoneWorkMode: 2,
  packetMax: 49152,
  carName: 'pi-carplay',
  oemName: 'pi-carplay',
  nightMode: true,
  hand: HandDriveType.LHD,
  mediaDelay: 1000,
  mediaSound: 1,
  callQuality: 1,
  autoPlay: true,
  autoConn: true,
  audioTransferMode: false,
  wifiType: '5ghz',
  wifiChannel: 36,
  micType: 'os',
  phoneConfig: {
    [PhoneType.CarPlay]: { frameInterval: 5000 },
    [PhoneType.AndroidAuto]: { frameInterval: null }
  }
}

export class DriverStateError extends Error {}

export class DongleDriver extends EventEmitter {
  private _heartbeatInterval: ReturnType<typeof setInterval> | null = null
  private _device: USBDevice | null = null
  private _inEP: USBEndpoint | null = null
  private _outEP: USBEndpoint | null = null
  private _ifaceNumber: number | null = null

  private errorCount = 0
  private _closing = false
  private _started = false
  private _readerActive = false
  private _closePromise: Promise<void> | null = null

  private _dongleFwVersion?: string
  private _boxInfo?: BoxInfoSettings
  private _lastDongleInfoEmitKey = ''

  static knownDevices = [
    { vendorId: 0x1314, productId: 0x1520 },
    { vendorId: 0x1314, productId: 0x1521 }
  ]

  private sleep(ms: number) {
    return new Promise<void>((r) => setTimeout(r, ms))
  }

  private async waitForReaderStop(timeoutMs = 1500) {
    const t0 = Date.now()
    while (this._readerActive && Date.now() - t0 < timeoutMs) {
      await this.sleep(10)
    }
  }

  /**
   * NOTE: This driver talks to the node-usb "WebUSB" compatibility layer.
   * It's still node-usb/libusb, but the device object follows the WebUSB-shaped API
   * (transferIn/transferOut returning Promises).
   * Pending transfers on macOS can block close() and may crash libusb/node-usb finalizers.
   */

  private isBenignUsbShutdownError(err: unknown): boolean {
    const msg = err instanceof Error ? err.message : String(err)

    // Typical macOS/libusb shutdown / unplug / reset fallout.
    return (
      msg.includes('LIBUSB_ERROR_NO_DEVICE') ||
      msg.includes('LIBUSB_ERROR_NOT_FOUND') ||
      msg.includes('LIBUSB_TRANSFER_NO_DEVICE') ||
      msg.includes('LIBUSB_TRANSFER_ERROR') ||
      msg.includes('transferIn error') ||
      msg.includes('device has been disconnected') ||
      msg.includes('No such device')
    )
  }

  private async tryResetUnderlyingUsbDevice(dev: USBDevice): Promise<boolean> {
    const raw =
      (dev as any)?.device ??
      (dev as any)?._device ??
      (dev as any)?.usbDevice ??
      (dev as any)?.rawDevice

    const resetFn = raw?.reset
    if (typeof resetFn !== 'function') return false

    try {
      await new Promise<void>((resolve, reject) => {
        resetFn.call(raw, (err: unknown) => (err ? reject(err) : resolve()))
      })
      return true
    } catch (e) {
      console.warn('[DongleDriver] underlying usb reset() failed', e)
      return false
    }
  }

  private emitDongleInfoIfChanged() {
    const fw = this._dongleFwVersion
    const box = this._boxInfo

    let boxKey = ''
    if (box != null) {
      try {
        boxKey = JSON.stringify(box)
      } catch {
        boxKey = String(box)
      }
    }

    const key = `${fw ?? ''}||${boxKey}`
    if (key === this._lastDongleInfoEmitKey) return
    this._lastDongleInfoEmitKey = key

    this.emit('dongle-info', { dongleFwVersion: fw, boxInfo: box })
  }

  initialise = async (device: USBDevice) => {
    if (this._device) return

    try {
      this._device = device
      if (!device.opened) throw new DriverStateError('Device not opened')

      await device.selectConfiguration(CONFIG_NUMBER)
      const cfg = device.configuration
      if (!cfg) throw new DriverStateError('Device has no configuration')

      const intf = cfg.interfaces[0]
      if (!intf) throw new DriverStateError('No interface 0')

      this._ifaceNumber = intf.interfaceNumber
      await device.claimInterface(this._ifaceNumber)

      const alt = intf.alternate
      if (!alt) throw new DriverStateError('No active alternate on interface')

      this._inEP = alt.endpoints.find((e) => e.direction === 'in') || null
      this._outEP = alt.endpoints.find((e) => e.direction === 'out') || null
      if (!this._inEP || !this._outEP) throw new DriverStateError('Endpoints missing')
    } catch (err) {
      await this.close()
      throw err
    }
  }

  send = async (msg: SendableMessage): Promise<boolean> => {
    const dev = this._device
    if (!dev || !dev.opened || this._closing) return false
    if (!this._outEP) return false

    try {
      const buf = msg.serialise()
      const view = new Uint8Array(buf.buffer as ArrayBuffer, buf.byteOffset, buf.byteLength)
      const res = await dev.transferOut(this._outEP.endpointNumber, view)
      return res.status === 'ok'
    } catch (err) {
      console.error('[DongleDriver] Send error', msg?.constructor?.name, err)
      return false
    }
  }

  private async readLoop() {
    if (this._readerActive) return
    this._readerActive = true

    try {
      while (this._device?.opened && !this._closing) {
        if (this.errorCount >= MAX_ERROR_COUNT) {
          await this.close()
          this.emit('failure')
          return
        }

        try {
          const dev = this._device
          const inEp = this._inEP
          if (!dev || !inEp) break

          const headerRes = await dev.transferIn(inEp.endpointNumber, MessageHeader.dataLength)
          if (this._closing) break

          const headerBuf = headerRes?.data?.buffer
          if (!headerBuf) throw new HeaderBuildError('Empty header')

          const header = MessageHeader.fromBuffer(Buffer.from(headerBuf))
          let extra: Buffer | undefined

          if (header.length) {
            const extraRes = await dev.transferIn(inEp.endpointNumber, header.length)
            if (this._closing) break
            const extraBuf = extraRes?.data?.buffer
            if (!extraBuf) throw new Error('Failed to read extra data')
            extra = Buffer.from(extraBuf)
          }

          const msg = header.toMessage(extra)
          if (msg) {
            if (msg instanceof VendorCarPlaySessionBlob) {
              console.debug(
                `[DongleDriver] vendor blob 0x${msg.header.type.toString(16)} len=${msg.raw.length}`
              )
              //this.emit('vendor-opaque', { type: msg.header.type, len: msg.raw.length })
              continue
            }
            this.emit('message', msg)

            if (msg instanceof SoftwareVersion) {
              this._dongleFwVersion = msg.version
              this.emitDongleInfoIfChanged()
            } else if (msg instanceof BoxInfo) {
              this._boxInfo = msg.settings
              this.emitDongleInfoIfChanged()
            }

            if (this.errorCount !== 0) this.errorCount = 0
          }
        } catch (err) {
          if (this._closing || !this._device?.opened || this.isBenignUsbShutdownError(err)) {
            break
          }

          console.error('[DongleDriver] readLoop error', err)
          this.errorCount++
        }
      }
    } finally {
      this._readerActive = false
    }
  }

  start = async (cfg: DongleConfig) => {
    if (!this._device) throw new DriverStateError('initialise() first')
    if (!this._device.opened) return
    if (this._started) return

    this.errorCount = 0
    this._started = true

    if (!this._readerActive) void this.readLoop()

    const ui = (cfg.oemName ?? '').trim()
    const label = ui.length > 0 ? ui : cfg.carName

    const messages: SendableMessage[] = [
      new SendNumber(cfg.dpi, FileAddress.DPI),
      new SendOpen(cfg),
      new SendBoolean(cfg.nightMode, FileAddress.NIGHT_MODE),
      new SendNumber(cfg.hand, FileAddress.HAND_DRIVE_MODE),
      new SendString(label, FileAddress.BOX_NAME),
      new SendIconConfig({ oemName: cfg.oemName }),
      new SendBoolean(true, FileAddress.CHARGE_MODE),
      new SendCommand(cfg.wifiType === '5ghz' ? 'wifi5g' : 'wifi24g'),
      new SendBoxSettings(cfg),
      new SendCommand('wifiEnable'),
      new SendCommand(cfg.micType === 'box' ? 'boxMic' : 'mic'),
      new SendCommand(cfg.audioTransferMode ? 'audioTransferOn' : 'audioTransferOff')
    ]
    if (cfg.androidWorkMode)
      messages.push(new SendBoolean(cfg.androidWorkMode, FileAddress.ANDROID_WORK_MODE))

    for (const m of messages) {
      await this.send(m)
      await this.sleep(120)
    }

    setTimeout(() => void this.send(new SendCommand('wifiConnect')), 600)

    if (this._heartbeatInterval) clearInterval(this._heartbeatInterval)
    this._heartbeatInterval = setInterval(() => void this.send(new HeartBeat()), 2000)
  }

  close = async (): Promise<void> => {
    // Serialize close() calls
    if (this._closePromise) return this._closePromise

    this._closePromise = (async () => {
      // Nothing to do?
      if (!this._device && !this._readerActive && !this._started) return

      this._closing = true

      if (this._heartbeatInterval) {
        clearInterval(this._heartbeatInterval)
        this._heartbeatInterval = null
      }

      const dev = this._device
      const iface = this._ifaceNumber

      // If we end up in the "pending request" situation, we may intentionally keep the device ref.
      let keepDeviceRefToAvoidGcFinalizerCrash = false

      try {
        if (dev && dev.opened) {
          // Best effort: abort pending transferIn on macOS
          try {
            if (process.platform === 'darwin') {
              await dev.reset()
            }
          } catch (e) {
            const msg = e instanceof Error ? e.message : String(e)
            if (
              !msg.includes('LIBUSB_ERROR_NOT_FOUND') &&
              !msg.includes('LIBUSB_ERROR_NO_DEVICE')
            ) {
              console.warn('[DongleDriver] device.reset() failed (ignored)', e)
            }
          }

          // Give readLoop a moment to unwind after reset
          await this.waitForReaderStop(1500)

          if (iface != null) {
            try {
              await dev.releaseInterface(iface)
            } catch (e) {
              console.warn('[DongleDriver] releaseInterface() failed (ignored)', e)
            }
          }

          try {
            await dev.close()
          } catch (e: unknown) {
            const msg = e instanceof Error ? e.message : String(e)

            if (/pending request/i.test(msg)) {
              console.warn(
                '[DongleDriver] device.close(): pending request -> trying underlying usb reset()'
              )

              // Try to cancel libusb I/O at the raw level
              const resetOk = await this.tryResetUnderlyingUsbDevice(dev)
              if (resetOk) {
                await this.sleep(50)
                await this.waitForReaderStop(1500)
              }

              // Try close once more (best-effort)
              try {
                await dev.close()
              } catch (e2: unknown) {
                const msg2 = e2 instanceof Error ? e2.message : String(e2)
                if (/pending request/i.test(msg2)) {
                  console.warn(
                    '[DongleDriver] device.close(): pending request did not resolve before deadline'
                  )
                  // Intentionally keep reference: avoids GC finalizer calling libusb_close later.
                  keepDeviceRefToAvoidGcFinalizerCrash = true
                } else {
                  console.warn('[DongleDriver] device.close() failed', e2)
                }
              }
            } else {
              console.warn('[DongleDriver] device.close() failed', e)
            }
          }
        }
      } catch (err) {
        console.warn('[DongleDriver] close() outer error', err)
      } finally {
        // Always reset logical state
        this._heartbeatInterval = null
        this._inEP = null
        this._outEP = null
        this._ifaceNumber = null
        this._started = false
        this._readerActive = false
        this.errorCount = 0

        this._dongleFwVersion = undefined
        this._boxInfo = undefined
        this._lastDongleInfoEmitKey = ''

        // Only clear the device ref if we successfully closed OR we are sure it won't crash later.
        if (!keepDeviceRefToAvoidGcFinalizerCrash) {
          this._device = null
        }

        this._closing = false
      }
    })().finally(() => {
      this._closePromise = null
    })

    return this._closePromise
  }
}
