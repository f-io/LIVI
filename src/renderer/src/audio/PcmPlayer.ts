import { RingBuffer } from 'ringbuf.js'

const RING_POINTERS_SIZE = 8
const BYTES_PER_SAMPLE = Int16Array.BYTES_PER_ELEMENT
const RENDER_QUANTUM = 128

export class PcmPlayer {
  private workletName = 'pcm-worklet-processor'

  private context: AudioContext | undefined
  private gainNode: GainNode | undefined
  private channels: number
  private worklet: AudioWorkletNode | undefined
  private buffers: Int16Array[] = []
  private rb: RingBuffer
  public readonly sab: SharedArrayBuffer
  private sampleRate: number
  private dropped = 0

  constructor(sampleRate: number, channels: number, expectedJitterMs: number = 10) {
    this.sampleRate = sampleRate
    this.channels = Math.max(1, channels | 0)

    this.sab = this.createJitterBuffer(sampleRate, channels, expectedJitterMs)
    this.rb = new RingBuffer(this.sab, Int16Array)

    this.context = new AudioContext({
      latencyHint: 'balanced', // interactive, balanced, playback (higher latency)
      sampleRate: this.sampleRate
    })
    this.gainNode = this.context.createGain()
    this.gainNode.gain.value = 1.0
    this.gainNode.connect(this.context.destination)
  }

  private createJitterBuffer(
    sr: number,
    ch: number,
    expectedJitterMs = 10,
    packetMs = 60
  ): SharedArrayBuffer {
    const safetyMs = Math.max(120, Math.min(240, expectedJitterMs * 2))
    const targetSamples = Math.ceil((safetyMs / 1000) * sr * ch)

    const packetSamples = Math.ceil((packetMs / 1000) * sr) * ch
    const quantumSamples = RENDER_QUANTUM * ch

    const alignedToPackets = Math.ceil(targetSamples / packetSamples) * packetSamples
    const finalSamples = Math.ceil(alignedToPackets / quantumSamples) * quantumSamples

    const storageBytes = finalSamples * BYTES_PER_SAMPLE
    const totalBytes = RING_POINTERS_SIZE + storageBytes
    const PAGE_SIZE = 4096
    return new SharedArrayBuffer(Math.ceil(totalBytes / PAGE_SIZE) * PAGE_SIZE)
  }

  private feedWorklet(data: Int16Array) {
    if (!this.rb) return 0
    try {
      const n = this.rb.push(data)
      if (n === 0) this.dropped++
      return n
    } catch (error) {
      console.error('[PcmPlayer] Error feeding worklet', error)
      return 0
    }
  }

  getRawBuffer(): SharedArrayBuffer {
    return this.sab
  }

  getStats() {
    return { dropped: this.dropped }
  }

  feed(source: Int16Array) {
    if (!this.worklet) {
      this.buffers.push(source)
      return
    }
    this.feedWorklet(source)
  }

  volume(volume: number, duration: number = 0) {
    if (!this.gainNode || !this.context) {
      console.warn('[PcmPlayer] Audio context not ready for volume change')
      return
    }
    const now = this.context.currentTime
    try {
      this.gainNode.gain.cancelScheduledValues(now)
      if (duration <= 0) {
        this.gainNode.gain.setValueAtTime(volume, now)
      } else {
        this.gainNode.gain.setValueAtTime(this.gainNode.gain.value, now)
        this.gainNode.gain.linearRampToValueAtTime(volume, now + duration / 1000)
      }
    } catch (error) {
      console.error('[PcmPlayer] Error setting volume', error)
    }
  }

  async start() {
    if (!this.context || !this.gainNode) {
      throw new Error('Illegal state - context or gainNode not set')
    }

    const isDev =
      typeof import.meta !== 'undefined' &&
      typeof (import.meta as any).env !== 'undefined' &&
      !!(import.meta as any).env.DEV

    const workletURL = isDev
      ? '/audio.worklet.js'
      : new URL(/* @vite-ignore */ './audio.worklet.js', import.meta.url).href

    try {
      await this.context.audioWorklet.addModule(workletURL)

      this.worklet = new AudioWorkletNode(this.context, this.workletName, {
        numberOfInputs: 0,
        numberOfOutputs: 1,
        outputChannelCount: [this.channels],
        channelCount: this.channels,
        channelCountMode: 'explicit',
        processorOptions: {
          sab: this.sab,
          channels: this.channels,
          streamSampleRate: this.sampleRate,
          prerollMs: 72,
          maxPrerollMs: 180,
          rampMs: 8
        }
      })

      this.worklet.connect(this.gainNode)
      await this.context.resume()

      if (this.buffers.length > 0) {
        for (const source of this.buffers) this.feedWorklet(source)
        this.buffers.length = 0
      }
    } catch (error) {
      console.error('[PcmPlayer] Failed to start', error)
      throw error
    }
  }

  async stop() {
    if (!this.context) return
    try {
      if (this.context.state !== 'closed') {
        await this.context.close()
      }
    } catch (error) {
      console.warn('[PcmPlayer] Error during stop', error)
    } finally {
      this.cleanup()
    }
  }

  private cleanup() {
    this.gainNode?.disconnect()
    this.worklet?.disconnect()
    this.context = undefined
    this.gainNode = undefined
    this.worklet = undefined
    this.buffers.length = 0
    this.dropped = 0
  }

  dispose() {
    this.cleanup()
  }
}
