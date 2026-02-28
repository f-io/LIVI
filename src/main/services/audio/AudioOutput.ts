import { spawn, ChildProcessWithoutNullStreams, execSync } from 'child_process'
import os from 'os'
import fs from 'fs'
import path from 'path'

export interface AudioOutputOptions {
  sampleRate: number
  channels: number
}

export class AudioOutput {
  private process: ChildProcessWithoutNullStreams | null = null
  private readonly sampleRate: number
  private readonly channels: number

  private bytesWritten = 0
  private queue: Buffer[] = []
  private writing = false

  constructor(opts: AudioOutputOptions) {
    this.sampleRate = opts.sampleRate
    this.channels = Math.max(1, opts.channels | 0)

    console.debug('[AudioOutput] Init', {
      sampleRate: this.sampleRate,
      channels: this.channels,
      platform: os.platform()
    })
  }

  start(): void {
    this.stop()

    let cmd: string
    let args: string[]
    const env = { ...process.env, PATH: AudioOutput.buildExecPath(process.env.PATH) }

    if (os.platform() === 'linux') {
      cmd = 'pw-play'
      args = [
        '--raw',
        '--rate',
        this.sampleRate.toString(),
        '--channels',
        this.channels.toString(),
        '-' // stdin
      ]
    } else if (os.platform() === 'darwin') {
      const playPath = AudioOutput.resolvePlayPath()
      if (!playPath) {
        console.error('[AudioOutput] SoX (play) not found. Install with: brew install sox')
        return
      }
      // Known macOS limitation: minimizing the window via the frame controls (-)
      // can block the main thread (event loop) for ~575ms,
      // causing audio chunk gaps (data source stall).
      cmd = playPath
      args = [
        '-q',
        '--buffer',
        '4096',
        '-t',
        'raw',
        '-r',
        this.sampleRate.toString(),
        '-e',
        'signed-integer',
        '-b',
        '16',
        '-c',
        this.channels.toString(),
        '-L',
        '--ignore-length',
        '-', // stdin
        '-t',
        'coreaudio',
        'default'
      ]
    } else if (os.platform() === 'win32') {
      const ffplayPath = AudioOutput.resolveFfplayPath()
      if (!ffplayPath) {
        console.error('[AudioOutput] ffplay not found (expected resources/bin/ffplay.exe)')
        return
      }

      cmd = ffplayPath
      args = [
        '-nodisp',
        '-loglevel',
        'warning',
        '-f',
        's16le',
        '-ar',
        this.sampleRate.toString(),
        '-ch_layout',
        AudioOutput.ffplayChannelLayout(this.channels),
        '-i',
        'pipe:0'
      ]
    } else {
      console.error('[AudioOutput] Platform not supported for audio output')
      return
    }

    console.debug('[AudioOutput] Spawning', cmd, args.join(' '))
    this.bytesWritten = 0
    this.queue = []
    this.writing = false

    const spawnEnv = os.platform() === 'win32' ? process.env : env
    this.process = spawn(cmd, args, { env: spawnEnv, shell: false })

    const proc = this.process
    const stdin = proc.stdin

    stdin.on('error', (err) => {
      console.warn('[AudioOutput] stdin error:', err.message)
    })
    stdin.on('drain', () => this.flushQueue())

    proc.stderr.on('data', (d: Buffer) => {
      const s = d.toString().trim()
      if (s) console.warn('[AudioOutput] STDERR:', s)
    })
    proc.on('error', (err) => {
      console.error('[AudioOutput] process error:', err)
      this.cleanup()
    })
    proc.on('close', (code, signal) => {
      console.debug('[AudioOutput] process exited', {
        code,
        signal,
        bytesWritten: this.bytesWritten
      })
      this.cleanup()
    })

    console.debug('[AudioOutput] playback started')
  }

  private flushQueue(): void {
    const proc = this.process
    if (!proc || !proc.stdin || proc.stdin.destroyed) {
      this.queue = []
      this.writing = false
      return
    }

    const stdin = proc.stdin
    this.writing = true

    while (this.queue.length > 0) {
      const buf = this.queue.shift()!
      const ok = stdin.write(buf)
      this.bytesWritten += buf.byteLength
      if (!ok) return
    }

    this.writing = false
  }

  write(chunk: Int16Array | Buffer | undefined | null): void {
    const proc = this.process
    if (!proc || !proc.stdin || proc.stdin.destroyed) return
    if (!chunk) return

    const buf = Buffer.isBuffer(chunk)
      ? chunk
      : Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength)

    this.queue.push(buf)
    if (!this.writing) this.flushQueue()
  }

  stop(): void {
    if (!this.process) return
    try {
      if (this.process.stdin && !this.process.stdin.destroyed) {
        this.process.stdin.end()
      }
    } catch (e) {
      console.warn('[AudioOutput] failed to end stdin:', e)
    }
    try {
      this.process.kill()
    } catch (e) {
      console.warn('[AudioOutput] failed to kill process:', e)
    }
    this.cleanup()
  }

  dispose(): void {
    this.stop()
  }

  private cleanup(): void {
    this.queue = []
    this.writing = false
    this.process = null
  }

  private static resolvePlayPath(): string | null {
    const fromEnv = process.env.SOX_PLAY_PATH
    if (fromEnv && fs.existsSync(fromEnv)) return fromEnv

    const candidates = ['/opt/homebrew/bin/play', '/usr/local/bin/play']
    for (const p of candidates) if (fs.existsSync(p)) return p

    try {
      const widened = AudioOutput.buildExecPath(process.env.PATH)
      const out = execSync('which play', {
        encoding: 'utf8',
        env: { ...process.env, PATH: widened }
      })
        .toString()
        .trim()
      if (out && fs.existsSync(out)) return out
    } catch {}

    return null
  }

  private static resolveFfplayPath(): string | null {
    const bundled = path.join(process.resourcesPath, 'bin', 'ffplay.exe')
    return fs.existsSync(bundled) ? bundled : null
  }

  private static buildExecPath(current?: string): string {
    const extra = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin']
    const set = new Set<string>([...extra, ...(current ? current.split(':') : [])])
    return Array.from(set).join(':')
  }

  private static ffplayChannelLayout(channels: number): string {
    switch (channels | 0) {
      case 1:
        return 'mono'
      case 2:
        return 'stereo'
      case 4:
        return 'quad'
      case 6:
        return '5.1'
      case 8:
        return '7.1'
      default:
        throw new Error(`[AudioOutput] Unsupported channel count for ffplay: ${channels}`)
    }
  }
}
