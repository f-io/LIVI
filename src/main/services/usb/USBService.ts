import { registerIpcHandle } from '@main/ipc/register'
import { Microphone } from '@main/services/audio'
import { BrowserWindow } from 'electron'
import { usb } from 'usb'
import { isAccessoryMode, probeAaCapable } from '../projection/driver/aa/stack/aoap/handshake.js'
import { ProjectionService } from '../projection/services/ProjectionService'
import { isCarlinkitDongle } from './constants'
import { findDongle } from './helpers'

type Device = USBDevice

const SKIP_PROBE_DEVICE_CLASSES = new Set<number>([
  0x03, // HID (keyboard, mouse, gamepad)
  0x07, // Printer
  0x08, // Mass Storage (USB stick)
  0x09, // Hub
  0x0e, // Video (UVC webcam)
  0x11 // Billboard (USB-C alt-mode advertising)
])

// Suppress detach/attach noise during the AOAP handshake cycle
const PHONE_REENUM_SUPPRESS_MS = 2_500

export class USBService {
  private lastDongleState: boolean = false
  private lastPhoneState: boolean = false
  private connectedPhoneDevice: Device | null = null
  private phoneSuspendUntil = 0
  private stopped = false
  private resetInProgress = false
  private shutdownInProgress = false
  private _onConnect: ((ev: USBConnectionEvent) => void) | null = null
  private _onDisconnect: ((ev: USBConnectionEvent) => void) | null = null

  public beginShutdown(): void {
    this.shutdownInProgress = true
  }

  public async stop(): Promise<void> {
    if (this.stopped) return
    this.stopped = true
    try {
      if (this._onConnect) usb.removeEventListener('connect', this._onConnect)
      if (this._onDisconnect) usb.removeEventListener('disconnect', this._onDisconnect)
    } catch {}
    this._onConnect = null
    this._onDisconnect = null
  }

  constructor(private projection: ProjectionService) {
    this.registerIpcHandlers()
    this.listenToUsbEvents()
    void this._init().catch((err) => {
      console.debug('[USBService] startup init threw', err)
    })
  }

  private async _init(): Promise<void> {
    const device = (await usb.getDevices()).find((d) => this.isDongle(d))
    if (device) {
      console.log('[USBService] Dongle was already connected on startup')
      this.lastDongleState = true
      this.projection.markDongleConnected(true)
      this.notifyDeviceChange(device, true)
    }

    await this._scanForExistingPhone()
  }

  private async _scanForExistingPhone(): Promise<void> {
    if (this.stopped || this.lastPhoneState) return

    const allDevices = await usb.getDevices()

    // Normal shutdown path resets the phone out of accessory mode
    const accessory = allDevices.find((d) => isAccessoryMode(d))
    if (accessory) {
      console.log('[USBService] Phone already in accessory mode at startup — claiming directly')
      this.markPhoneAttached(accessory)
      return
    }

    console.log(
      `[USBService] startup scan: ${allDevices.length} USB devices on bus: ${allDevices
        .map(
          (d) =>
            `vid=0x${d.vendorId?.toString(16) ?? '??'} pid=0x${d.productId?.toString(16) ?? '??'} cls=0x${d.deviceClass?.toString(16) ?? '??'}`
        )
        .join(', ')}`
    )
    const candidates = allDevices.filter((d) => this.isPhoneCandidate(d))
    if (candidates.length === 0) return
    console.log(`[USBService] Probing ${candidates.length} startup USB candidate(s) for AOAP`)
    for (const dev of candidates) {
      if (this.stopped || this.lastPhoneState) return
      const vid = dev.vendorId
      const pid = dev.productId
      try {
        // resetOnBusy: if a previous run left the interface exclusively held, force a clean
        // re-enumeration so the hotplug path re-probes a fresh handle (self-healing, no replug).
        const proto = await probeAaCapable(dev, { resetOnBusy: true })
        if (proto < 1) {
          console.log(
            `[USBService] startup probe: vid=0x${vid.toString(16)} pid=0x${pid.toString(16)} returned proto=${proto} — not AOAP-capable (phone locked / no USB confirmation?)`
          )
          continue
        }
        if (this.stopped || this.lastPhoneState) return
        console.log(
          `[USBService] AOAP-capable phone found on startup (vid=0x${vid.toString(16)}, pid=0x${pid.toString(16)}, proto=${proto})`
        )
        this.markPhoneAttached(dev)
        return
      } catch (err) {
        console.log(
          `[USBService] startup probe THREW for vid=0x${vid.toString(16)} pid=0x${pid.toString(16)}`,
          err
        )
      }
    }
  }

  // Inactive-transport USB events must not surface to the renderer.
  private shouldSuppressDongleEvents(): boolean {
    return this.projection.getActiveTransport() === 'aa'
  }

