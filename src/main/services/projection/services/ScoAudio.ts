import { MicTap } from '@main/services/audio/micTap'
import { AudioCommand } from '@shared/types/ProjectionEnums'
import { AudioData } from '../messages'

/** Where the helper's call bridge takes the microphone from. */
const MIC_SOCK = '/tmp/aa-sco.mic'
const SCO_AUDIO_TYPE = 2
const SCO_DECODE_TYPE = 3
const SCO_RATE = 8000
const SCO_CHANNELS = 1

/**
 * HFP call audio, the control side. The helper streams the caller into the
 * pipeline's feed as the call stream and takes the microphone from the
 * pipeline's tap. This only says which stream and where the tap goes.
 */
export class ScoAudio {
  private active = false
  private tap: MicTap | null = null
  private unsubscribe: (() => void) | null = null

  constructor(
    private readonly deps: {
      emitAudio: (msg: AudioData) => void
      getMicDevice: () => string | undefined
      /** Opens the call stream in the pipeline, it announces its id through onCallStream. */
      primeCall: () => void
      dropCall: () => void
      onCallStream: (cb: (streamId: number) => void) => () => void
      feedPath: () => Promise<string>
      setScoSink: (feed?: string, streamId?: number) => Promise<unknown>
    }
  ) {}

  start(): void {
    if (this.active) return
    this.active = true
    console.log('[ScoAudio] call audio up')
    this.deps.emitAudio(
      new AudioData({
        audioType: SCO_AUDIO_TYPE,
        decodeType: SCO_DECODE_TYPE,
        command: AudioCommand.AudioPhonecallStart
      })
    )
    this.unsubscribe = this.deps.onCallStream((streamId) => void this.announce(streamId))
    this.deps.primeCall()
    this.tap = MicTap.open(MIC_SOCK, {
      sampleRate: SCO_RATE,
      channels: SCO_CHANNELS,
      device: this.deps.getMicDevice()
    })
    if (!this.tap) console.warn('[ScoAudio] no microphone tap, the caller hears nothing')
  }

  private async announce(streamId: number): Promise<void> {
    const feed = await this.deps.feedPath()
    if (!this.active) return
    if (!feed) {
      console.warn('[ScoAudio] host has no media feed, the caller stays silent')
      return
    }
    try {
      await this.deps.setScoSink(feed, streamId)
    } catch (e) {
      console.warn(`[ScoAudio] helper did not take the call sink: ${(e as Error).message}`)
    }
  }

  stop(): void {
    if (!this.active) return
    this.active = false
    console.log('[ScoAudio] call audio down')
    this.unsubscribe?.()
    this.unsubscribe = null
    this.tap?.close()
    this.tap = null
    void this.deps.setScoSink().catch(() => {})
    this.deps.dropCall()
    this.deps.emitAudio(
      new AudioData({
        audioType: SCO_AUDIO_TYPE,
        decodeType: SCO_DECODE_TYPE,
        command: AudioCommand.AudioPhonecallStop
      })
    )
  }
}
