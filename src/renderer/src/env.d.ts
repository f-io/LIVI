/// <reference types="@webgpu/types" />

declare module 'ringbuf.js' {
  export class RingBuffer {
    constructor(sab: SharedArrayBuffer, type: { BYTES_PER_ELEMENT: number })
    push(elements: ArrayBufferView, length?: number, offset?: number): number
    pop(elements: ArrayBufferView, length: number, offset?: number): number
    empty(): boolean
    full(): boolean
    capacity(): number
    availableRead(): number
  }
}

interface USBDevice {
  readonly productName?: string
  readonly manufacturerName?: string
  readonly serialNumber?: string
  readonly deviceVersionMajor?: number
  readonly deviceVersionMinor?: number
  readonly vendorId: number
  readonly productId: number
}

interface USBDeviceRequestOptions {
  filters?: Array<Partial<USBDevice>>
}

declare global {
  interface Navigator {
    usb: {
      getDevices(): Promise<USBDevice[]>
      requestDevice(options?: USBDeviceRequestOptions): Promise<USBDevice>
      addEventListener(type: 'connect' | 'disconnect', listener: (ev: Event) => void): void
      removeEventListener(type: 'connect' | 'disconnect', listener: (ev: Event) => void): void
    }
  }

  interface Window {
    carplay: {
      quit: () => Promise<any>
      onUSBResetStatus: (callback: (...args: any[]) => void) => void

      usb: {
        forceReset: () => Promise<any>
        detectDongle: () => Promise<any>
        getDeviceInfo: () => Promise<any>
        getLastEvent: () => Promise<any>
        getSysdefaultPrettyName: () => Promise<string>
        listenForEvents: (callback: (...args: any[]) => void) => void
        unlistenForEvents: (callback: (...args: any[]) => void) => void
      }

      settings: {
        get: () => Promise<any>
        save: (settings: any) => Promise<any>
        onUpdate: (callback: (...args: any[]) => void) => void
      }

      ipc: {
        start: () => Promise<any>
        stop: () => Promise<any>
        sendFrame: () => Promise<any>
        sendTouch: (x: number, y: number, action: number) => void
        sendMultiTouch: (points: any[]) => void
        sendKeyCommand: (key: string) => void
        onEvent: (callback: (...args: any[]) => void) => void

        readMedia: () => Promise<{
          timestamp: string
          payload: {
            type: number
            media?: {
              MediaSongName?: string
              MediaAlbumName?: string
              MediaArtistName?: string
              MediaAPPName?: string
              MediaSongDuration?: number
              MediaSongPlayTime?: number
              MediaPlayStatus?: number
              MediaLyrics?: string
            }
            base64Image?: string
          }
        } | null>

        onVideoChunk: (handler: (payload: any) => void) => void
        onAudioChunk: (handler: (payload: any) => void) => void
      }
    }

    app: {
      getVersion: () => Promise<string>
      getLatestRelease: () => Promise<any>
      performUpdate: (imageUrl?: string) => Promise<any>
      onUpdateEvent: (cb: (payload: any) => void) => () => void
      onUpdateProgress: (cb: (payload: any) => void) => () => void
      getKiosk: () => Promise<boolean>
      onKioskSync: (cb: (kiosk: boolean) => void) => () => void
    }
  }
}

export {}
