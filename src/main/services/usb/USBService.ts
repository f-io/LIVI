import { registerIpcHandle } from '@main/ipc/register'
import { Microphone } from '@main/services/audio'
import { BrowserWindow } from 'electron'
import { type Device, usb } from 'usb'
import { isAccessoryMode, probeAaCapable } from '../projection/driver/aa/stack/aoap/handshake.js'
import { ProjectionService } from '../projection/services/ProjectionService'
import { findDongle } from './helpers'

const getDeviceList = () => usb.getDeviceList()

const SKIP_PROBE_DEVICE_CLASSES = new Set<number>([
  0x03, // HID (keyboard, mouse, gamepad)
  0x07, // Printer
  0x08, // Mass Storage (USB stick)
  0x09, // Hub
  0x0e, // Video (UVC webcam)
  0x11 // Billboard (USB-C alt-mode advertising)
])

// Suppress detach/attach noise during the AOAP handshake cycle.
const PHONE_REENUM_SUPPRESS_MS = 2_500

export class USBService {
  private lastDongleState: boolean = false
  private lastPhoneState: boolean = false
  private connectedPhoneDevice: Device | null = null
  private phoneSuspendUntil = 0
  private stopped = false
  private resetInProgress = false
  private shutdownInProgress = false

  public beginShutdown(): void {
    this.shutdownInProgress = true
  }

  public async stop(): Promise<void> {
    if (this.stopped) return
    this.stopped = true
    try {
      usb.removeAllListeners('attach')
    } catch {}
    try {
      usb.removeAllListeners('detach')
    } catch {}
  }

  constructor(private projection: ProjectionService) {
    this.registerIpcHandlers()
    this.listenToUsbEvents()
    try {
      if (process.platform !== 'darwin') usb.unrefHotplugEvents()
    } catch {}

    const device = getDeviceList().find(this.isDongle)
    if (device) {
      console.log('[USBService] Dongle was already connected on startup')
      this.lastDongleState = true
      this.projection.markDongleConnected(true)
      this.notifyDeviceChange(device, true)
      this.projection.autoStartIfNeeded().catch(console.error)
    }

    void this._scanForExistingPhone().catch((err) => {
      console.debug('[USBService] startup phone scan threw', err)
    })
  }

  private async _scanForExistingPhone(): Promise<void> {
    if (this.stopped || this.lastPhoneState) return

    // Normal shutdown path resets the phone out of accessory mode
    const accessory = getDeviceList().find((d) => isAccessoryMode(d))
    if (accessory) {
      console.log('[USBService] Phone already in accessory mode at startup — claiming directly')
      this.markPhoneAttached(accessory)
      return
    }

    const candidates = getDeviceList().filter((d) => this.isPhoneCandidate(d))
    if (candidates.length === 0) return
    console.log(`[USBService] Probing ${candidates.length} startup USB candidate(s) for AOAP`)
    for (const dev of candidates) {
      if (this.stopped || this.lastPhoneState) return
      try {
        const proto = await probeAaCapable(dev)
        if (proto < 1) continue
        if (this.stopped || this.lastPhoneState) return
        const vid = dev.deviceDescriptor.idVendor
        const pid = dev.deviceDescriptor.idProduct
        console.log(
          `[USBService] AOAP-capable phone found on startup (vid=0x${vid.toString(16)}, pid=0x${pid.toString(16)}, proto=${proto})`
        )
        this.markPhoneAttached(dev)
        return
      } catch (err) {
        console.debug(
          `[USBService] startup probe threw for vid=0x${dev.deviceDescriptor.idVendor.toString(16)}`,
          err
        )
      }
    }
  }

