/**
 * CpRtpAudioDecoder — decodes a CarPlay audio stream (OPUS or AAC-LC) to PCM via the
 * bundled GStreamer, fed over RTP into a jitter buffer. The phone ships audio in
 * bursts (buffered media arrives seconds ahead in ~400ms chunks); feeding the
 * reconstructed RTP through `udpsrc ! rtpjitterbuffer` lets the jitter buffer pace the
 * output back to steady real time. The PCM we hand on (and the FFT taps) is then smooth
 * like the dongle/AA PCM, instead of the raw bursty arrival that underran the sink.
 *
 *   OPUS: udpsrc ! rtpjitterbuffer ! rtpopusdepay ! opusdec ! audioconvert ! fdsink
 *   AAC : udpsrc ! rtpjitterbuffer ! rtpmp4gdepay ! aacparse ! faad ! audioconvert ! fdsink
 *
 * OPUS is standard RTP that rtpopusdepay reads directly. Apple sends AAC as raw access
 * units, so we wrap each one in the RFC 3640 AU-header section that rtpmp4gdepay expects.
 */

import { ChildProcessWithoutNullStreams, spawn } from 'node:child_process'
import dgram from 'node:dgram'
import { EventEmitter } from 'node:events'
import path from 'node:path'
import { gstEnv, resolveGStreamerRoot } from '@main/services/audio/gstreamer'

export interface CpRtpDecoderOpts {
  codec: 'opus' | 'aac-lc'
  /** RTP payload type (the CarPlay stream type) the phone sends on. */
  payloadType: number
  /** RTP clock rate: OPUS is always 48000, AAC is the negotiated sample rate. */
  clockRate: number
  /** Output channels: OPUS mono, AAC stereo. */
  channels: number
  /** AAC/buffered jitter-buffer depth in ms (the phone's negotiated audioLatencyMs).
   *  Must cover how far the phone buffers ahead or the buffer underruns. OPUS ignores
   *  this and stays low-latency. */
  latencyMs?: number
  label: string
}

// MPEG-4 sampling frequency index for the AudioSpecificConfig / RFC 3640 caps.
const AAC_FREQ_INDEX: Record<number, number> = {
  96000: 0,
  88200: 1,
  64000: 2,
  48000: 3,
  44100: 4,
  32000: 5,
  24000: 6,
  22050: 7,
  16000: 8
}

export class CpRtpAudioDecoder extends EventEmitter {
  private _proc: ChildProcessWithoutNullStreams | null = null
  private _sock: dgram.Socket | null = null
  private _port = 0

  constructor(private readonly opts: CpRtpDecoderOpts) {
    super()
  }

