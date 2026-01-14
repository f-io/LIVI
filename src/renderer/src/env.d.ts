/// <reference types="@webgpu/types" />

import type { ElectronAPI } from '@electron-toolkit/preload'
import type { ExtraConfig } from '../../main/Globals'
import type { MultiTouchPoint } from '../../main/carplay/messages/sendable'

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
  type UpdateEvent =
    | {
        phase:
          | 'start'
          | 'ready'
          | 'mounting'
          | 'copying'
          | 'unmounting'
          | 'installing'
          | 'relaunching'
      }
    | { phase: 'error'; message?: string }

  type UpdateProgress = {
    phase: 'download'
    percent?: number
    received?: number
    total?: number
  }
}

type UsbDeviceInfo =
  | { device: false; vendorId: null; productId: null; usbFwVersion: string }
  | { device: true; vendorId: number; productId: number; usbFwVersion: string }

type MediaPayload = {
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
} | null

type DongleFirmwareAction = 'check' | 'download' | 'upload' | 'status'

type DongleFirmwareCheckResult =
  | {
      ok: true
      hasUpdate: boolean
      latestVer?: string
      notes?: string
      size?: number
      id?: string
      token?: string
      request?: {
        lang: number
        code: number
        appVer: string
        ver: string
        uuid: string
        mfd: string
        fwn: string
        model: string
      }
      raw: unknown
    }
  | { ok: false; error: string }

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
    electron: ElectronAPI

    carplay: {
      quit(): Promise<void>
      onUSBResetStatus(callback: (event: unknown, ...args: unknown[]) => void): void

      usb: {
        forceReset(): Promise<boolean>
        detectDongle(): Promise<boolean>
        getDeviceInfo(): Promise<UsbDeviceInfo>
        getLastEvent(): Promise<unknown>
        getSysdefaultPrettyName(): Promise<string>
        uploadIcons(): Promise<void>
        listenForEvents(callback: (event: unknown, ...args: unknown[]) => void): void
        unlistenForEvents(callback: (event: unknown, ...args: unknown[]) => void): void
      }

      settings: {
        get(): Promise<ExtraConfig>
        save(settings: ExtraConfig): Promise<void>
        onUpdate(callback: (event: unknown, settings: ExtraConfig) => void): void
      }

      ipc: {
        start(): Promise<void>
        stop(): Promise<void>
        sendFrame(): Promise<void>
        dongleFirmware(action: DongleFirmwareAction): Promise<DongleFirmwareCheckResult>
        sendTouch(x: number, y: number, action: number): void
        sendMultiTouch(points: MultiTouchPoint[]): void
        sendKeyCommand(key: string): void
        onEvent(callback: (event: unknown, ...args: unknown[]) => void): void
        offEvent(callback: (event: unknown, ...args: unknown[]) => void): void
        setVisualizerEnabled(enabled: boolean): void
        readMedia(): Promise<MediaPayload>
        onVideoChunk(handler: (payload: unknown) => void): void
        onAudioChunk(handler: (payload: unknown) => void): void
      }
    }

    app: {
      quitApp(): Promise<void>
      restartApp(): Promise<void>
      getVersion(): Promise<string>
      getLatestRelease(): Promise<{ version?: string; url?: string }>
      performUpdate(imageUrl?: string): Promise<void>
      onUpdateEvent(cb: (payload: UpdateEvent) => void): () => void
      onUpdateProgress(cb: (payload: UpdateProgress) => void): () => void
      getKiosk(): Promise<boolean>
      onKioskSync(cb: (kiosk: boolean) => void): () => void
      beginInstall(): Promise<void>
      abortUpdate(): Promise<void>
    }
  }
}

export {}