  private listenToUsbEvents() {
    usb.on('attach', (device) => {
      if (this.stopped || this.resetInProgress || this.shutdownInProgress) return
      this.broadcastGenericUsbEvent({ type: 'attach', device })
      if (this.isDongle(device) && !this.lastDongleState) {
        console.log('[USBService] Dongle connected')
        this.lastDongleState = true
        this.projection.markDongleConnected(true)
        this.notifyDeviceChange(device, true)
        this.projection.autoStartIfNeeded().catch(console.error)
        return
      }

      // Post-handshake fast path: phone already enumerated as an accessory.
      if (isAccessoryMode(device)) {
        const inSuspend = this.lastPhoneState && this.isPhoneSuspendWindow()
        const expectingReenum =
          this.lastPhoneState && this.projection.isExpectingPhoneReenumeration()
        if (inSuspend || expectingReenum) {
          console.log(
            `[USBService] Accessory-mode re-attach during re-enumeration window — bridge owns it (${inSuspend ? 'suspend' : 'reset'})`
          )
          this.connectedPhoneDevice = device
          this.projection.markPhoneConnected(true, device)
          return
        }
        if (!this.lastPhoneState) {
          console.log('[USBService] Phone connected (accessory mode)')
          this.markPhoneAttached(device)
        }
        return
      }

      if (!this.lastPhoneState && this.isPhoneCandidate(device)) {
        this.tryProbePhone(device).catch((err) => {
          console.debug('[USBService] AOAP probe threw', err)
        })
      }
    })

    usb.on('detach', (device) => {
      if (this.stopped || this.resetInProgress || this.shutdownInProgress) return
      this.broadcastGenericUsbEvent({ type: 'detach', device })
      if (this.isDongle(device) && this.lastDongleState) {
        console.log('[USBService] Dongle disconnected')
        this.lastDongleState = false
        this.projection.markDongleConnected(false)
        this.notifyDeviceChange(device, false)
        return
      }

      if (this.lastPhoneState && this.isSamePhoneDevice(device)) {
        if (this.isPhoneSuspendWindow() || this.projection.isExpectingPhoneReenumeration()) {
          // Either the AOAP handshake or a bridge-driven bus reset
          console.log('[USBService] Phone detach during re-enumeration window — suppressed')
          return
        }
        console.log('[USBService] Phone disconnected')
        this.markPhoneDetached(device)
      }
    })
  }

  private isPhoneSuspendWindow(): boolean {
    return Date.now() < this.phoneSuspendUntil
  }

  private markPhoneAttached(device: Device): void {
    this.lastPhoneState = true
    this.connectedPhoneDevice = device
    this.phoneSuspendUntil = Date.now() + PHONE_REENUM_SUPPRESS_MS
    this.projection.markPhoneConnected(true, device)
  }

  private markPhoneDetached(_device: Device): void {
    this.lastPhoneState = false
    this.connectedPhoneDevice = null
    this.phoneSuspendUntil = 0
    this.projection.markPhoneConnected(false)
  }

  private isPhoneCandidate(device: Device): boolean {
    if (this.isDongle(device)) return false
    const cls = device.deviceDescriptor?.bDeviceClass
    if (cls === undefined) return false
    if (SKIP_PROBE_DEVICE_CLASSES.has(cls)) return false
    return cls === 0x00 || cls === 0xff
  }

  private async tryProbePhone(device: Device): Promise<void> {
    // Skip if state changed while waiting on the event loop.
    if (this.stopped || this.lastPhoneState) return
    const proto = await probeAaCapable(device)
    if (proto < 1) return
    if (this.stopped || this.lastPhoneState) return

    const vid = device.deviceDescriptor.idVendor
    const pid = device.deviceDescriptor.idProduct
    console.log(
      `[USBService] AOAP-capable phone detected (vid=0x${vid.toString(16)}, pid=0x${pid.toString(16)}, proto=${proto})`
    )
    this.markPhoneAttached(device)
  }

  private isSamePhoneDevice(device: Device): boolean {
    const cur = this.connectedPhoneDevice
    if (!cur) return false
    const a = device.deviceDescriptor
    const b = cur.deviceDescriptor
    return a.idVendor === b.idVendor && a.idProduct === b.idProduct
  }

