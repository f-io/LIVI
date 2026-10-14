// On macOS the video addon stands in for the compositor. Core places the planes over its
// control socket, gst-host hands over the frames and the addon draws them into the windows.
import { dirname, join } from 'node:path'
import { coreSocketPath } from '@shared/core/link'
import { app, type BrowserWindow } from 'electron'

type VideoAddon = {
  serve(control: string, planes: string): boolean
  setWindow(screen: string, handle?: Buffer | null): void
}

export type VideoScreen = 'main' | 'cluster' | 'aux'

let addon: VideoAddon | null = null

function addonPath(): string {
  return app.isPackaged
    ? join(process.resourcesPath, 'video', 'gst_video.node')
    : join(app.getAppPath(), 'native', 'livi-gst-video', 'build', 'Release', 'gst_video.node')
}

function useBundledGStreamer(): void {
  if (!app.isPackaged) return
  const root = join(process.resourcesPath, 'gstreamer', 'macos-arm64')
  process.env.GST_PLUGIN_SYSTEM_PATH = ''
  process.env.GST_PLUGIN_PATH = join(root, 'lib', 'gstreamer-1.0')
  process.env.GST_PLUGIN_SCANNER = join(root, 'libexec', 'gstreamer-1.0', 'gst-plugin-scanner')
}

/** Core passes this path on to gst-host in its environment. */
export function uiPlanesPath(): string {
  return join(dirname(coreSocketPath()), 'ui-planes.sock')
}

/** Must listen before core starts, core looks for the socket where the compositor's would be. */
export function startVideo(): void {
  if (process.platform !== 'darwin') return
  const path = addonPath()
  try {
    useBundledGStreamer()
    const mod = { exports: {} as VideoAddon }
    process.dlopen(mod, path)
    addon = mod.exports
  } catch (e) {
    console.error(`[video] ${path} did not load, there will be no video: ${(e as Error).message}`)
    return
  }
  const control = join(dirname(coreSocketPath()), 'compositor.ctrl')
  if (!addon.serve(control, uiPlanesPath())) addon = null
}

export function drawVideoIn(screen: VideoScreen, win: BrowserWindow | null): void {
  if (!addon) return
  addon.setWindow(screen, win && !win.isDestroyed() ? win.getNativeWindowHandle() : null)
}
