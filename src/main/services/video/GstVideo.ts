import net from 'node:net'
import { app, BrowserWindow, type WebContents } from 'electron'
import path from 'path'
import { resolveGStreamerRoot } from '../audio/gstreamer'

export type GstVideoCodec = 'h264' | 'h265' | 'vp9' | 'av1'

// Linux: control channel to livi-compositor. Video planes are addressed by tag (claim),
// then placed (videocfg) and toggled (videoshow). `state` is resent on reconnect.
class CompositorControl {
  private socket: net.Socket | null = null
  private connecting = false
  private readonly state = new Map<string, string>() // videocfg/videoshow/backdrop, resent
  private outbox: string[] = [] // one-shot lines (claims), sent once
  private readonly path = process.env.LIVI_COMPOSITOR_CTRL ?? ''

  private get enabled(): boolean {
    return process.platform === 'linux' && this.path.length > 0
  }

  // The next new video toplevel gets this tag. Send before creating the waylandsink.
  claim(tag: string): void {
    if (!this.enabled) return
    this.outbox.push(`claim ${tag}\n`)
    this.flush()
  }

  // Place + crop the tagged plane on a screen (fullscreen with its own AA content region).
  videocfg(
    tag: string,
    screen: string,
    cropL: number,
    cropT: number,
    visW: number,
    visH: number,
    tierW: number,
    tierH: number
  ): void {
    if (!this.enabled) return
    const n = (v: number): number => Math.round(v)
    this.state.set(
      `cfg:${tag}`,
      `videocfg ${tag} ${screen} ${n(cropL)} ${n(cropT)} ${n(visW)} ${n(visH)} ${n(tierW)} ${n(tierH)}\n`
    )
    this.flush()
  }

  // Toggle the tagged plane's visibility.
  videoshow(tag: string, visible: boolean): void {
    if (!this.enabled) return
    this.state.set(`show:${tag}`, `videoshow ${tag} ${visible ? 1 : 0}\n`)
    this.flush()
  }

  // Open/close a role's nested output (its own movable host window). Resent on reconnect.
  // Optional w/h sizes the output to that screen's own configured resolution.
  screen(role: string, on: boolean, w?: number, h?: number): void {
    if (!this.enabled) return
    const size = w && h && w > 0 && h > 0 ? ` ${Math.round(w)} ${Math.round(h)}` : ''
    this.state.set(`screen:${role}`, `screen ${role} ${on ? 1 : 0}${size}\n`)
    this.flush()
  }

  // Theme background for the compositor backdrop (kept in sync with themeColors.ts).
  setBackdrop(darkMode: boolean): void {
    if (!this.enabled) return
    const [r, g, b] = darkMode ? [0, 0, 0] : [0xd4, 0xd4, 0xd4]
    this.state.set('__backdrop__', `backdrop ${r} ${g} ${b}\n`)
    this.flush()
  }

  // Ask the compositor to relaunch its inner UI child (the Electron app). One-shot, not resent on reconnect.
  restart(): boolean {
    if (!this.enabled) return false
    this.outbox.push('restart\n')
    this.flush()
    return true
  }

  private flush(): void {
    const s = this.socket
    if (s && !s.destroyed && s.writable) {
      for (const line of this.outbox) s.write(line)
      this.outbox = []
      for (const line of this.state.values()) s.write(line)
      return
    }
    this.connect()
  }

  private connect(): void {
    if (this.connecting || !this.enabled) return
    this.connecting = true
    const s = net.connect(this.path)
    s.on('connect', () => {
      this.connecting = false
      this.socket = s
      this.flush()
    })
    s.on('error', () => {
      this.connecting = false
    })
    s.on('close', () => {
      this.connecting = false
      if (this.socket === s) this.socket = null
    })
  }
}

const compositorControl = new CompositorControl()

// Push the theme background colour to the compositor backdrop (Linux/compositor only)
export function setCompositorBackdrop(darkMode: boolean): void {
  compositorControl.setBackdrop(darkMode)
}

// macOS only: paint the window's content view (below the video subviews) with the theme colour
export function setMacBackdrop(win: BrowserWindow, darkMode: boolean): void {
  if (process.platform !== 'darwin') return
  if (!win || win.isDestroyed()) return
  const a = load()
  if (!a || typeof a.setBackdrop !== 'function') return
  const [r, g, b] = darkMode ? [0, 0, 0] : [0xd4 / 255, 0xd4 / 255, 0xd4 / 255]
  try {
    a.setBackdrop(win.getNativeWindowHandle(), r, g, b)
  } catch {
    // older addon build without setBackdrop, or no handle yet
  }
}

// Open/close a secondary screen's nested output window (Linux/compositor only).
// Optional w/h sizes the output to that screen's own configured resolution.
export function setCompositorScreen(role: string, on: boolean, w?: number, h?: number): void {
  compositorControl.screen(role, on, w, h)
}

// Ask the compositor to relaunch the inner UI (Linux/compositor only). Returns false when
// not running in the compositor, so the caller can fall back to a normal relaunch.
export function compositorRestart(): boolean {
  return compositorControl.restart()
}

export type GstCodecSupport = { hw: boolean; sw: boolean }
export type GstCodecProbe = Record<GstVideoCodec, GstCodecSupport>