  private notifyDeviceChange(device: Device, connected: boolean): void {
    const vendorId = device.deviceDescriptor.idVendor
    const productId = device.deviceDescriptor.idProduct
    const payload = {
      type: connected ? 'plugged' : 'unplugged',
      device: { vendorId, productId, deviceName: '' }
    }
    BrowserWindow.getAllWindows().forEach((win) => {
      win.webContents.send('usb-event', payload)
      win.webContents.send('projection-event', payload)
    })
  }

  private broadcastGenericUsbEvent(event: { type: 'attach' | 'detach'; device: Device }) {
    const vendorId = event.device.deviceDescriptor.idVendor
    const productId = event.device.deviceDescriptor.idProduct
    const payload = {
      type: event.type,
      device: { vendorId, productId, deviceName: '' }
    }
    BrowserWindow.getAllWindows().forEach((win) => win.webContents.send('usb-event', payload))
  }

  private broadcastGenericUsbEventNoDevice(type: 'attach' | 'detach') {
    const payload = {
      type,
      device: { vendorId: null, productId: null, deviceName: '' }
    }
    BrowserWindow.getAllWindows().forEach((win) => win.webContents.send('usb-event', payload))
  }

  private notifyDeviceChangeNoDevice(connected: boolean): void {
    const payload = {
      type: connected ? 'plugged' : 'unplugged',
      device: { vendorId: null, productId: null, deviceName: '' }
    }
    BrowserWindow.getAllWindows().forEach((win) => {
      win.webContents.send('usb-event', payload)
      win.webContents.send('projection-event', payload)
    })
  }

  private registerIpcHandlers() {
    registerIpcHandle('usb-detect-dongle', async () => {
      if (this.shutdownInProgress || this.resetInProgress) {
        return false
      }
      const devices = getDeviceList()
      return devices.some(this.isDongle)
    })

    registerIpcHandle('projection:usbDevice', async () => {
      if (this.shutdownInProgress || this.resetInProgress) {
        return {
          device: false,
          vendorId: null,
          productId: null,
          usbFwVersion: 'Unknown'
        }
      }

      const devices = getDeviceList()
      const detectDev = devices.find(this.isDongle)
      if (!detectDev) {
        return {
          device: false,
          vendorId: null,
          productId: null,
          usbFwVersion: 'Unknown'
        }
      }

      const info = this.getDongleUsbBasics(detectDev)

      return {
        device: true,
        vendorId: info.vendorId,
        productId: info.productId,
        usbFwVersion: info.usbFwVersion
      }
    })

    registerIpcHandle('usb-force-reset', async () => {
      if (this.shutdownInProgress) {
        console.log('[USBService] usb-force-reset ignored: shutting down')
        return false
      }
      if (this.resetInProgress) {
        console.log('[USBService] usb-force-reset ignored: reset already in progress')
        return false
      }

      return this.forceReset()
    })

    registerIpcHandle('usb-last-event', async () => {
      if (this.shutdownInProgress || this.resetInProgress) {
        return { type: 'unplugged', device: null }
      }

      if (this.lastDongleState) {
        const devices = getDeviceList()
        const dev = devices.find(this.isDongle)
        if (dev) {
          return {
            type: 'plugged',
            device: {
              vendorId: dev.deviceDescriptor.idVendor,
              productId: dev.deviceDescriptor.idProduct,
              deviceName: ''
            }
          }
        }
      }
      return { type: 'unplugged', device: null }
    })

    registerIpcHandle('get-sysdefault-mic-label', () => Microphone.getSysdefaultPrettyName())
  }

  private getDongleUsbBasics(device: Device) {
    const usbFwVersion = device.deviceDescriptor.bcdDevice
      ? `${device.deviceDescriptor.bcdDevice >> 8}.${(device.deviceDescriptor.bcdDevice & 0xff)
          .toString()
          .padStart(2, '0')}`
      : 'Unknown'
    const vendorId = device.deviceDescriptor.idVendor
    const productId = device.deviceDescriptor.idProduct

    return {
      vendorId,
      productId,
      usbFwVersion
    }
  }

