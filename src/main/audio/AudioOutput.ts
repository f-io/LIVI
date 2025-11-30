import { spawn, ChildProcessWithoutNullStreams, execSync } from 'child_process'
import os from 'os'
import fs from 'fs'

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

    let cmd = ''
    let args: string[] = []
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
        'coreaudio', // explicit CoreAudio sink (prevents early close)
        'default'
      ]
    } else {
      console.error('[AudioOutput] Platform not supported for audio output')
      return
    }

    console.debug('[AudioOutput] Spawning', cmd, args.join(' '))
    this.bytesWritten = 0
    this.queue = []
    this.writing = false

    this.process = spawn(cmd, args, { env, shell: false })

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

  private static buildExecPath(current?: string): string {
    const extra = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin']
    const set = new Set<string>([...extra, ...(current ? current.split(':') : [])])
    return Array.from(set).join(':')
  }
}
