import { type ChildProcess, execFileSync, spawn } from 'node:child_process'
import { chmodSync, existsSync, readFileSync, unlinkSync } from 'node:fs'
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

// Spawns the gst-video pipeline in a standalone native gst-host binary (not the Electron
// executable, which exports a bundled libffi that corrupts wayland marshalling) and forwards
// calls over a unix socket.
class GstHost {
  private child: ChildProcess | null = null
  private sock: net.Socket | null = null
  private starting = false
  private quitHooked = false
  private readonly queue: Buffer[] = []

  private start(): void {
    if (this.child || this.starting) return
    this.starting = true

    let addonPath: string
    try {
      // require.resolve gives the logical app.asar path; the real files are unpacked (asarUnpack),
      // and spawn plus the child need the physical path.
      addonPath = require
        .resolve('gst-video')
        .replace(`app.asar${path.sep}`, `app.asar.unpacked${path.sep}`)
    } catch (e) {
      console.error('[gstHost] cannot resolve gst-video:', (e as Error).message)
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
      for (const b of this.queue) s.write(b)
      this.queue.length = 0
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
      this.child = spawn(hostBin, [sockPath, crashPath], {
        env,
        stdio: ['ignore', 'inherit', 'inherit']
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
    if (this.sock?.writable) this.sock.write(buf)
    else this.queue.push(buf)
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
      .resolve('gst-video')
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
