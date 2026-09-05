import { type ChildProcess, execFileSync, spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { chmodSync, existsSync, mkdirSync, openSync, readFileSync, unlinkSync } from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { app } from 'electron'
import { gstEnv, resolveGStreamerRoot } from '../audio/gstreamer'

// Where the gst-host child drops its crash backtrace.
function crashLogPath(): string {
  const appImage = process.env.APPIMAGE
  const dir = appImage ? path.dirname(appImage) : process.cwd()
  return path.join(dir, 'livi-gst-host-crash.log')
}

// Frame: [uint32 LE len][uint8 op][uint32 LE id][rest]. op 1 create([1B codecLen][codec]
// [codec_data]), 2 data, 3 stop, 4 setGamma (5 float64).
function frame(op: number, id: number, rest: Buffer): Buffer {
  const head = Buffer.allocUnsafe(9)
  head.writeUInt32LE(5 + rest.length, 0)
  head.writeUInt8(op, 4)
  head.writeUInt32LE(id, 5)
  return rest.length ? Buffer.concat([head, rest]) : head
}

// Spawns the gst-video pipeline in a standalone native gst-host binary and forwards
// calls over a unix socket.
export const VIDEO_PLANE_MAIN = 0x7a000001
export const VIDEO_PLANE_CLUSTER_RECV = 0x7a000010
const CLUSTER_PLANE_BASE = 0x7a000011

export function clusterPlaneId(screen: 'main' | 'dash' | 'aux'): number {
  return CLUSTER_PLANE_BASE + (screen === 'main' ? 0 : screen === 'dash' ? 1 : 2)
}

type ReverseEvents = {
  config: (id: number, codec: 'h264' | 'h265', atom: Buffer) => void
  started: (id: number) => void
  audioStarted: (id: number, firstSample: number) => void
  visualizerAudio: (samples: Uint8Array, sampleRate: number) => void
}

/** How a CarPlay audio stream travels. */
export type CpAudioCodec = 'aac-lc' | 'opus' | 'pcm' | 'pcm-le'

const AUDIO_CODEC_BYTE: Record<CpAudioCodec, number> = {
  'aac-lc': 0,
  opus: 1,
  pcm: 2,
  'pcm-le': 3
}

export type AudioStreamOpts = {
  codec: CpAudioCodec
  /** RTP payload type, which is the CarPlay stream type. */
  payloadType: number
  clockRate: number
  channels: number
  /** Jitter buffer depth, the phone's negotiated playout latency. */
  latencyMs: number
  /** Voice and call streams take the short path to the sink. */
  realtime: boolean
  /** The main process feeds this stream instead of the host binding ports. */
  fed?: boolean
  device?: string
}

export type MicStreamOpts = {
  codec: 'opus' | 'pcm'
  payloadType: number
  sampleRate: number
  channels: number
  bitrate: number
  frameMs: number
  /** Where the phone listens. */
  port: number
  phone: string
  device?: string
}

class GstHost {
  private child: ChildProcess | null = null
  private sock: net.Socket | null = null
  private starting = false
  private quitHooked = false
  private readonly queue: Buffer[] = []
  private readonly events = new EventEmitter()
  private readonly portWaiters = new Map<number, (port: number) => void>()
  private readonly audioWaiters = new Map<number, (data: number, control: number) => void>()
  private recvBuf: Buffer = Buffer.alloc(0)
  private nextReceiverId = 0x7b000000
  private feedWaiter: ((path: string) => void) | null = null
  private feedPath: Promise<string> | null = null
  // Set once the host binary is missing, this platform has no host process.
  private unavailable = false

  private start(): void {
    if (this.child || this.starting || this.unavailable) return
    if (process.platform !== 'linux') {
      this.unavailable = true
      console.warn('[gstHost] no host process on this platform, call ignored')
      return
    }
    this.starting = true

    let addonPath: string
    try {
      // require.resolve gives the logical app.asar path. The real files are unpacked (asarUnpack),
      // and spawn plus the child need the physical path.
      addonPath = require
        .resolve('livi-gst-video')
        .replace(`app.asar${path.sep}`, `app.asar.unpacked${path.sep}`)
    } catch (e) {
      console.error('[gstHost] cannot resolve livi-gst-video:', (e as Error).message)
      this.starting = false
      return
    }
    const hostBin = path.join(path.dirname(addonPath), 'build', 'Release', 'livi-gst-host')
    const sockPath = path.join(os.tmpdir(), `livi-gst-${process.pid}.sock`)
    const crashPath = crashLogPath()
    try {
      unlinkSync(sockPath)
    } catch {}
    try {
      unlinkSync(crashPath)
    } catch {}
    try {
      chmodSync(hostBin, 0o755) // asarUnpack can drop the exec bit
    } catch {}

    const server = net.createServer((s) => {
      this.sock = s
      this.recvBuf = Buffer.alloc(0)
      for (const b of this.queue) s.write(b)
      this.queue.length = 0
      s.on('data', (chunk: Buffer) => this.onHostData(chunk))
      s.on('error', () => {})
      s.on('close', () => {
        if (this.sock === s) this.sock = null
      })
    })
    server.on('error', (e) => console.error('[gstHost] server error:', e.message))
    server.listen(sockPath, () => {
      const env = { ...process.env }
      const gstRoot = resolveGStreamerRoot()
      if (gstRoot) Object.assign(env, gstEnv(gstRoot))
      // LIVI_GST_PRELOAD LD_PRELOADs an override lib into the gst-host child only
      if (process.env.LIVI_GST_PRELOAD) env.LD_PRELOAD = process.env.LIVI_GST_PRELOAD
      env.GST_GL_WINDOW = 'surfaceless'
      env.GST_GL_PLATFORM = 'egl'
      // A silent pipeline failure must be diagnosable from the log.
      let stdio: ('ignore' | 'inherit' | number)[] = ['ignore', 'inherit', 'inherit']
      try {
        const logDir = path.join(os.homedir(), '.config', 'LIVI', 'log')
        mkdirSync(logDir, { recursive: true })
        const fd = openSync(path.join(logDir, 'gst-host.log'), 'w')
        stdio = ['ignore', fd, fd]
      } catch {
        // no log dir: keep the inherited stdio
      }
      this.child = spawn(hostBin, [sockPath, crashPath], { env, stdio })
      this.child.on('error', (e: Error) => {
        console.error(
          `[gstHost] cannot start the host, media fed by the helper has nowhere to go: ${e.message}`
        )
        this.unavailable = true
        this.child = null
        this.sock = null
        this.starting = false
        this.feedPath = null
        const waiter = this.feedWaiter
        this.feedWaiter = null
        waiter?.('')
        server.close()
      })
      this.child.on('exit', (code, signal) => {
        console.error('[gstHost] child exited:', code, signal ?? '')
        if (signal && existsSync(crashPath)) {
          console.error(
            `[gstHost] crash backtrace (${crashPath}):\n${readFileSync(crashPath, 'utf8')}`
          )
        }
        this.child = null
        this.sock = null
        this.starting = false
        this.feedPath = null
        server.close()
      })
    })

    if (!this.quitHooked) {
      this.quitHooked = true
      app.on('before-quit', () => this.child?.kill())
    }
  }

  private send(buf: Buffer): void {
    this.start()
    if (this.unavailable) return
    if (this.sock?.writable) this.sock.write(buf)
    else this.queue.push(buf)
  }

  private onHostData(chunk: Buffer): void {
    this.recvBuf = this.recvBuf.length ? Buffer.concat([this.recvBuf, chunk]) : chunk
    for (;;) {
      if (this.recvBuf.length < 4) break
      const len = this.recvBuf.readUInt32LE(0)
      if (this.recvBuf.length < 4 + len) break
      if (len >= 5) {
        const rop = this.recvBuf.readUInt8(4)
        const id = this.recvBuf.readUInt32LE(5)
        this.onReverse(rop, id, this.recvBuf.subarray(9, 4 + len))
      }
      this.recvBuf = this.recvBuf.subarray(4 + len)
    }
  }

  private onReverse(rop: number, id: number, rest: Buffer): void {
    if (rop === 1) {
      const port = rest.length >= 2 ? rest.readUInt16LE(0) : 0
      const waiter = this.portWaiters.get(id)
      if (waiter) {
        this.portWaiters.delete(id)
        waiter(port)
      }
    } else if (rop === 2) {
      const codec = rest.length && rest[0] === 1 ? 'h265' : 'h264'
      this.events.emit('config', id, codec, Buffer.from(rest.subarray(1)))
    } else if (rop === 3) {
      this.events.emit('started', id)
    } else if (rop === 4) {
      const waiter = this.audioWaiters.get(id)
      if (waiter) {
        this.audioWaiters.delete(id)
        waiter(
          rest.length >= 4 ? rest.readUInt16LE(0) : 0,
          rest.length >= 4 ? rest.readUInt16LE(2) : 0
        )
      }
    } else if (rop === 5) {
      this.events.emit('audioStarted', id, rest.length >= 4 ? rest.readUInt32LE(0) : 0)
    } else if (rop === 6 && rest.length >= 4) {
      // [rate u32 LE][mono s16 samples]. Copy off the shared socket buffer.
      this.events.emit('visualizerAudio', new Uint8Array(rest.subarray(4)), rest.readUInt32LE(0))
    } else if (rop === 7) {
      const waiter = this.feedWaiter
      this.feedWaiter = null
      waiter?.(rest.toString('utf8'))
    }
  }

  /** The socket the helper streams media into, empty when the host cannot bind it. */
  openFeed(): Promise<string> {
    if (this.feedPath) return this.feedPath
    const feedPath = path.join(os.tmpdir(), `livi-gst-${process.pid}.sock.feed`)
    this.feedPath = new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.feedWaiter = null
        this.feedPath = null
        resolve('')
      }, 4000)
      this.feedWaiter = (bound) => {
        clearTimeout(timer)
        if (!bound) this.feedPath = null
        resolve(bound)
      }
      this.send(frame(16, 0, Buffer.from(feedPath, 'utf8')))
    })
    return this.feedPath
  }

  openVideoReceiver(
    planeId: number,
    key: Buffer,
    cluster = false
  ): Promise<{ port: number; receiverId: number }> {
    const receiverId = this.nextReceiverId++
    return new Promise((resolve) => {
      const done = (port: number): void => {
        clearTimeout(timer)
        resolve({ port, receiverId })
      }
      const timer = setTimeout(() => {
        this.portWaiters.delete(receiverId)
        resolve({ port: 0, receiverId })
      }, 4000)
      this.portWaiters.set(receiverId, done)
      const head = Buffer.allocUnsafe(5)
      head.writeUInt32LE(planeId, 0)
      head.writeUInt8(cluster ? 1 : 0, 4)
      this.send(frame(5, receiverId, Buffer.concat([head, key])))
    })
  }

  setActiveFeeder(receiverId: number, active: boolean): void {
    this.send(frame(7, receiverId, Buffer.from([active ? 1 : 0])))
  }

  closeVideoReceiver(receiverId: number): void {
    this.send(frame(6, receiverId, Buffer.alloc(0)))
  }

  /** Opens one audio stream in the host and answers with the ports it bound. */
  openAudio(
    key: Buffer,
    o: AudioStreamOpts
  ): Promise<{ streamId: number; dataPort: number; controlPort: number }> {
    const streamId = this.nextReceiverId++
    return new Promise((resolve) => {
      const done = (dataPort: number, controlPort: number): void => {
        clearTimeout(timer)
        resolve({ streamId, dataPort, controlPort })
      }
      const timer = setTimeout(() => {
        this.audioWaiters.delete(streamId)
        resolve({ streamId, dataPort: 0, controlPort: 0 })
      }, 4000)
      this.audioWaiters.set(streamId, done)

      const head = Buffer.alloc(12)
      head.writeUInt8(AUDIO_CODEC_BYTE[o.codec], 0)
      head.writeUInt8(o.payloadType & 0xff, 1)
      head.writeUInt32LE(o.clockRate, 2)
      head.writeUInt8(o.channels, 6)
      head.writeUInt32LE(o.latencyMs, 7)
      head.writeUInt8((o.realtime ? 1 : 0) | (o.fed ? 2 : 0), 11)
      const device = Buffer.from(o.device ?? '', 'utf8')
      this.send(frame(8, streamId, Buffer.concat([head, key, device])))
    })
  }

  /** Feeds samples into a stream the host does not receive itself. */
  pushAudio(streamId: number, samples: Buffer): void {
    this.send(frame(14, streamId, samples))
  }

  /** Toggles the pre-fader mono tap on every audio stream. */
  setVisualizerTap(on: boolean): void {
    this.send(frame(15, 0, Buffer.from([on ? 1 : 0])))
  }

  onVisualizerAudio(cb: ReverseEvents['visualizerAudio']): void {
    this.events.removeAllListeners('visualizerAudio')
    this.events.on('visualizerAudio', cb)
  }

  /** Sets the level at once, or over `rampMs` on the pipeline clock. */
  setAudioVolume(streamId: number, level: number, rampMs = 0): void {
    const body = Buffer.alloc(12)
    body.writeDoubleLE(level, 0)
    body.writeUInt32LE(Math.max(0, Math.round(rampMs)), 8)
    this.send(frame(9, streamId, body))
  }

  /** Only the active phone's streams reach the sink. */
  setAudioActive(streamId: number, active: boolean): void {
    this.send(frame(13, streamId, Buffer.from([active ? 1 : 0])))
  }

  closeAudio(streamId: number): void {
    this.audioWaiters.delete(streamId)
    this.send(frame(10, streamId, Buffer.alloc(0)))
  }

  /** Starts capturing and sending the microphone stream. */
  openMic(key: Buffer, o: MicStreamOpts): number {
    const streamId = this.nextReceiverId++
    const phone = Buffer.from(o.phone, 'utf8')
    const head = Buffer.alloc(17)
    head.writeUInt8(o.codec === 'pcm' ? 1 : 0, 0)
    head.writeUInt8(o.payloadType & 0xff, 1)
    head.writeUInt32LE(o.sampleRate, 2)
    head.writeUInt8(o.channels, 6)
    head.writeUInt32LE(o.bitrate, 7)
    head.writeUInt32LE(o.frameMs, 11)
    head.writeUInt16LE(o.port, 15)
    const device = Buffer.from(o.device ?? '', 'utf8')
    this.send(
      frame(11, streamId, Buffer.concat([head, key, Buffer.from([phone.length]), phone, device]))
    )
    return streamId
  }

  closeMic(streamId: number): void {
    this.send(frame(12, streamId, Buffer.alloc(0)))
  }

  /** Captures the microphone and streams raw samples into the socket at `path`. */
  openMicTap(path: string, o: { sampleRate: number; channels: number; device?: string }): number {
    const tapId = this.nextReceiverId++
    const head = Buffer.alloc(6)
    head.writeUInt32LE(o.sampleRate, 0)
    head.writeUInt8(o.channels, 4)
    const device = Buffer.from(o.device ?? '', 'utf8')
    head.writeUInt8(device.length, 5)
    this.send(frame(20, tapId, Buffer.concat([head, device, Buffer.from(path, 'utf8')])))
    return tapId
  }

  closeMicTap(tapId: number): void {
    this.send(frame(21, tapId, Buffer.alloc(0)))
  }

  onAudioStarted(cb: ReverseEvents['audioStarted']): void {
    this.events.removeAllListeners('audioStarted')
    this.events.on('audioStarted', cb)
  }

  onVideoReceiverConfig(cb: ReverseEvents['config']): void {
    this.events.removeAllListeners('config')
    this.events.on('config', cb)
  }

  onVideoReceiverStarted(cb: ReverseEvents['started']): void {
    this.events.removeAllListeners('started')
    this.events.on('started', cb)
  }

  createPlayer(id: number, codec: string, codecData?: Buffer): void {
    const c = Buffer.from(codec, 'utf8')
    const head = Buffer.from([c.length])
    const rest =
      codecData && codecData.length ? Buffer.concat([head, c, codecData]) : Buffer.concat([head, c])
    this.send(frame(1, id, rest))
  }

  pushBuffer(id: number, nal: Buffer): void {
    this.send(frame(2, id, nal))
  }

  stop(id: number): void {
    this.send(frame(3, id, Buffer.alloc(0)))
  }

  // op 4: calibration LUT as 5 little-endian float64 (gamma, contrast, gain R/G/B).
  setGamma(id: number, gamma: number, contrast: number, r: number, g: number, b: number): void {
    const rest = Buffer.allocUnsafe(40)
    rest.writeDoubleLE(gamma, 0)
    rest.writeDoubleLE(contrast, 8)
    rest.writeDoubleLE(r, 16)
    rest.writeDoubleLE(g, 24)
    rest.writeDoubleLE(b, 32)
    this.send(frame(4, id, rest))
  }
}

export const gstHost = new GstHost()

function resolveHostBinary(): string | null {
  try {
    const addonPath = require
      .resolve('livi-gst-video')
      .replace(`app.asar${path.sep}`, `app.asar.unpacked${path.sep}`)
    return path.join(path.dirname(addonPath), 'build', 'Release', 'livi-gst-host')
  } catch {
    return null
  }
}

export function probeCodecsViaHost(): Record<string, { hw: boolean; sw: boolean }> | null {
  try {
    const bin = resolveHostBinary()
    if (!bin) return null
    const env: NodeJS.ProcessEnv = { ...process.env }
    const gstRoot = resolveGStreamerRoot()
    if (gstRoot) Object.assign(env, gstEnv(gstRoot))
    env.GST_GL_WINDOW = 'surfaceless'
    env.GST_GL_PLATFORM = 'egl'
    try {
      chmodSync(bin, 0o755)
    } catch {}
    const out = execFileSync(bin, ['--probe'], { env, encoding: 'utf8', timeout: 10000 })
    return JSON.parse(out.trim()) as Record<string, { hw: boolean; sw: boolean }>
  } catch (e) {
    console.error('[gstHost] codec probe failed:', (e as Error).message)
    return null
  }
}