  /** Pick a free loopback UDP port, spawn the gst pipeline on it, open the sender. */
  async start(): Promise<boolean> {
    const gstRoot = resolveGStreamerRoot()
    if (!gstRoot) {
      console.error(`[cpRtpDec:${this.opts.label}] bundled GStreamer not found`)
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
    proc.stdout.on('data', (pcm: Buffer) => {
      this.emit('pcm', pcm)
    })
    proc.stderr.on('data', (d: Buffer) => {
      for (const line of d.toString().split('\n')) {
        if (/error|fail|invalid|reject/i.test(line)) {
          console.warn(`[cpRtpDec:${this.opts.label}] ${line.trim()}`)
        }
      }
    })
    proc.on('error', (err) => {
      console.warn(`[cpRtpDec:${this.opts.label}] spawn error: ${err.message}`)
      if (this._proc === proc) this._proc = null
    })
    proc.on('close', () => {
      if (this._proc === proc) this._proc = null
    })

    this._sock = dgram.createSocket('udp4')
    this._sock.on('error', () => {})
    return true
  }

  /** Feed one reconstructed RTP packet (original 12-byte header + decrypted payload). */
  write(rtp: Buffer): void {
    if (!this._sock || !this._port) return
    const pkt = this.opts.codec === 'aac-lc' ? this._reframeAac(rtp) : rtp
    this._sock.send(pkt, this._port, '127.0.0.1')
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

  /**
   * Wrap a raw AAC-LC access unit in the RFC 3640 AU-header section rtpmp4gdepay wants:
   * [12B RTP header][AU-headers-length=16b][AU-header: size<<3 | index=0][AU]. One AU
   * per packet, so the header section is a fixed 4 bytes.
   */
  private _reframeAac(rtp: Buffer): Buffer {
    const header = rtp.subarray(0, 12)
    const au = rtp.subarray(12)
    const auHeader = (au.length << 3) & 0xffff // 13-bit size, 3-bit index (0)
    const section = Buffer.from([0x00, 0x10, (auHeader >> 8) & 0xff, auHeader & 0xff])
    const out = Buffer.concat([header, section, au])
    // Rewrite RTP byte 1 (marker + payload type). Two reasons: rtpjitterbuffer looks up
    // the clock-rate by pt, so it must match the caps (the phone's pt differs from the
    // CarPlay stream type), and rtpmp4gdepay needs the marker bit to flush the access
    // unit (one complete AAC frame per packet) — without it it buffers forever and
    // emits no PCM. The phone sets neither the way we need, so force both.
    out[1] = 0x80 | (this.opts.payloadType & 0x7f)
    return out
  }

  /** Bind a UDP socket to port 0 to learn a free port, then release it for gst. */
  private _freePort(): Promise<number> {
    return new Promise((resolve, reject) => {
      const probe = dgram.createSocket('udp4')
      probe.once('error', reject)
      probe.bind(0, '127.0.0.1', () => {
        const port = probe.address().port
        probe.close(() => resolve(port))
      })
    })
  }

  private _buildArgs(port: number): string[] {
    const { codec, payloadType, clockRate, channels } = this.opts
    const tail = [
      'audioconvert',
      '!',
      `audio/x-raw,format=S16LE,channels=${channels},rate=${clockRate === 44100 ? 44100 : 48000}`,
      '!',
      // clocksync paces the PCM to real time. The phone ships buffered audio ahead in
      // ~400ms bursts; without this the decoder emits those bursts straight through
      // (the jitter buffer alone doesn't pace with a non-syncing fdsink), which underran
      // the sink and made the FFT strobe. clocksync releases each buffer at its
      // running-time so Node (and the FFT tap) see a steady stream.
      'clocksync',
      'sync=true',
      '!',
      'fdsink',
      'fd=1',
      'sync=false'
    ]
    if (codec === 'opus') {
      const caps = `application/x-rtp,media=audio,clock-rate=${clockRate},encoding-name=OPUS,payload=${payloadType}`
      return [
        '-q',
        'udpsrc',
        `port=${port}`,
        `caps=${caps}`,
        '!',
        'rtpjitterbuffer',
        'latency=100',
        '!',
        'rtpopusdepay',
        '!',
        'opusdec',
        '!',
        ...tail
      ]
    }
    // AAC-LC: RFC 3640 MPEG4-GENERIC. config is the 2-byte AudioSpecificConfig
    // (objectType 2 = AAC-LC, freq index, channel config).
    const asc = ((2 << 11) | ((AAC_FREQ_INDEX[clockRate] ?? 3) << 7) | (channels << 3)) & 0xffff
    const config = asc.toString(16).padStart(4, '0')
    // The RFC 3640 fields (config/mode/sizelength/...) are string-typed in GStreamer's
    // caps. Without the explicit (string) hints they parse as ints, the caps never
    // apply, and rtpjitterbuffer then reports "No clock-rate in caps" and drops every
    // packet. clock-rate/payload stay ints.
    const caps =
      `application/x-rtp,media=(string)audio,clock-rate=(int)${clockRate},` +
      `encoding-name=(string)MPEG4-GENERIC,mode=(string)AAC-hbr,config=(string)${config},` +
      `sizelength=(string)13,indexlength=(string)3,indexdeltalength=(string)3,payload=(int)${payloadType}`
    return [
      '-q',
      'udpsrc',
      `port=${port}`,
      `caps=${caps}`,
      '!',
      'rtpjitterbuffer',
      // AAC/music path: buffer to the phone's negotiated audioLatencyMs (default 1000).
      // The phone ships buffered audio that far ahead in bursts,
      // so a shallower buffer underruns. Follows mediaDelay via /info. OPUS/nav stays 100.
      `latency=${Math.max(100, Math.round(this.opts.latencyMs ?? 1000))}`,
      '!',
      'rtpmp4gdepay',
      '!',
      'aacparse',
      '!',
      'faad',
      '!',
      ...tail
    ]
  }
}