interface GstAddon {
  version(): string
  probeCodecs(): GstCodecProbe
  createPlayer(codec: string, windowHandle: Buffer): unknown
  start(player: unknown): void
  pushBuffer(player: unknown, buffer: Buffer): boolean
  setVisible(player: unknown, visible: boolean): void
  setContentRegion(
    player: unknown,
    cropL: number,
    cropT: number,
    visW: number,
    visH: number,
    tierW: number,
    tierH: number
  ): void
  setBackdrop(windowHandle: Buffer, r: number, g: number, b: number): void
  stop(player: unknown): void
}

let addon: GstAddon | null = null
let loadFailed = false

// Windows has no system GStreamer
function prepareWindowsRuntime(): void {
  if (process.platform !== 'win32') return
  const root = resolveGStreamerRoot()
  if (!root) return
  process.env.PATH = `${path.join(root, 'bin')};${process.env.PATH ?? ''}`
  process.env.GST_PLUGIN_SYSTEM_PATH = ''
  process.env.GST_PLUGIN_PATH = path.join(root, 'lib', 'gstreamer-1.0')
  process.env.GST_PLUGIN_SCANNER = path.join(
    root,
    'libexec',
    'gstreamer-1.0',
    'gst-plugin-scanner.exe'
  )
}

function prepareMacRuntime(): void {
  if (process.platform !== 'darwin' || !app.isPackaged) return
  const root = resolveGStreamerRoot()
  if (!root) return
  process.env.GST_PLUGIN_SYSTEM_PATH = ''
  process.env.GST_PLUGIN_PATH = path.join(root, 'lib', 'gstreamer-1.0')
  process.env.GST_PLUGIN_SCANNER = path.join(root, 'libexec', 'gstreamer-1.0', 'gst-plugin-scanner')
}

function load(): GstAddon | null {
  if (addon || loadFailed) return addon
  try {
    prepareWindowsRuntime()
    prepareMacRuntime()
    addon = require('gst-video') as GstAddon
    console.log('[GstVideo]', addon.version())
  } catch (e) {
    loadFailed = true
    console.error('[GstVideo] native addon load failed:', (e as Error).message)
  }
  return addon
}

// Which codecs the bundled/loaded GStreamer can decode on this platform,
// and whether the decoder is hardware-accelerated
export function probeGstCodecs(): GstCodecProbe {
  const none: GstCodecSupport = { hw: false, sw: false }
  const a = load()
  if (!a) return { h264: none, h265: none, vp9: none, av1: none }
  try {
    return a.probeCodecs()
  } catch {
    return { h264: none, h265: none, vp9: none, av1: none }
  }
}

// In-process GStreamer video player rendering into a window's native surface
export class GstVideo {
  private player: unknown = null
  private codec: GstVideoCodec | null = null
  private visible = true
  // AA content region inside the decoded tier (so the user-chosen AR fills the display)
  private region: {
    cropL: number
    cropT: number
    visW: number
    visH: number
    tierW: number
    tierH: number
  } | null = null

  // role = compositor tag for this plane; targetScreen = which screen it's placed on
  constructor(
    private readonly wc: WebContents,
    private readonly role: string = 'main',
    private readonly targetScreen: string = 'main'
  ) {}

  private windowHandle(): Buffer | null {
    const win = BrowserWindow.fromWebContents(this.wc)
    if (!win || win.isDestroyed()) return null
    return win.getNativeWindowHandle()
  }

  private ensure(codec: GstVideoCodec): void {
    const a = load()
    if (!a) return
    if (this.player && this.codec === codec) return
    this.dispose()
    const handle = this.windowHandle()
    if (!handle) return
    compositorControl.claim(this.role) // Linux: tag the waylandsink toplevel we create next
    this.player = a.createPlayer(codec, handle)
    this.codec = codec
    if (this.player) {
      a.start(this.player)
      a.setVisible(this.player, this.visible)
      if (this.region) this.applyRegion(a)
    }
  }

  push(codec: GstVideoCodec, nal: Buffer): void {
    const a = load()
    if (!a) return
    this.ensure(codec)
    if (this.player) a.pushBuffer(this.player, nal)
  }

  // Show/hide the video surface as the user navigates in/out of projection
  setVisible(visible: boolean): void {
    this.visible = visible
    compositorControl.videoshow(this.role, visible) // Linux: toggle the compositor plane
    if (addon && this.player) addon.setVisible(this.player, visible)
  }

  // Set the AA content region inside the decoded tier. The native view crops to it by
  // sizing + positioning the GL render (zero-copy).
  setContentRegion(
    cropL: number,
    cropT: number,
    visW: number,
    visH: number,
    tierW: number,
    tierH: number
  ): void {
    this.region = visW > 0 && visH > 0 ? { cropL, cropT, visW, visH, tierW, tierH } : null
    // Linux: the compositor places + crops the tagged plane on its target screen
    compositorControl.videocfg(this.role, this.targetScreen, cropL, cropT, visW, visH, tierW, tierH)
    if (addon && this.player) this.applyRegion(addon)
  }

  private applyRegion(a: GstAddon): void {
    if (!this.player) return
    const r = this.region
    a.setContentRegion(
      this.player,
      r?.cropL ?? 0,
      r?.cropT ?? 0,
      r?.visW ?? 0,
      r?.visH ?? 0,
      r?.tierW ?? 0,
      r?.tierH ?? 0
    )
  }

  dispose(): void {
    if (addon && this.player) {
      try {
        addon.stop(this.player)
      } catch {
        /* ignore */
      }
    }
    this.player = null
    this.codec = null
  }
}
