import type { Config } from '@shared/types'
import type { MultiTouchPoint } from '@shared/types/TouchTypes'
import { contextBridge, IpcRendererEvent, ipcRenderer } from 'electron'

type ApiCallback<TArgs extends unknown[] = unknown[]> = (
  event: IpcRendererEvent,
  ...args: TArgs
) => void

let usbEventQueue: Array<[IpcRendererEvent, ...unknown[]]> = []
let usbEventHandlers: Array<ApiCallback> = []

ipcRenderer.on('usb-event', (event, ...args: unknown[]) => {
  if (usbEventHandlers.length) {
    usbEventHandlers.forEach((h) => h(event, ...args))
  } else {
    usbEventQueue.push([event, ...args])
  }
})

type ChunkHandler = (payload: unknown) => void
let audioChunkQueue: unknown[] = []
let audioChunkHandler: ChunkHandler | null = null

let lastClusterResolution: unknown = null
let clusterResolutionHandlers: ChunkHandler[] = []

type TelemetryHandler = (payload: unknown) => void
let telemetryQueue: unknown[] = []
let telemetryHandlers: TelemetryHandler[] = []

let projectionEventQueue: Array<[IpcRendererEvent, ...unknown[]]> = []
let projectionEventHandlers: Array<ApiCallback> = []

ipcRenderer.on('projection-audio-chunk', (_event, payload: unknown) => {
  if (audioChunkHandler) audioChunkHandler(payload)
  else {
    // a window that never shows the visualizer would buffer forever; keep it bounded
    audioChunkQueue.push(payload)
    if (audioChunkQueue.length > 32) audioChunkQueue.shift()
  }
})

ipcRenderer.on('cluster-video-resolution', (_event, payload: unknown) => {
  lastClusterResolution = payload
  for (const h of clusterResolutionHandlers) h(payload)
})

ipcRenderer.on('telemetry:update', (_event, payload: unknown) => {
  if (telemetryHandlers.length) {
    telemetryHandlers.forEach((handler) => handler(payload))
  } else {
    telemetryQueue.push(payload)
  }
})

ipcRenderer.on('projection-event', (event, ...args: unknown[]) => {
  if (projectionEventHandlers.length) {
    projectionEventHandlers.forEach((handler) => handler(event, ...args))
  } else {
    projectionEventQueue.push([event, ...args])
  }
})

// Main broadcasts media key events
type MediaKeyHandler = (command: string) => void
const mediaKeyHandlers: MediaKeyHandler[] = []
let mediaKeyQueue: string[] = []

ipcRenderer.on('app:media-key', (_event, command: unknown) => {
  if (typeof command !== 'string' || !command) return
  if (mediaKeyHandlers.length) {
    mediaKeyHandlers.forEach((h) => h(command))
  } else {
    mediaKeyQueue.push(command)
  }
})

type UsbDeviceInfo =
  | { device: false; vendorId: null; productId: null; usbFwVersion: string }
  | { device: true; vendorId: number; productId: number; usbFwVersion: string }

type UsbLastEvent =
  | { type: 'unplugged'; device: null }
  | { type: 'plugged'; device: { vendorId: number; productId: number; deviceName: string } }

