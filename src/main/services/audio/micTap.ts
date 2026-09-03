import { gstAddon, useHostProcess } from '@main/services/video/GstVideo'
import { gstHost } from '@main/services/video/gstHost'

export type MicTapOptions = {
  sampleRate: number
  channels: number
  device?: string
}

/**
 * A microphone capture in the pipeline that streams its samples into a unix
 * socket, in the gst-host on Linux and inside this process elsewhere. The samples
 * never pass through here.
 */
export class MicTap {
  private constructor(
    private readonly hostId: number | null,
    private handle: unknown
  ) {}

  static open(path: string, opts: MicTapOptions): MicTap | null {
    if (useHostProcess) return new MicTap(gstHost.openMicTap(path, opts), null)
    const a = gstAddon()
    if (!a) return null
    const handle = a.openMicTap(path, opts.sampleRate, opts.channels, opts.device)
    return handle ? new MicTap(null, handle) : null
  }

  close(): void {
    if (this.hostId != null) {
      gstHost.closeMicTap(this.hostId)
      return
    }
    if (this.handle) {
      gstAddon()?.closeMicTap(this.handle)
      this.handle = null
    }
  }
}
