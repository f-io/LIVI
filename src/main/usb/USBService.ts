import type { Device } from 'usb'
import { ipcMain, BrowserWindow } from 'electron'
import { CarplayService } from '../carplay/services/CarplayService'
import { findDongle } from './helpers'
import { Microphone } from '@audio'

import * as usbModule from 'usb'
const { usb, getDeviceList } = usbModule

export class USBService {
  private lastDongleState: boolean = false
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

  constructor(private carplay: CarplayService) {
    this.registerIpcHandlers()
    this.listenToUsbEvents()
    try {
      if (process.platform !== 'darwin') usb.unrefHotplugEvents()
    } catch {}

    const device = getDeviceList().find(this.isDongle)
    if (device) {
      console.log('[USBService] Dongle was already connected on startup', device)
      this.lastDongleState = true
      this.carplay.markDongleConnected(true)
      this.notifyDeviceChange(device, true)
      this.carplay.autoStartIfNeeded().catch(console.error)
    }
  }

  private listenToUsbEvents() {
    usb.on('attach', (device) => {
      if (this.stopped || this.resetInProgress || this.shutdownInProgress) return
      this.broadcastGenericUsbEvent({ type: 'attach', device })
      if (this.isDongle(device) && !this.lastDongleState) {
        console.log('[USBService] Dongle connected:', device)
        this.lastDongleState = true
        this.carplay.markDongleConnected(true)
        this.notifyDeviceChange(device, true)
        this.carplay.autoStartIfNeeded().catch(console.error)
      }
    })

    usb.on('detach', (device) => {
      if (this.stopped || this.resetInProgress || this.shutdownInProgress) return
      this.broadcastGenericUsbEvent({ type: 'detach', device })
      if (this.isDongle(device) && this.lastDongleState) {
        console.log('[USBService] Dongle disconnected:', device)
        this.lastDongleState = false
        this.carplay.markDongleConnected(false)
        this.notifyDeviceChange(device, false)
      }
    })
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
      win.webContents.send('carplay-event', payload)
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
      win.webContents.send('carplay-event', payload)
    })
  }

  private registerIpcHandlers() {
    ipcMain.handle('usb-detect-dongle', async () => {
      if (this.shutdownInProgress || this.resetInProgress) {
        return false
      }
      const devices = getDeviceList()
      return devices.some(this.isDongle)
    })

    ipcMain.handle('carplay:usbDevice', async () => {
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

    ipcMain.handle('usb-force-reset', async () => {
      if (this.shutdownInProgress) {
        console.log('[USBService] usb-force-reset ignored: shutting down')
        return false
      }
      if (this.resetInProgress) {
        console.log('[USBService] usb-force-reset ignored: reset already in progress')
        return false
      }

      if (process.platform === 'darwin') {
        console.log('[USBService] macOS detected – using graceful reset')
        return this.gracefulReset()
      }

      return this.forceReset()
    })

    ipcMain.handle('usb-last-event', async () => {
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

    ipcMain.handle('get-sysdefault-mic-label', () => Microphone.getSysdefaultPrettyName())
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
      // Stop CarPlay first (clears pending transfers)
      try {
        await this.carplay.stop()
      } catch (e) {
        console.warn('[USB] carplay.stop() failed before reset', e)
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
      console.log('[USB] Graceful disconnect: stopping CarPlay')
      await this.carplay.stop()

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