const api = {
  quit: (): Promise<void> => ipcRenderer.invoke('quit'),

  onUSBResetStatus: (callback: ApiCallback): (() => void) => {
    const s = 'usb-reset-start'
    const d = 'usb-reset-done'
    ipcRenderer.on(s, callback)
    ipcRenderer.on(d, callback)
    return () => {
      ipcRenderer.removeListener(s, callback)
      ipcRenderer.removeListener(d, callback)
    }
  },

  usb: {
    forceReset: (): Promise<boolean> => ipcRenderer.invoke('usb-force-reset'),
    detectDongle: (): Promise<boolean> => ipcRenderer.invoke('usb-detect-dongle'),
    getDeviceInfo: (): Promise<UsbDeviceInfo> => ipcRenderer.invoke('projection:usbDevice'),
    getLastEvent: (): Promise<UsbLastEvent> => ipcRenderer.invoke('usb-last-event'),
    getSysdefaultPrettyName: (): Promise<string> => ipcRenderer.invoke('get-sysdefault-mic-label'),
    uploadIcons: () => ipcRenderer.invoke('projection-upload-icons'),
    uploadLiviScripts: () => ipcRenderer.invoke('projection-upload-livi-scripts'),
    listenForEvents: (callback: ApiCallback): (() => void) => {
      usbEventHandlers.push(callback)
      usbEventQueue.forEach(([evt, ...args]) => callback(evt, ...args))
      usbEventQueue = []
      return () => {
        usbEventHandlers = usbEventHandlers.filter((cb) => cb !== callback)
      }
    }
  },

  settings: {
    get: (): Promise<Config> => ipcRenderer.invoke('getSettings'),
    save: (settings: Partial<Config>): Promise<void> =>
      ipcRenderer.invoke('save-settings', settings),
    onUpdate: (callback: ApiCallback<[Config]>): (() => void) => {
      const ch = 'settings'
      ipcRenderer.on(ch, callback)
      return () => ipcRenderer.removeListener(ch, callback)
    }
  },

  audio: {
    listSinks: (): Promise<
      Array<{ id: string; name: string; isDefault: boolean; offline?: boolean }>
    > => ipcRenderer.invoke('audio:listSinks'),
    listSources: (): Promise<
      Array<{ id: string; name: string; isDefault: boolean; offline?: boolean }>
    > => ipcRenderer.invoke('audio:listSources')
  },

  ipc: {
    start: (): Promise<void> => ipcRenderer.invoke('projection-start'),
    stop: (): Promise<void> => ipcRenderer.invoke('projection-stop'),
    restart: (): Promise<void> => ipcRenderer.invoke('projection-restart'),
    setVisible: (visible: boolean): Promise<void> =>
      ipcRenderer.invoke('projection-set-visible', visible),
    sendFrame: (): Promise<void> => ipcRenderer.invoke('projection-sendframe'),
    setBluetoothPairedList: (listText: string): Promise<{ ok: boolean }> =>
      ipcRenderer.invoke('projection-bt-pairedlist-set', listText),
    connectBluetoothPairedDevice: (mac: string): Promise<{ ok: boolean }> =>
      ipcRenderer.invoke('projection-bt-connect-device', mac),
    forgetBluetoothPairedDevice: (mac: string): Promise<{ ok: boolean }> =>
      ipcRenderer.invoke('projection-bt-forget-device', mac),
    dongleFirmware: (action: 'check' | 'download' | 'upload' | 'status'): Promise<unknown> =>
      ipcRenderer.invoke('dongle-fw', { action }),
    switchTransport: (): Promise<{ ok: boolean; active: 'dongle' | 'aa' | 'cp' | null }> =>
      ipcRenderer.invoke('transport:switch'),
    getTransportState: (): Promise<{
      active: 'dongle' | 'aa' | 'cp' | null
      dongleDetected: boolean
      wiredPhoneDetected: boolean
      wirelessPhoneActive: boolean
      wiredPhoneActive: boolean
      preference: 'auto' | 'dongle' | 'native'
    }> => ipcRenderer.invoke('transport:state'),
    sendTouch: (x: number, y: number, action: number): void =>
      ipcRenderer.send('projection-touch', { x, y, action }),
    sendMultiTouch: (points: MultiTouchPoint[]): void =>
      ipcRenderer.send('projection-multi-touch', points),
    sendCommand: (key: string): void => ipcRenderer.send('projection-command', key),
    sendRawMessage: (type: number, data: Uint8Array): void => {
      ipcRenderer.send('projection-raw-message', {
        type,
        data: Array.from(data)
      })
    },
    onEvent: (callback: ApiCallback): (() => void) => {
      projectionEventHandlers.push(callback)
      projectionEventQueue.forEach(([evt, ...args]) => callback(evt, ...args))
      projectionEventQueue = []
      return () => {
        projectionEventHandlers = projectionEventHandlers.filter((cb) => cb !== callback)
      }
    },
    readMedia: (): Promise<unknown> => ipcRenderer.invoke('projection-media-read'),
    readNavigation: (): Promise<unknown> => ipcRenderer.invoke('projection-navigation-read'),
    onAudioChunk: (handler: ChunkHandler): void => {
      audioChunkHandler = handler
      audioChunkQueue.forEach((chunk) => handler(chunk))
      audioChunkQueue = []
    },
    offAudioChunk: (handler: ChunkHandler): void => {
      if (audioChunkHandler === handler) {
        audioChunkHandler = null
      }
    },
    setVolume: (stream: 'music' | 'nav' | 'voiceAssistant' | 'call', volume: number): void => {
      ipcRenderer.send('projection-set-volume', { stream, volume })
    },
    setVisualizerEnabled: (enabled: boolean): void => {
      ipcRenderer.send('projection-set-visualizer-enabled', !!enabled)
    },
    requestCluster: (enabled: boolean): Promise<{ ok: boolean; enabled: boolean }> =>
      ipcRenderer.invoke('cluster:request', enabled),
    // macOS-only: nudge the window a pixel to force a surface repaint, clearing the
    // transparent-window stale-paint that otherwise hides the cluster plane until a manual resize.
    clusterRepaintNudge: (): Promise<{ ok: boolean }> =>
      ipcRenderer.invoke('cluster:repaint-nudge'),
    onClusterResolution: (handler: ChunkHandler): (() => void) => {
      clusterResolutionHandlers.push(handler)
      // replay the latest so a late subscriber immediately knows the stream is live
      if (lastClusterResolution != null) handler(lastClusterResolution)
      return () => {
        clusterResolutionHandlers = clusterResolutionHandlers.filter((h) => h !== handler)
      }
    },
    onTelemetry: (handler: (payload: unknown) => void): void => {
      telemetryHandlers.push(handler)
      telemetryQueue.forEach((p) => handler(p))
      telemetryQueue = []
    },
    offTelemetry: (handler: (payload: unknown) => void): void => {
      telemetryHandlers = telemetryHandlers.filter((h) => h !== handler)
    },
    getTelemetrySnapshot: (): Promise<unknown> => ipcRenderer.invoke('telemetry:snapshot')
  }
}

