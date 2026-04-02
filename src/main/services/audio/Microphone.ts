import { DEBUG } from '@main/constants'
import { type AudioFormat, decodeTypeMap } from '@shared/types'
import { ChildProcessWithoutNullStreams, spawn } from 'child_process'
import { app } from 'electron'
import { EventEmitter } from 'events'
import fs from 'fs'
import path from 'path'

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

    if (
      process.platform !== 'darwin' &&
      process.platform !== 'linux' &&
      process.platform !== 'win32'
    ) {
      console.error('[Microphone] Unsupported platform')
      return
    }

    const gstRoot = Microphone.resolveGStreamerRoot()
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

    const sourceArgs =
      process.platform === 'darwin'
        ? ['osxaudiosrc']
        : process.platform === 'win32'
          ? ['wasapisrc']
          : ['pulsesrc']

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

  static getSysdefaultPrettyName(): string {
    return 'system default'
  }
}
