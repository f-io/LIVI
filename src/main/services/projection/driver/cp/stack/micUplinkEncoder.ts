/**
 * CpMicUplinkEncoder — the mirror of CpRtpAudioDecoder: encodes captured mic PCM to
 * OPUS via the bundled GStreamer for the CarPlay input (mic) stream. Wireless CarPlay
 * negotiates OPUS for telephony/speech in both directions; the downlink is decoded by
 * CpRtpAudioDecoder, this encodes the uplink.
 *
 *   fdsrc ! rawaudioparse(S16LE) ! audioconvert ! audioresample
 *         ! opusenc ! rtpopuspay ! udpsink 127.0.0.1:<port>
 *
 * gst frames the OPUS stream via rtpopuspay (one OPUS frame per RTP packet on a local
 * UDP port). We strip the 12-byte RTP header off each and emit the raw OPUS frame; the
 * caller wraps it in the CarPlay RTP + ChaCha. gst's RTP timestamp/clock is NOT used —
 * CarPlay clocks the input stream at the negotiated sample rate, not the RTP-OPUS 48k.
 */

import { ChildProcessWithoutNullStreams, spawn } from 'node:child_process'
import dgram from 'node:dgram'
import { EventEmitter } from 'node:events'
import path from 'node:path'
import { gstEnv, resolveGStreamerRoot } from '@main/services/audio/gstreamer'

export interface CpMicEncoderOpts {
  /** Mic capture / OPUS encode sample rate (the negotiated OPUS rate: 16k/24k/48k). */
  sampleRate: number
  channels: number
  /** OPUS target bitrate: 48k (≤24kHz), 64k (≤32kHz), 96k (48kHz) per the R6 spec. */
  bitrate: number
  /** OPUS frame duration in ms (from the phone's framesPerPacket; one of 2.5..60). */
  frameMs: number
  label: string
}

export class CpMicUplinkEncoder extends EventEmitter {
  private _proc: ChildProcessWithoutNullStreams | null = null
  private _sock: dgram.Socket | null = null
  private _port = 0

  constructor(private readonly opts: CpMicEncoderOpts) {
    super()
  }

  async start(): Promise<boolean> {
    const gstRoot = resolveGStreamerRoot()
    if (!gstRoot) {
      console.error(`[cpMicEnc:${this.opts.label}] bundled GStreamer not found`)
      return false
    }
    this._port = await this._freePort()
    const cmd = path.join(
      gstRoot,
      'bin',
      process.platform === 'win32' ? 'gst-launch-1.0.exe' : 'gst-launch-1.0'
    )
    const proc = spawn(cmd, this._buildArgs(this._port), { env: gstEnv(gstRoot), shell: false })
    this._proc = proc
    proc.stderr.on('data', (d: Buffer) => {
      for (const line of d.toString().split('\n')) {
        if (/error|fail|invalid|reject/i.test(line)) {
          console.warn(`[cpMicEnc:${this.opts.label}] ${line.trim()}`)
        }
      }
    })
    proc.on('error', (err) => {
      console.warn(`[cpMicEnc:${this.opts.label}] spawn error: ${err.message}`)
      if (this._proc === proc) this._proc = null
    })
    proc.on('close', () => {
      if (this._proc === proc) this._proc = null
    })

    this._sock = dgram.createSocket('udp4')
    this._sock.on('error', () => {})
    this._sock.on('message', (rtp: Buffer) => {
      // rtpopuspay emits one OPUS frame per RTP packet; drop the 12-byte header.
      if (rtp.length > 12) this.emit('opus', rtp.subarray(12))
    })
    await new Promise<void>((resolve) => {
      this._sock!.bind(this._port, '127.0.0.1', () => resolve())
    })
    return true
  }

  /** Feed captured mic PCM (S16LE at the configured sampleRate) to the encoder. */
  write(pcmLe: Buffer): void {
    this._proc?.stdin.write(pcmLe)
  }

  stop(): void {
    const proc = this._proc
    this._proc = null
    this._sock?.close()
    this._sock = null
    if (!proc) return
    try {
      proc.kill()
    } catch {}
  }

  private _buildArgs(port: number): string[] {
    const { sampleRate, channels, bitrate, frameMs } = this.opts
    return [
      '-q',
      'fdsrc',
      'fd=0',
      '!',
      'rawaudioparse',
      'use-sink-caps=false',
      'format=pcm',
      'pcm-format=s16le',
      `sample-rate=${sampleRate}`,
      `num-channels=${channels}`,
      '!',
      'audioconvert',
      '!',
      'audioresample',
      '!',
      `audio/x-raw,rate=${sampleRate},channels=${channels}`,
      '!',
      'opusenc',
      'audio-type=voice',
      `bitrate=${bitrate}`,
      `frame-size=${frameMs}`,
      '!',
      'rtpopuspay',
      '!',
      'udpsink',
      'host=127.0.0.1',
      `port=${port}`,
      'sync=false',
      'async=false'
    ]
  }

  private _freePort(): Promise<number> {
    return new Promise((resolve, reject) => {
      const s = dgram.createSocket('udp4')
      s.once('error', reject)
      s.bind(0, '127.0.0.1', () => {
        const addr = s.address()
        const port = typeof addr === 'object' && addr ? addr.port : 0
        s.close(() => resolve(port))
      })
    })
  }
}
