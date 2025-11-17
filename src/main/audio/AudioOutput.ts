import { spawn, ChildProcessWithoutNullStreams, execSync } from 'child_process'
import os from 'os'
import fs from 'fs'

export interface AudioOutputOptions {
  sampleRate: number
  channels: number
  device?: string
}

export class AudioOutput {
  private process: ChildProcessWithoutNullStreams | null = null
  private readonly sampleRate: number
  private readonly channels: number
  private readonly device: string

  private bytesWritten = 0

  private queue: Buffer[] = []
  private writing = false

  constructor(opts: AudioOutputOptions) {
    this.sampleRate = opts.sampleRate
    this.channels = Math.max(1, opts.channels | 0)
    this.device = opts.device ?? AudioOutput.resolveDefaultDevice()

    console.debug('[AudioOutput] Init', {
      sampleRate: this.sampleRate,
      channels: this.channels,
      device: this.device,
      platform: os.platform()
    })
  }

  start(): void {
    this.stop()

    let cmd = ''
    let args: string[] = []
    const env = { ...process.env, PATH: AudioOutput.buildExecPath(process.env.PATH) }

    if (os.platform() === 'linux') {
      cmd = 'aplay'
      args = [
        '-D',
        this.device,
        '-f',
        'S16_LE',
        '-c',
        this.channels.toString(),
        '-r',
        this.sampleRate.toString(),
        '-t',
        'raw',
        '-q',
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
        '-' // stdin
      ]
    } else {
      console.error('[AudioOutput] Platform not supported for audio output')
      return
    }

    console.debug('[AudioOutput] Spawning', cmd, args.join(' '))
    this.bytesWritten = 0
    this.queue = []
    this.writing = false

    this.process = spawn(cmd, args, {
      env,
      shell: false
    })

    const proc = this.process
    const stdin = proc.stdin

    stdin.on('error', (err) => {
      console.warn('[AudioOutput] stdin error:', err.message)
    })

    stdin.on('drain', () => {
      this.flushQueue()
    })

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

  private flushQueue() {
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

      if (!ok) {
        if (this.queue.length > 0) {
          console.debug('[AudioOutput] backpressure: waiting for drain, queued', this.queue.length)
        }
        return
      }
    }

    this.writing = false
  }

  write(chunk: Int16Array | Buffer | undefined | null): void {
    const proc = this.process
    if (!proc || !proc.stdin || proc.stdin.destroyed) return
    if (!chunk) return

    let buf: Buffer
    if (Buffer.isBuffer(chunk)) {
      buf = chunk
    } else {
      buf = Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength)
    }

    this.queue.push(buf)
    if (!this.writing) {
      this.flushQueue()
    }
  }

  stop(): void {
    if (!this.process) return

    // console.debug('[AudioOutput] stopping playback')

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

    const candidates = [
      '/opt/homebrew/bin/play', // Apple Silicon
      '/usr/local/bin/play' // Intel
    ]

    for (const p of candidates) {
      if (fs.existsSync(p)) return p
    }

    try {
      const widened = AudioOutput.buildExecPath(process.env.PATH)
      const out = execSync('which play', {
        encoding: 'utf8',
        env: { ...process.env, PATH: widened }
      })
        .toString()
        .trim()
      if (out && fs.existsSync(out)) return out
    } catch {
      // ignore
    }

    return null
  }

  private static buildExecPath(current?: string): string {
    const extra = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin']
    const set = new Set<string>([...extra, ...(current ? current.split(':') : [])])
    return Array.from(set).join(':')
  }

  private static resolveDefaultDevice(): string {
    if (os.platform() === 'linux') return 'default'
    if (os.platform() === 'darwin') return 'default'
    return 'default'
  }
}
