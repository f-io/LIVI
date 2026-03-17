import { spawn, ChildProcessWithoutNullStreams } from 'child_process'
import { EventEmitter } from 'events'
import { DEBUG } from '@main/constants'
import { app } from 'electron'
import fs from 'fs'
import path from 'path'
import { decodeTypeMap, type AudioFormat } from '@shared/types'

export default class Microphone extends EventEmitter {
  private process: ChildProcessWithoutNullStreams | null = null
  private currentDecodeType = 5
  private bytesRead = 0
  private chunkSeq = 0

  constructor() {
    super()

    if (DEBUG) {
      console.debug('[Microphone] Init', {
        platform: process.platform
      })
    }
  }

  start(decodeType = 5): void {
    this.stop()

    if (process.platform !== 'darwin') {
      console.error('[Microphone] Only macOS is supported by this build')
      return
    }

    const gstRoot = Microphone.resolveGStreamerRoot()
    if (!gstRoot) {
      console.error('[Microphone] Bundled GStreamer not found')
      return
    }

    const format = Microphone.resolveFormat(decodeType)
    this.currentDecodeType = decodeType

    const cmd = path.join(gstRoot, 'bin', 'gst-launch-1.0')
    const args = [
      'osxaudiosrc',
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

    const env = {
      ...process.env,
      DYLD_LIBRARY_PATH: path.join(gstRoot, 'lib'),
      GST_PLUGIN_SYSTEM_PATH_1_0: path.join(gstRoot, 'lib', 'gstreamer-1.0'),
      GST_PLUGIN_SCANNER: path.join(gstRoot, 'libexec', 'gstreamer-1.0', 'gst-plugin-scanner')
    }

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
      this.cleanup()
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
      this.cleanup()
    })

    if (DEBUG) {
      console.debug('[Microphone] Recording started', {
        ts: Date.now(),
        decodeType: this.currentDecodeType,
        frequency: format.frequency,
        channel: format.channel,
        bitDepth: format.bitDepth,
        format: format.format
      })
    }
  }

  stop(): void {
    if (!this.process) {
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
      this.process.kill()
    } catch (e) {
      if (DEBUG) {
        console.warn('[Microphone] Failed to kill process:', e)
      }
    }

    this.cleanup()
  }

  isCapturing(): boolean {
    return !!this.process
  }

  private cleanup(): void {
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

  private static resolveGStreamerRoot(): string | null {
    const isPackaged = app.isPackaged
    const base = isPackaged ? process.resourcesPath : path.join(app.getAppPath(), 'assets')
    const bundled = path.join(base, 'gstreamer', 'darwin')

    return fs.existsSync(bundled) ? bundled : null
  }

  static getSysdefaultPrettyName(): string {
    return 'system default'
  }
}
