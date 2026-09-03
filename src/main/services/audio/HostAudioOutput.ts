import { gstAddon, useHostProcess } from '@main/services/video/GstVideo'
import { gstHost } from '@main/services/video/gstHost'

export type HostAudioOutputOptions = {
  sampleRate: number
  channels: number
  device?: string
  /** Voice and call streams take the short path to the sink. */
  realtime?: boolean
  /** The stream id, once the stream is open. */
  onOpened?: (streamId: number) => void
}

/** Buffers held while the stream is still opening. */
const PENDING_MAX = 64

/**
 * One audio stream played out by the pipeline: in the gst-host on Linux, inside
 * this process on the other platforms. The samples are handed over as they are.
 */
export class HostAudioOutput {
  private streamId: number | null = null
  private stream: unknown = null
  private opening = false
  private stopped = false
  private readonly pending: Buffer[] = []

  constructor(private readonly opts: HostAudioOutputOptions) {}

  get hostStreamId(): number | null {
    return this.streamId
  }

  start(): void {
    if (this.opening || this.streamId != null) return
    if (!useHostProcess) {
      this.startInProcess()
      return
    }
    this.opening = true
    void gstHost
      .openAudio(Buffer.alloc(32), {
        codec: 'pcm-le',
        payloadType: 0,
        clockRate: this.opts.sampleRate,
        channels: this.opts.channels,
        latencyMs: 0,
        realtime: this.opts.realtime ?? false,
        fed: true,
        device: this.opts.device
      })
      .then(({ streamId }) => {
        this.opening = false
        if (this.stopped) {
          gstHost.closeAudio(streamId)
          return
        }
        this.streamId = streamId
        gstHost.setAudioActive(streamId, true)
        for (const b of this.pending) gstHost.pushAudio(streamId, b)
        this.pending.length = 0
        this.opts.onOpened?.(streamId)
      })
      .catch(() => {
        this.opening = false
      })
  }

  private startInProcess(): void {
    const a = gstAddon()
    if (!a) return
    const stream = a.openAudio(
      this.opts.sampleRate,
      this.opts.channels,
      this.opts.device,
      this.opts.realtime ?? false
    )
    if (!stream) {
      console.warn('[HostAudioOutput] the in-process pipeline could not open the stream')
      return
    }
    this.stream = stream
    this.streamId = a.audioStreamId(stream)
    a.setAudioActive(stream, true)
    for (const b of this.pending) a.pushAudio(stream, b)
    this.pending.length = 0
    this.opts.onOpened?.(this.streamId)
  }

  write(chunk: Int16Array | Buffer | undefined | null): void {
    if (!chunk || this.stopped) return
    const buf = Buffer.isBuffer(chunk)
      ? chunk
      : Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength)

    if (this.streamId == null) {
      if (this.pending.length < PENDING_MAX) this.pending.push(buf)
      return
    }
    if (this.stream) gstAddon()?.pushAudio(this.stream, buf)
    else gstHost.pushAudio(this.streamId, buf)
  }

  /** Sets the level at once, or glides to it over rampMs. */
  setVolume(level: number, rampMs = 0): void {
    if (this.streamId == null) return
    if (this.stream) gstAddon()?.setAudioVolume(this.stream, level, rampMs)
    else gstHost.setAudioVolume(this.streamId, level, rampMs)
  }

  stop(): void {
    this.stopped = true
    this.pending.length = 0
    if (this.streamId == null) return
    if (this.stream) {
      gstAddon()?.closeAudio(this.stream)
      this.stream = null
    } else {
      gstHost.closeAudio(this.streamId)
    }
    this.streamId = null
  }
}