  private listenToUsbEvents() {
    const onConnect = (ev: USBConnectionEvent): void => {
      const device = ev.device as Device
      if (this.stopped || this.resetInProgress || this.shutdownInProgress) return
      const isDongleDev = this.isDongle(device)
      console.log(
        `[USBService] attach vid=0x${device.vendorId?.toString(16) ?? '??'} pid=0x${device.productId?.toString(16) ?? '??'} cls=0x${device.deviceClass?.toString(16) ?? '??'} → dongle=${isDongleDev} accessory=${isAccessoryMode(device)} phoneCandidate=${this.isPhoneCandidate(device)} lastPhone=${this.lastPhoneState}`
      )
      if (!(isDongleDev && this.shouldSuppressDongleEvents())) {
        this.broadcastGenericUsbEvent({ type: 'attach', device })
      }
      if (isDongleDev && !this.lastDongleState) {
        console.log('[USBService] Dongle connected')
        this.lastDongleState = true
        this.projection.markDongleConnected(true)
        if (!this.shouldSuppressDongleEvents()) {
          this.notifyDeviceChange(device, true)
        }
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
        if (this.lastPhoneState) {
          // Stale handle from a re-enumerated device (e.g. settings-restart reset()) — re-claim fresh.
          console.log('[USBService] Accessory re-attach outside reenum window — fresh device')
          this.markPhoneDetached(device)
        }
        console.log('[USBService] Phone connected (accessory mode)')
        this.markPhoneAttached(device)
        return
      }

      if (this.isPhoneCandidate(device)) {
        if (this.lastPhoneState) {
          console.log(
            '[USBService] OEM-PID phone re-attach while lastPhone=true — assuming stale state, resetting'
          )
          this.markPhoneDetached(device)
        }
        console.log(
          `[USBService] phone candidate detected — running AOAP probe vid=0x${device.vendorId?.toString(16)} pid=0x${device.productId?.toString(16)}`
        )
        this.tryProbePhone(device).catch((err) => {
          console.log('[USBService] AOAP probe threw', err)
        })
      }
    }

    const onDisconnect = (ev: USBConnectionEvent): void => {
      const device = ev.device as Device
      if (this.stopped || this.resetInProgress || this.shutdownInProgress) return
      const isDongleDev = this.isDongle(device)
      if (!(isDongleDev && this.shouldSuppressDongleEvents())) {
        this.broadcastGenericUsbEvent({ type: 'detach', device })
      }
      if (isDongleDev && this.lastDongleState) {
        console.log('[USBService] Dongle disconnected')
        this.lastDongleState = false
        this.projection.markDongleConnected(false)
        if (!this.shouldSuppressDongleEvents()) {
          this.notifyDeviceChange(device, false)
        }
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
    }

    this._onConnect = onConnect
    this._onDisconnect = onDisconnect
    usb.addEventListener('connect', onConnect)
    usb.addEventListener('disconnect', onDisconnect)
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
    const cls = device.deviceClass
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

    const vid = device.vendorId
    const pid = device.productId
    console.log(
      `[USBService] AOAP-capable phone detected (vid=0x${vid.toString(16)}, pid=0x${pid.toString(16)}, proto=${proto})`
    )
    this.markPhoneAttached(device)
  }

  private isSamePhoneDevice(device: Device): boolean {
    const cur = this.connectedPhoneDevice
    if (!cur) return false
    return device.vendorId === cur.vendorId && device.productId === cur.productId
  }

  private notifyDeviceChange(device: Device, connected: boolean): void {
    const vendorId = device.vendorId
    const productId = device.productId
    const payload = {
      type: connected ? 'plugged' : 'unplugged',
      device: { vendorId, productId, deviceName: '' }
    }
    BrowserWindow.getAllWindows().forEach((win) => {
      win.webContents.send('usb-event', payload)
    })
  }

  private broadcastGenericUsbEvent(event: { type: 'attach' | 'detach'; device: Device }) {
    const vendorId = event.device.vendorId
    const productId = event.device.productId
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
    })
  }

  private registerIpcHandlers() {
    registerIpcHandle('usb-detect-dongle', async () => {
      if (this.shutdownInProgress || this.resetInProgress) {
        return false
      }
      const devices = await usb.getDevices()
      return devices.some((d) => this.isDongle(d))
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

      const devices = await usb.getDevices()
      const detectDev = devices.find((d) => this.isDongle(d))
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
        const devices = await usb.getDevices()
        const dev = devices.find((d) => this.isDongle(d))
        if (dev) {
          return {
            type: 'plugged',
            device: {
              vendorId: dev.vendorId,
              productId: dev.productId,
              deviceName: ''
            }
          }
        }
      }

      // Direct-USB AA path: phone in accessory mode without a dongle.
      if (this.lastPhoneState && this.connectedPhoneDevice) {
        const dev = this.connectedPhoneDevice
        return {
          type: 'plugged',
          device: {
            vendorId: dev.vendorId,
            productId: dev.productId,
            deviceName: ''
          }
        }
      }

      return { type: 'unplugged', device: null }
    })

    registerIpcHandle('get-sysdefault-mic-label', () => Microphone.getSysdefaultPrettyName())
  }

  private getDongleUsbBasics(device: Device) {
    const major = device.deviceVersionMajor
    const lowByte = (device.deviceVersionMinor << 4) | device.deviceVersionSubminor
    const bcd = (major << 8) | lowByte
    const usbFwVersion = bcd ? `${major}.${lowByte.toString().padStart(2, '0')}` : 'Unknown'
    const vendorId = device.vendorId
    const productId = device.productId

    return {
      vendorId,
      productId,
      usbFwVersion
    }
  }

  private isDongle(device: Pick<Device, 'vendorId' | 'productId'>) {
    return isCarlinkitDongle(device.vendorId, device.productId)
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

      const dongle = await findDongle()
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
    let opened = false

    try {
      await dongle.open()
      opened = true
    } catch (openErr) {
      console.warn('[USB] Could not open device for reset:', openErr)
      return false
    }

    try {
      await dongle.reset()
      console.log('[USB] reset ok')
      return true
    } catch (err) {
      const msg = err instanceof Error ? err.message : typeof err === 'string' ? err : String(err)
      // A reset that drops the device off the bus is the intended outcome.
      if (/not[\s_-]?found|no[\s_-]?device|disconnect/i.test(msg)) {
        console.warn('[USB] reset triggered disconnect – treating as success')
        return true
      }
      console.error('[USB] reset error', err)
      return false
    } finally {
      if (opened) {
        try {
          await dongle.close()
        } catch (e) {
          console.warn('[USB] Failed to close dongle after reset:', e)
        }
      }
    }
  }
}
