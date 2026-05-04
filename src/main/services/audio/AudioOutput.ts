import { DEBUG } from '@main/constants'
import { ChildProcessWithoutNullStreams, spawn } from 'child_process'
import { app } from 'electron'
import fs from 'fs'
import path from 'path'

export interface AudioOutputOptions {
  sampleRate: number
  channels: number
  mode?: 'music' | 'realtime'
}

export class AudioOutput {
  private static readonly STOP_GRACE_MS = 500

  private process: ChildProcessWithoutNullStreams | null = null
  private readonly sampleRate: number
  private readonly channels: number
  private readonly mode: 'music' | 'realtime'

  private bytesWritten = 0
  private queue: Buffer[] = []
  private writing = false
  private writeSeq = 0

  constructor(opts: AudioOutputOptions) {
    this.sampleRate = opts.sampleRate
    this.channels = Math.max(1, opts.channels | 0)
    this.mode = opts.mode ?? AudioOutput.inferMode(this.sampleRate, this.channels)

    if (DEBUG) {
      console.debug('[AudioOutput] Init', {
        sampleRate: this.sampleRate,
        channels: this.channels,
        mode: this.mode,
        platform: process.platform
      })
    }
  }

  start(): void {
    // Restart semantics: forcibly tear down any prior process. Don't use the
    // public stop() here — that drains gracefully, which would briefly leave
    // two gst-launch processes alive and racing to mutate `this.process`.
    this.killImmediate()

    if (
      process.platform !== 'darwin' &&
      process.platform !== 'linux' &&
      process.platform !== 'win32'
    ) {
      console.error('[AudioOutput] Unsupported platform')
      return
    }

    const gstRoot = AudioOutput.resolveGStreamerRoot()
    if (!gstRoot) {
      console.error('[AudioOutput] Bundled GStreamer not found')
      return
    }

    const cmd = path.join(
      gstRoot,
      'bin',
      process.platform === 'win32' ? 'gst-launch-1.0.exe' : 'gst-launch-1.0'
    )
    const args = this.buildArgs()

    const pluginPath = path.join(gstRoot, 'lib', 'gstreamer-1.0')
    const pluginScanner = path.join(
      gstRoot,
      'libexec',
      'gstreamer-1.0',
      process.platform === 'win32' ? 'gst-plugin-scanner.exe' : 'gst-plugin-scanner'
    )

    let env: NodeJS.ProcessEnv
    if (process.platform === 'darwin') {
      env = {
        ...process.env,
        DYLD_LIBRARY_PATH: path.join(gstRoot, 'lib'),
        GST_PLUGIN_SYSTEM_PATH: '',
        GST_PLUGIN_PATH: pluginPath,
        GST_PLUGIN_SCANNER: pluginScanner
      }
    } else if (process.platform === 'linux') {
      env = {
        ...process.env,
        LD_LIBRARY_PATH: path.join(gstRoot, 'lib'),
        GST_PLUGIN_SYSTEM_PATH: '',
        GST_PLUGIN_PATH: pluginPath,
        GST_PLUGIN_SCANNER: pluginScanner
      }
    } else {
      env = {
        ...process.env,
        PATH: `${path.join(gstRoot, 'bin')};${process.env.PATH ?? ''}`,
        GST_PLUGIN_SYSTEM_PATH: '',
        GST_PLUGIN_PATH: pluginPath,
        GST_PLUGIN_SCANNER: pluginScanner
      }
    }

    if (DEBUG) {
      console.debug('[AudioOutput] Spawning', cmd, args.join(' '))
    }

    this.bytesWritten = 0
    this.queue = []
    this.writing = false
    this.writeSeq = 0

    this.process = spawn(cmd, args, {
      env,
      shell: false
    })

    const proc = this.process
    const stdin = proc.stdin

    stdin.on('error', (err) => {
      if (DEBUG) {
        console.warn('[AudioOutput] stdin error:', err.message)
      }
    })

    stdin.on('drain', () => {
      if (this.process !== proc) return

      if (DEBUG) {
        console.debug('[AudioOutput] stdin drain', {
          ts: Date.now(),
          mode: this.mode,
          queueLength: this.queue.length,
          bytesWritten: this.bytesWritten
        })
      }

      this.flushQueue()
    })

    proc.stderr.on('data', (d: Buffer) => {
      const s = d.toString().trim()
      if (s && DEBUG) {
        console.warn('[AudioOutput] STDERR:', s)
      }
    })

    proc.on('error', (err) => {
      if (DEBUG) {
        console.error('[AudioOutput] process error:', err)
      }
      if (this.process === proc) {
        this.cleanup()
      }
    })

    proc.on('close', (code, signal) => {
      if (DEBUG) {
        console.debug('[AudioOutput] process exited', {
          ts: Date.now(),
          code,
          signal,
          mode: this.mode,
          bytesWritten: this.bytesWritten
        })
      }

      if (this.process === proc) {
        this.cleanup()
      }
    })

    if (DEBUG) {
      console.debug('[AudioOutput] playback started', {
        ts: Date.now(),
        mode: this.mode
      })
    }
  }

