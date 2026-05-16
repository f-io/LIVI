import { DEBUG } from '@main/constants'
import { type AudioFormat, decodeTypeMap } from '@shared/types'
import { ChildProcessWithoutNullStreams, spawn } from 'child_process'
import { EventEmitter } from 'events'
import path from 'path'
import { audioDeviceProp, audioSourceElement, gstEnv, resolveGStreamerRoot } from './gstreamer'

export default class Microphone extends EventEmitter {
  private process: ChildProcessWithoutNullStreams | null = null
  private currentDecodeType = 5
  private bytesRead = 0
  private chunkSeq = 0
  private device: string | undefined

  constructor() {
    super()

    if (DEBUG) {
      console.debug('[Microphone] Init', {
        platform: process.platform
      })
    }
  }

  setDevice(device: string | undefined): void {
    this.device = device
  }

  start(decodeType = 5): void {
    this.stop()

    if (
      process.platform !== 'darwin' &&
      process.platform !== 'linux' &&
      process.platform !== 'win32'
    ) {
      console.error('[Microphone] Unsupported platform')
      return
    }

    const gstRoot = resolveGStreamerRoot()
    if (!gstRoot) {
      console.error('[Microphone] Bundled GStreamer not found')
      return
    }

    const format = Microphone.resolveFormat(decodeType)
    this.currentDecodeType = decodeType

    const cmd = path.join(
      gstRoot,
      'bin',
      process.platform === 'win32' ? 'gst-launch-1.0.exe' : 'gst-launch-1.0'
    )

    const sourceArgs: string[] = [audioSourceElement()]
    if (this.device) sourceArgs.push(`${audioDeviceProp()}=${this.device}`)

    const args = [
      '-q',
      ...sourceArgs,
      '!',
      'queue',
      'max-size-time=20000000', // max 20 ms
      'max-size-bytes=0',
      'max-size-buffers=0',
      'leaky=downstream',
      '!',
      'audioconvert',
      '!',
      'audioresample',
      '!',
      `audio/x-raw,format=${Microphone.toGstRawFormat(format)},rate=${format.frequency},channels=${format.channel}`,
      '!',
      'fdsink',
      'fd=1'
    ]

    const env = gstEnv(gstRoot)

    if (DEBUG) {
      console.debug('[Microphone] Spawning', cmd, args.join(' '))
    }

    this.bytesRead = 0
    this.chunkSeq = 0

    this.process = spawn(cmd, args, {
      env,
      shell: false
    })

    const proc = this.process
    if (!proc) {
      console.error('[Microphone] Failed to spawn recorder process')
      this.cleanup()
      return
    }

    proc.stdout.on('data', (chunk: Buffer) => {
      this.bytesRead += chunk.byteLength
      this.chunkSeq += 1

      if (DEBUG && (this.chunkSeq === 1 || this.chunkSeq % 100 === 0)) {
        console.debug('[Microphone] chunk received', {
          ts: Date.now(),
          decodeType: this.currentDecodeType,
          chunkBytes: chunk.byteLength,
          bytesRead: this.bytesRead,
          seq: this.chunkSeq
        })
      }

      this.emit('data', chunk)
    })

    proc.stderr.on('data', (d: Buffer) => {
      const s = d.toString().trim()
      if (s && DEBUG) {
        console.warn('[Microphone] STDERR:', s)
      }
    })

    proc.on('error', (err) => {
      console.error('[Microphone] process error:', err)
      this.cleanup(proc)
    })

    proc.on('close', (code, signal) => {
      if (DEBUG) {
        console.debug('[Microphone] recorder exited', {
          ts: Date.now(),
          code,
          signal,
          decodeType: this.currentDecodeType,
          bytesRead: this.bytesRead
        })
      }
      this.cleanup(proc)
    })

    if (DEBUG) {
      console.debug('[Microphone] Recording started', {
        ts: Date.now(),
        decodeType: this.currentDecodeType,
        frequency: format.frequency,
        channel: format.channel,
        bitDepth: format.bitDepth,
        format: format.format,
        device:
          process.platform === 'linux'
            ? 'pulse-default'
            : process.platform === 'win32'
              ? 'wasapi-default'
              : 'default'
      })
    }
  }

  stop(): void {
    const proc = this.process

    if (!proc) {
      if (DEBUG) {
        console.debug('[Microphone] No active process to stop')
      }
      return
    }

    if (DEBUG) {
      console.debug('[Microphone] Stopping recording', {
        ts: Date.now(),
        decodeType: this.currentDecodeType,
        bytesRead: this.bytesRead
      })
    }

    try {
      proc.kill()
    } catch (e) {
      if (DEBUG) {
        console.warn('[Microphone] Failed to kill process:', e)
      }
    }

    this.cleanup(proc)
  }

  isCapturing(): boolean {
    return !!this.process
  }

  private cleanup(proc?: ChildProcessWithoutNullStreams | null): void {
    if (proc && this.process !== proc) {
      return
    }

    this.process = null
    this.bytesRead = 0
    this.chunkSeq = 0
  }

  private static resolveFormat(decodeType: number): AudioFormat {
    return (
      decodeTypeMap[decodeType] ?? {
        frequency: 16000,
        channel: 1,
        bitDepth: 16,
        format: 's16le'
      }
    )
  }

  private static toGstRawFormat(format: AudioFormat): string {
    const raw = (format.format ?? 's16le').toLowerCase()

    if (raw === 's16le' || raw === 's16_le') {
      return 'S16LE'
    }

    return raw.toUpperCase()
  }

  static getSysdefaultPrettyName(): string {
    return 'system default'
  }
}
