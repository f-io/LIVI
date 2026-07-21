import { execFileSync } from 'node:child_process'
import net from 'node:net'
import { app, BrowserWindow, type WebContents } from 'electron'
import path from 'path'
import { gstEnv, resolveBinary, resolveGStreamerRoot } from '../audio/gstreamer'
import { gstHost, probeCodecsViaHost } from './gstHost'

export type GstVideoCodec = 'h264' | 'h265' | 'vp9' | 'av1'

// Parse "#rrggbb" into 0..255 channels, falls back to black on a malformed value
function hexToRgb255(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{6})$/i.exec((hex ?? '').trim())
  if (!m) return [0, 0, 0]
  const n = Number.parseInt(m[1], 16)
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff]
}

// Linux runs the pipeline in the gstHost child process (its own GLib loop so waylandsink resizes
// live, and out of reach of the Electron-vs-system libffi crash). mac/Windows render in-process.
const useHostProcess = process.platform === 'linux'
let nextPlayerId = 1

// Display calibration applied as a glshader pass in each video pipeline.
type GammaState = { gamma: number; contrast: number; r: number; g: number; b: number }
let currentGamma: GammaState = { gamma: 1, contrast: 1, r: 1, g: 1, b: 1 }
const livePlayers = new Set<GstVideo>()

// Linux: control channel to livi-compositor. Video planes are addressed by tag (claim),
// then placed (videocfg) and toggled (videoshow). `state` is resent on reconnect.
/** Millimetres per pixel per screen role, reported by the compositor from the panel's EDID. */
const panelMmPerPx = new Map<string, { x: number; y: number }>()

/** Physical size in mm for a pixel count on that screen, or null when the panel is unknown. */
export function panelPhysicalMm(
  role: string,
  widthPixels: number,
  heightPixels: number
): { widthMm: number; heightMm: number } | null {
  const p = panelMmPerPx.get(role)
  if (!p || !(p.x > 0) || !(p.y > 0)) return null
  const widthMm = Math.round(widthPixels * p.x)
  const heightMm = Math.round(heightPixels * p.y)
  if (widthMm <= 0 || heightMm <= 0) return null
  return { widthMm, heightMm }
}

class CompositorControl {
  private socket: net.Socket | null = null
  private connecting = false
  private readonly state = new Map<string, string>() // videocfg/videoshow/backdrop, resent
  private outbox: string[] = [] // one-shot lines (claims), sent once
  private readonly path = process.env.LIVI_COMPOSITOR_CTRL ?? ''
  private claimQueue: Array<{ role: string; onClaimed: () => void }> = []
  private claimInFlight: string | null = null
  private claimTimer: ReturnType<typeof setTimeout> | null = null
  private inbox = ''

  private get enabled(): boolean {
    return process.platform === 'linux' && this.path.length > 0
  }

  // The next new video toplevel gets this tag. Send before creating the waylandsink.
  claim(tag: string): void {
    if (!this.enabled) return
    this.outbox.push(`claim ${tag}\n`)
    this.flush()
  }

  private unclaim(tag: string): void {
    if (!this.enabled) return
    this.outbox.push(`unclaim ${tag}\n`)
    this.flush()
  }

  serializedClaim(role: string, onClaimed: () => void): void {
    if (!this.enabled) {
      onClaimed()
      return
    }
    this.claimQueue.push({ role, onClaimed })
    this.pumpClaims()
  }

  releaseClaim(role: string): void {
    if (!this.enabled) return
    this.claimQueue = this.claimQueue.filter((c) => c.role !== role)
    if (this.claimInFlight === role) this.abortInFlightClaim()
  }

  private pumpClaims(): void {
    if (this.claimInFlight || this.claimQueue.length === 0) return
    const next = this.claimQueue.shift()
    if (!next) return
    this.claimInFlight = next.role
    this.claim(next.role)
    if (this.claimTimer) clearTimeout(this.claimTimer)
    this.claimTimer = setTimeout(() => {
      if (this.claimInFlight === next.role) this.abortInFlightClaim()
    }, 3000)
    next.onClaimed()
  }