  write(chunk: Int16Array | Buffer | undefined | null): void {
    const proc = this.process
    if (!proc || !proc.stdin || proc.stdin.destroyed) return
    if (!chunk) return

    const buf = Buffer.isBuffer(chunk)
      ? chunk
      : Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength)

    this.queue.push(buf)

    if (DEBUG) {
      this.writeSeq += 1

      if (this.writeSeq === 1 || this.writeSeq % 100 === 0) {
        console.debug('[AudioOutput] write queued', {
          ts: Date.now(),
          mode: this.mode,
          seq: this.writeSeq,
          chunkBytes: buf.byteLength,
          queueLength: this.queue.length
        })
      }
    }

    if (!this.writing) {
      this.flushQueue()
    }
  }

  /**
   * Graceful stop: close stdin so EOS propagates through the gst pipeline,
   * letting pulsesink drain its tail.
   */
  stop(): void {
    if (!this.process) return
    const proc = this.process

    this.endStdin(proc)

    const fallback = setTimeout(() => {
      if (this.process !== proc) return
      try {
        proc.kill()
      } catch (e) {
        if (DEBUG) {
          console.warn('[AudioOutput] failed to kill process:', e)
        }
      }
    }, AudioOutput.STOP_GRACE_MS)
    fallback.unref?.()
  }

  dispose(): void {
    this.killImmediate()
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

      if (!ok) {
        if (DEBUG) {
          console.warn('[AudioOutput] stdin backpressure', {
            ts: Date.now(),
            mode: this.mode,
            queueLength: this.queue.length,
            bytesWritten: this.bytesWritten
          })
        }
        return
      }
    }

    this.writing = false
  }

  private buildArgs(): string[] {
    const isRealtime = this.mode === 'realtime'

    const inputQueueArgs = isRealtime
      ? [
          'queue',
          'max-size-time=40000000', // max 40 ms
          'max-size-bytes=0',
          'max-size-buffers=0'
        ]
      : ['queue', 'max-size-time=200000000', 'max-size-bytes=0', 'max-size-buffers=0'] // max 200ms

    const outputQueueArgs = isRealtime
      ? [
          'queue',
          'max-size-time=20000000', // max 20 ms
          'max-size-bytes=0',
          'max-size-buffers=0'
        ]
      : ['queue', 'max-size-time=100000000', 'max-size-bytes=0', 'max-size-buffers=0'] // max 100ms

    const sink =
      process.platform === 'darwin'
        ? 'osxaudiosink'
        : process.platform === 'win32'
          ? 'wasapisink'
          : 'pulsesink'

    const sinkArgs = isRealtime ? [sink, 'sync=false'] : [sink]

    return [
      'fdsrc',
      'fd=0',
      '!',
      ...inputQueueArgs,
      '!',
      'rawaudioparse',
      'format=pcm',
      'pcm-format=s16le',
      `sample-rate=${this.sampleRate}`,
      `num-channels=${this.channels}`,
      '!',
      'audioconvert',
      '!',
      'audioresample',
      ...(process.platform === 'win32'
        ? []
        : ['!', 'audio/x-raw,format=S16LE,rate=48000,channels=2']),
      '!',
      ...outputQueueArgs,
      '!',
      ...sinkArgs
    ]
  }

  private cleanup(): void {
    this.queue = []
    this.writing = false
    this.process = null
  }

  private killImmediate(): void {
    if (!this.process) return
    const proc = this.process

    this.endStdin(proc)

    try {
      proc.kill()
    } catch (e) {
      if (DEBUG) {
        console.warn('[AudioOutput] failed to kill process:', e)
      }
    }

    this.cleanup()
  }

  private endStdin(proc: ChildProcessWithoutNullStreams): void {
    try {
      if (proc.stdin && !proc.stdin.destroyed) {
        proc.stdin.end()
      }
    } catch (e) {
      if (DEBUG) {
        console.warn('[AudioOutput] failed to end stdin:', e)
      }
    }
  }

  private static inferMode(sampleRate: number, channels: number): 'music' | 'realtime' {
    if (channels === 1) return 'realtime'
    if (sampleRate <= 24000) return 'realtime'
    return 'music'
  }

  private static resolveGStreamerRoot(): string | null {
    const isPackaged = app.isPackaged
    const base = isPackaged ? process.resourcesPath : path.join(app.getAppPath(), 'assets')

    const platformDir =
      process.platform === 'darwin'
        ? process.arch === 'arm64'
          ? 'macos-arm64'
          : null
        : process.platform === 'linux'
          ? process.arch === 'arm64'
            ? 'linux-arm64'
            : process.arch === 'x64'
              ? 'linux-x64'
              : null
          : process.platform === 'win32'
            ? process.arch === 'x64'
              ? 'windows-x64'
              : null
            : null

    if (!platformDir) return null

    const bundled = path.join(base, 'gstreamer', platformDir)
    return fs.existsSync(bundled) ? bundled : null
  }
}