contextBridge.exposeInMainWorld('projection', api)

type UpdateEvent = { phase: string; message?: string }
type UpdateProgress = { phase?: string; percent?: number; received?: number; total?: number }

const appApi = {
  platform: process.platform,
  compositor: process.env.LIVI_COMPOSITOR === '1',
  getVersion: (): Promise<string> => ipcRenderer.invoke('app:getVersion'),
  getLatestRelease: (): Promise<{ version?: string; url?: string }> =>
    ipcRenderer.invoke('app:getLatestRelease'),
  performUpdate: (imageUrl?: string): Promise<void> =>
    ipcRenderer.invoke('app:performUpdate', imageUrl),

  onUpdateEvent: (cb: (payload: UpdateEvent) => void): (() => void) => {
    const ch = 'update:event'
    const handler = (_e: IpcRendererEvent, payload: UpdateEvent) => cb(payload)
    ipcRenderer.on(ch, handler)
    return () => ipcRenderer.removeListener(ch, handler)
  },
  onUpdateProgress: (cb: (payload: UpdateProgress) => void): (() => void) => {
    const ch = 'update:progress'
    const handler = (_e: IpcRendererEvent, payload: UpdateProgress) => cb(payload)
    ipcRenderer.on(ch, handler)
    return () => ipcRenderer.removeListener(ch, handler)
  },

  resetDongleIcons: (): Promise<{
    dongleIcon120?: string
    dongleIcon180?: string
    dongleIcon256?: string
  }> => ipcRenderer.invoke('settings:reset-dongle-icons'),

  beginInstall: (): Promise<void> => ipcRenderer.invoke('app:beginInstall'),
  abortUpdate: (): Promise<void> => ipcRenderer.invoke('app:abortUpdate'),
  quitApp: (): Promise<void> => ipcRenderer.invoke('app:quitApp'),
  restartApp: (): Promise<void> => ipcRenderer.invoke('app:restartApp'),
  openExternal: (url: string): Promise<{ ok: boolean; error?: string }> =>
    ipcRenderer.invoke('app:openExternal', url),

  notifyUserActivity: (): void => {
    ipcRenderer.send('app:user-activity')
  },

  broadcastMediaKey: (command: string): void => {
    ipcRenderer.send('app:media-key', command)
  },

  onMediaKey: (handler: MediaKeyHandler): (() => void) => {
    mediaKeyHandlers.push(handler)
    if (mediaKeyQueue.length) {
      const drained = mediaKeyQueue
      mediaKeyQueue = []
      drained.forEach((cmd) => handler(cmd))
    }
    return () => {
      const i = mediaKeyHandlers.indexOf(handler)
      if (i >= 0) mediaKeyHandlers.splice(i, 1)
    }
  }
}

contextBridge.exposeInMainWorld('app', appApi)