  private isDongle(
    device: Partial<Device> & { deviceDescriptor?: { idVendor: number; idProduct: number } }
  ) {
    return (
      device.deviceDescriptor?.idVendor === 0x1314 &&
      [0x1520, 0x1521].includes(device.deviceDescriptor?.idProduct ?? -1)
    )
  }

  private notifyReset(type: 'usb-reset-start' | 'usb-reset-done', ok: boolean) {
    BrowserWindow.getAllWindows().forEach((win) => win.webContents.send(type, ok))
  }

  public async forceReset(): Promise<boolean> {
    if (this.shutdownInProgress) return false
    if (this.resetInProgress) return false

    this.resetInProgress = true
    this.notifyReset('usb-reset-start', true)

    let ok = false
    try {
      // Stop projection first (clears pending transfers)
      try {
        await this.projection.stop()
      } catch (e) {
        console.warn('[USB] projection.stop() failed before reset', e)
      }

      if (this.shutdownInProgress) return false

      const dongle = findDongle()
      if (!dongle) {
        console.warn('[USB] Dongle not found')
        this.lastDongleState = false
        this.broadcastGenericUsbEventNoDevice('detach')
        this.notifyDeviceChangeNoDevice(false)
        ok = false
        return ok
      }

      this.lastDongleState = false
      this.broadcastGenericUsbEvent({ type: 'detach', device: dongle })
      this.notifyDeviceChange(dongle, false)

      ok = await this.resetDongle(dongle)
      return ok
    } catch (e) {
      console.error('[USB] forceReset exception', e)
      ok = false
      return ok
    } finally {
      this.notifyReset('usb-reset-done', ok)
      await new Promise<void>((r) => setTimeout(r, 200))
      this.resetInProgress = false
    }
  }

  public async gracefulReset(): Promise<boolean> {
    this.notifyReset('usb-reset-start', true)

    this.resetInProgress = true
    try {
      console.log('[USB] Graceful disconnect: stopping projection')
      await this.projection.stop()

      this.lastDongleState = false
      this.broadcastGenericUsbEventNoDevice('detach')
      this.notifyDeviceChangeNoDevice(false)

      this.notifyReset('usb-reset-done', true)
      return true
    } catch (e) {
      console.error('[USB] Exception during graceful disconnect', e)
      this.notifyReset('usb-reset-done', false)
      return false
    } finally {
      await new Promise((resolve) => setTimeout(resolve, 400))
      this.resetInProgress = false
    }
  }

  private async resetDongle(dongle: Device): Promise<boolean> {
    let opened: boolean

    try {
      dongle.open()
      opened = true
    } catch (openErr) {
      console.warn('[USB] Could not open device for reset:', openErr)
      return false
    }

    try {
      await new Promise<void>((resolve, reject) => {
        dongle.reset((err?: unknown) => {
          if (!err) {
            console.log('[USB] reset ok')
            resolve()
            return
          }

          const msg =
            err instanceof Error ? err.message : typeof err === 'string' ? err : String(err)

          if (
            msg.includes('LIBUSB_ERROR_NOT_FOUND') ||
            msg.includes('LIBUSB_ERROR_NO_DEVICE') ||
            msg.includes('LIBUSB_TRANSFER_NO_DEVICE')
          ) {
            console.warn('[USB] reset triggered disconnect – treating as success')
            resolve()
            return
          }

          console.error('[USB] reset error', err)
          reject(new Error('Reset failed'))
        })
      })

      return true
    } catch (e) {
      console.error('[USB] Exception during resetDongle()', e)
      return false
    } finally {
      if (opened) {
        try {
          dongle.close()
        } catch (e) {
          console.warn('[USB] Failed to close dongle after reset:', e)
        }
      }
    }
  }
}