  private abortInFlightClaim(): void {
    if (this.claimInFlight) this.unclaim(this.claimInFlight)
    this.endClaim()
  }

  private endClaim(): void {
    if (this.claimTimer) {
      clearTimeout(this.claimTimer)
      this.claimTimer = null
    }
    this.claimInFlight = null
    this.pumpClaims()
  }

  private onCtrlData(chunk: Buffer | string): void {
    this.inbox += typeof chunk === 'string' ? chunk : chunk.toString('utf8')
    let nl = this.inbox.indexOf('\n')
    while (nl >= 0) {
      const line = this.inbox.slice(0, nl)
      this.inbox = this.inbox.slice(nl + 1)
      const m = /^bound (.+)$/.exec(line)
      if (m && this.claimInFlight === m[1]) this.endClaim()
      const p = /^panel (\S+) (\d+) (\d+) (\d+) (\d+)$/.exec(line)
      if (p) {
        const [, role, mmW, mmH, pxW, pxH] = p
        panelMmPerPx.set(role, {
          x: Number(mmW) / Number(pxW),
          y: Number(mmH) / Number(pxH)
        })
        console.log(`[compositor] panel '${role}': ${mmW}x${mmH} mm over ${pxW}x${pxH} px`)
      }
      nl = this.inbox.indexOf('\n')
    }
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

  // Theme background for the compositor backdrop, hex "#rrggbb" from config.
  setBackdrop(hex: string): void {
    if (!this.enabled) return
    const [r, g, b] = hexToRgb255(hex)
    this.state.set('__backdrop__', `backdrop ${r} ${g} ${b}\n`)
    this.flush()
  }

  // Push the display calibration to the compositor's per-video shader pass.
  gamma(gamma: number, contrast: number, r: number, g: number, b: number): void {
    if (!this.enabled) return
    this.state.set('__gamma__', `gamma ${gamma} ${contrast} ${r} ${g} ${b}\n`)
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
      this.inbox = ''
      this.flush()
    })
    s.on('data', (chunk) => this.onCtrlData(chunk))
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

// Resolve the active backdrop colour for a config, falling back to the theme defaults
export function backdropHex(darkMode: boolean, dark?: string, light?: string): string {
  return (darkMode ? dark : light) || (darkMode ? '#000000' : '#d4d4d4')
}

// Push the theme background colour to the compositor backdrop (Linux/compositor only)
export function setCompositorBackdrop(hex: string): void {
  compositorControl.setBackdrop(hex)
}

// Push the display calibration into every live video pipeline's glshader pass (all platforms).
export function setStreamGamma(
  gamma: number,
  contrast: number,
  r: number,
  g: number,
  b: number
): void {
  currentGamma = { gamma, contrast, r, g, b }
  if (useHostProcess) compositorControl.gamma(gamma, contrast, r, g, b)
  for (const p of livePlayers) p.applyGamma()
}

// macOS only: paint the window's content view (below the video subviews) with the theme colour
export function setMacBackdrop(win: BrowserWindow, hex: string): void {
  if (process.platform !== 'darwin') return
  if (!win || win.isDestroyed()) return
  const a = load()
  if (!a || typeof a.setBackdrop !== 'function') return
  const [r, g, b] = hexToRgb255(hex)
  try {
    a.setBackdrop(win.getNativeWindowHandle(), r / 255, g / 255, b / 255)
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
  setGamma(player: unknown, gamma: number, contrast: number, r: number, g: number, b: number): void
  stop(player: unknown): void
}

let addon: GstAddon | null = null
let loadFailed = false

function runningGstVersion(fallback: string): string {
  try {
    const root = resolveGStreamerRoot()
    const bin = resolveBinary('gst-launch-1.0')
    if (!root || !bin) return fallback
    const out = execFileSync(bin, ['--version'], { env: gstEnv(root), encoding: 'utf8' })
    const m = out.match(/GStreamer\s+(\S+)/)
    if (m) return `GStreamer ${m[1]} (bundled)`
  } catch {}
  return fallback
}

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
  // Linux uses gstHost + --probe, never the in-process addon (which links the system GStreamer
  // that the bundle replaces). Log the bundled version and skip the load.
  if (useHostProcess) {
    loadFailed = true
    console.log('[GstVideo]', runningGstVersion('GStreamer (bundled)'))
    return null
  }
  try {
    prepareWindowsRuntime()
    prepareMacRuntime()
    addon = require('gst-video') as GstAddon
    console.log('[GstVideo]', runningGstVersion(addon.version()))
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
  if (process.platform === 'linux') {
    const p = probeCodecsViaHost()
    if (p?.h264 && p.h265 && p.vp9 && p.av1) {
      return {
        h264: { hw: !!p.h264.hw, sw: !!p.h264.sw },
        h265: { hw: !!p.h265.hw, sw: !!p.h265.sw },
        vp9: { hw: !!p.vp9.hw, sw: !!p.vp9.sw },
        av1: { hw: !!p.av1.hw, sw: !!p.av1.sw }
      }
    }
    // host probe unavailable: fall through to the in-process addon
  }
  if (!a) return { h264: none, h265: none, vp9: none, av1: none }
  try {
    return a.probeCodecs()
  } catch {
    return { h264: none, h265: none, vp9: none, av1: none }
  }
}

// GStreamer video player. On Linux the pipeline lives in the gstHost child process and this only
// holds an id for it; on mac/Windows it drives the in-process addon directly.
export class GstVideo {
  private readonly id = nextPlayerId++
  private started = false
  private claiming = false
  private setupGen = 0
  private pendingBuffers: Buffer[] = []
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
  ) {
    livePlayers.add(this)
  }

  // Apply the current calibration to this player's glshader pass. Re-sent after each (re)create.
  applyGamma(): void {
    const { gamma, contrast, r, g, b } = currentGamma
    if (useHostProcess) {
      compositorControl.gamma(gamma, contrast, r, g, b)
      return
    }
    if (addon && this.player) addon.setGamma(this.player, gamma, contrast, r, g, b)
  }

  private windowHandle(): Buffer | null {
    const win = BrowserWindow.fromWebContents(this.wc)
    if (!win || win.isDestroyed()) return null
    return win.getNativeWindowHandle()
  }

  private ensure(codec: GstVideoCodec): void {
    if (useHostProcess) {
      if (this.started && this.codec === codec) return
      if (this.claiming) return
      this.dispose()
      this.claiming = true
      const gen = ++this.setupGen
      compositorControl.serializedClaim(this.role, () => {
        if (this.setupGen !== gen) return
        this.claiming = false
        gstHost.createPlayer(this.id, codec)
        this.codec = codec
        this.started = true
        this.applyGamma()
        for (const b of this.pendingBuffers) gstHost.pushBuffer(this.id, b)
        this.pendingBuffers = []
      })
      return
    }
    const a = load()
    if (!a) return
    if (this.player && this.codec === codec) return
    this.dispose()
    const handle = this.windowHandle()
    if (!handle) return
    compositorControl.claim(this.role)
    this.player = a.createPlayer(codec, handle)
    this.codec = codec
    if (this.player) {
      a.start(this.player)
      a.setVisible(this.player, this.visible)
      if (this.region) this.applyRegion(a)
      this.applyGamma()
    }
  }

  push(codec: GstVideoCodec, nal: Buffer): void {
    if (useHostProcess) {
      this.ensure(codec)
      if (this.started) {
        gstHost.pushBuffer(this.id, nal)
      } else if (this.claiming) {
        if (this.pendingBuffers.length >= 240) this.pendingBuffers.shift()
        this.pendingBuffers.push(nal)
      }
      return
    }
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
    if (useHostProcess) {
      this.setupGen++
      this.claiming = false
      this.pendingBuffers = []
      compositorControl.releaseClaim(this.role)
      if (this.started) gstHost.stop(this.id)
      this.started = false
      this.codec = null
      return
    }
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
