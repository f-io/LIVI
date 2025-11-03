import { RingBuffer } from 'ringbuf.js'

const RENDER_QUANTUM_FRAMES = 128
const RING_POINTERS_SIZE = 8
const BYTES_PER_SAMPLE = Int16Array.BYTES_PER_ELEMENT
const TARGET_BUFFER_SECONDS = 2.0

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

  constructor(sampleRate: number, channels: number) {
    this.sampleRate = sampleRate
    this.channels = Math.max(1, channels | 0)

    // Allocate SAB for Int16 PCM
    // bytes = pointers(8) + (framesPerBlock * channels * bytesPerSample * blocks)
    const blocks = Math.max(
      64,
      Math.ceil((TARGET_BUFFER_SECONDS * this.sampleRate) / RENDER_QUANTUM_FRAMES)
    )
    const storageBytes = RENDER_QUANTUM_FRAMES * this.channels * BYTES_PER_SAMPLE * blocks
    this.sab = new SharedArrayBuffer(RING_POINTERS_SIZE + storageBytes)

    // Create the ringbuffer
    this.rb = new RingBuffer(this.sab, Int16Array)

    this.context = new AudioContext({
      latencyHint: 'playback',
      sampleRate: this.sampleRate,
    })
    this.gainNode = this.context.createGain()
    this.gainNode.gain.value = 1
    this.gainNode.connect(this.context.destination)
  }

  private feedWorklet(data: Int16Array) {
    this.rb.push(data)
  }

  getRawBuffer() {
    return this.sab
  }

  feed(source: Int16Array) {
    if (!this.worklet) {
      this.buffers.push(source)
      return
    }
    this.feedWorklet(source)
  }

  volume(volume: number, duration: number = 0) {
    if (!this.gainNode || !this.context) return
    const now = this.context.currentTime
    if (duration <= 0) {
      this.gainNode.gain.cancelScheduledValues(now)
      this.gainNode.gain.setValueAtTime(volume, now)
    } else {
      this.gainNode.gain.cancelScheduledValues(now)
      this.gainNode.gain.setTargetAtTime(volume, now, Math.max(0.001, duration / 3))
    }
  }

  async start() {
    if (!this.context || !this.gainNode) {
      throw Error('Illegal state - context or gainNode not set - create a new PcmPlayer')
    }

    const isDev =
      typeof import.meta !== 'undefined' &&
      typeof (import.meta as any).env !== 'undefined' &&
      !!(import.meta as any).env.DEV

    const workletURL = isDev
      ? '/audio.worklet.js'
      : new URL(/* @vite-ignore */ './audio.worklet.js', import.meta.url).href

    await this.context.audioWorklet.addModule(workletURL)

    this.worklet = new AudioWorkletNode(this.context, this.workletName, {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [this.channels],
      processorOptions: {
        sab: this.sab,
        channels: this.channels,
        streamSampleRate: this.sampleRate,
        prerollMs: 24,
        maxPrerollMs: 160,
        rampMs: 8,
      },
    })
    this.worklet.connect(this.gainNode)

    // Flush
    for (const source of this.buffers) this.feedWorklet(source)
    this.buffers.length = 0
  }

  async stop() {
    if (!this.context) return
    try {
      if (this.context.state !== 'closed') {
        await this.context.close()
      }
    } finally {
      this.gainNode?.disconnect()
      this.worklet?.disconnect()
      this.context = undefined
      this.gainNode = undefined
      this.worklet = undefined
    }
  }
}
