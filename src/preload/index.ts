import { contextBridge, ipcRenderer, IpcRendererEvent } from 'electron'
import type { ExtraConfig } from '../main/Globals'
import type { MultiTouchPoint } from '../main/carplay/messages/sendable'

type ApiCallback<T = any> = (event: IpcRendererEvent, ...args: T[]) => void

let usbEventQueue: [IpcRendererEvent, ...any[]][] = []
let usbEventHandlers: ApiCallback<any>[] = []

ipcRenderer.on('usb-event', (event, ...args) => {
  if (usbEventHandlers.length) {
    usbEventHandlers.forEach((h) => h(event, ...args))
  } else {
    usbEventQueue.push([event, ...args])
  }
})

type ChunkHandler = (payload: any) => void
let videoChunkQueue: any[] = []
let videoChunkHandler: ChunkHandler | null = null
let audioChunkQueue: any[] = []
let audioChunkHandler: ChunkHandler | null = null

ipcRenderer.on('carplay-video-chunk', (_event, payload) => {
  if (videoChunkHandler) videoChunkHandler(payload)
  else videoChunkQueue.push(payload)
})
ipcRenderer.on('carplay-audio-chunk', (_event, payload) => {
  if (audioChunkHandler) audioChunkHandler(payload)
  else audioChunkQueue.push(payload)
})

const api = {
  quit: () => ipcRenderer.invoke('quit'),

  onUSBResetStatus: (callback: ApiCallback<any>) => {
    ipcRenderer.on('usb-reset-start', callback)
    ipcRenderer.on('usb-reset-done', callback)
  },

  usb: {
    forceReset: () => ipcRenderer.invoke('usb-force-reset'),
    detectDongle: () => ipcRenderer.invoke('usb-detect-dongle'),
    getDeviceInfo: () => ipcRenderer.invoke('carplay:usbDevice'),
    getLastEvent: () => ipcRenderer.invoke('usb-last-event'),
    getSysdefaultPrettyName: () => ipcRenderer.invoke('get-sysdefault-mic-label'),
    listenForEvents: (callback: ApiCallback<any>) => {
      usbEventHandlers.push(callback)
      usbEventQueue.forEach(([evt, ...args]) => callback(evt, ...args))
      usbEventQueue = []
    },
    unlistenForEvents: (callback: ApiCallback<any>) => {
      usbEventHandlers = usbEventHandlers.filter((cb) => cb !== callback)
    }
  },

  settings: {
    get: (): Promise<ExtraConfig> => ipcRenderer.invoke('getSettings'),
    save: (settings: ExtraConfig) => ipcRenderer.invoke('save-settings', settings),
    onUpdate: (callback: ApiCallback<ExtraConfig>) => ipcRenderer.on('settings', callback)
  },

  ipc: {
    start: () => ipcRenderer.invoke('carplay-start'),
    stop: () => ipcRenderer.invoke('carplay-stop'),
    sendFrame: () => ipcRenderer.invoke('carplay-sendframe'),
    sendTouch: (x: number, y: number, action: number) =>
      ipcRenderer.send('carplay-touch', { x, y, action }),
    sendMultiTouch: (points: MultiTouchPoint[]) => ipcRenderer.send('carplay-multi-touch', points),
    sendKeyCommand: (key: string) => ipcRenderer.send('carplay-key-command', key),
    onEvent: (callback: ApiCallback<any>) => ipcRenderer.on('carplay-event', callback),
    readMedia: () => ipcRenderer.invoke('carplay-media-read'),

    onVideoChunk: (handler: ChunkHandler) => {
      videoChunkHandler = handler
      videoChunkQueue.forEach((chunk) => handler(chunk))
      videoChunkQueue = []
    },
    onAudioChunk: (handler: ChunkHandler) => {
      audioChunkHandler = handler
      audioChunkQueue.forEach((chunk) => handler(chunk))
      audioChunkQueue = []
    }
  }
}

contextBridge.exposeInMainWorld('carplay', api)

const appApi = {
  getVersion: () => ipcRenderer.invoke('app:getVersion'),
  getLatestRelease: () => ipcRenderer.invoke('app:getLatestRelease'),
  performUpdate: (imageUrl?: string) => ipcRenderer.invoke('app:performUpdate', imageUrl),

  onUpdateEvent: (cb: (payload: any) => void): (() => void) => {
    const ch = 'update:event'
    const handler = (_e: IpcRendererEvent, payload: any) => cb(payload)
    ipcRenderer.on(ch, handler)
    return () => ipcRenderer.removeListener(ch, handler)
  },
  onUpdateProgress: (cb: (payload: any) => void): (() => void) => {
    const ch = 'update:progress'
    const handler = (_e: IpcRendererEvent, payload: any) => cb(payload)
    ipcRenderer.on(ch, handler)
    return () => ipcRenderer.removeListener(ch, handler)
  },

  getKiosk: (): Promise<boolean> => ipcRenderer.invoke('settings:get-kiosk'),
  onKioskSync: (cb: (kiosk: boolean) => void): (() => void) => {
    const ch = 'settings:kiosk-sync'
    const handler = (_e: IpcRendererEvent, kiosk: boolean) => cb(kiosk)
    ipcRenderer.on(ch, handler)
    return () => ipcRenderer.removeListener(ch, handler)
  }
}

contextBridge.exposeInMainWorld('app', appApi)
