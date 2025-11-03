/* eslint-disable no-undef */
'use strict'

const RENDER_QUANTUM_FRAMES = 128
const RING_POINTERS_SIZE = 8

class RingBuffReader {
  constructor(buffer) {
    const storageSize = (buffer.byteLength - RING_POINTERS_SIZE) / Int16Array.BYTES_PER_ELEMENT
    this.storage = new Int16Array(buffer, RING_POINTERS_SIZE, storageSize)
    this.writePointer = new Uint32Array(buffer, 0, 1)
    this.readPointer = new Uint32Array(buffer, 4, 1)
    this.storageLength = this.storage.length
  }
  getAvailable() {
    const rp = Atomics.load(this.readPointer, 0)
    const wp = Atomics.load(this.writePointer, 0)
    return wp >= rp ? wp - rp : this.storageLength - rp + wp
  }
  readTo(target) {
    const rp = Atomics.load(this.readPointer, 0)
    const wp = Atomics.load(this.writePointer, 0)
    const available = wp >= rp ? wp - rp : this.storageLength - rp + wp
    if (available === 0) return 0
    const readLength = Math.min(available, target.length)
    const first = Math.min(this.storageLength - rp, readLength)
    const second = readLength - first
    if (first > 0) target.set(this.storage.subarray(rp, rp + first), 0)
    if (second > 0) target.set(this.storage.subarray(0, second), first)
    Atomics.store(this.readPointer, 0, (rp + readLength) % this.storageLength)
    return readLength
  }
}

class PCMWorkletProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super()
    const p = (options && options.processorOptions) || {}

    this.channels = Math.max(1, p.channels | 0 || 1)
    this.sab = p.sab
    this.reader = new RingBuffReader(this.sab)

    this.readerOutput = new Int16Array(RENDER_QUANTUM_FRAMES * this.channels)
    this.lastOutput = new Float32Array(this.channels).fill(0)

    // rampMs -> rampLength in frames
    const rampMs = typeof p.rampMs === 'number' ? p.rampMs : 8
    this.rampLength = Math.max(1, Math.floor((sampleRate * rampMs) / 1000))
    this.rampSamples = 0

    // simple preroll gate
    const prerollMs = typeof p.prerollMs === 'number' ? p.prerollMs : 0
    this.prerollSamples = Math.max(0, Math.floor((sampleRate * prerollMs) / 1000) * this.channels)
    this.ready = this.prerollSamples === 0

    this.underrunCount = 0
    this.reportedUnderrun = false

    this.port.onmessage = (e) => {
      const msg = e.data || {}
      if (msg.t === 'setRampMs' && typeof msg.ms === 'number' && msg.ms >= 0) {
        this.rampLength = Math.max(1, Math.floor((sampleRate * msg.ms) / 1000))
        this.rampSamples = 0
      }
    }
  }

  toF32(s16) {
    return Math.max(-1.0, Math.min(1.0, s16 / 32768))
  }

  applyFadeIn(output, framesWritten) {
    if (framesWritten === 0 || this.rampSamples >= this.rampLength) return
    const rampFrames = Math.min(framesWritten, this.rampLength - this.rampSamples)
    for (let c = 0; c < output.length; c++) {
      const channel = output[c]
      for (let f = 0; f < rampFrames; f++) {
        const k = (this.rampSamples + f) / this.rampLength
        channel[f] *= k
      }
    }
    this.rampSamples += rampFrames
  }

  handleUnderrun(output, frames) {
    for (let c = 0; c < output.length; c++) output[c].fill(0)
    this.underrunCount++
    this.rampSamples = 0
    if (!this.reportedUnderrun && this.underrunCount > 2) {
      this.port.postMessage({ t: 'underrun', count: this.underrunCount })
      this.reportedUnderrun = true
    }
  }

  process(_inputs, outputs) {
    const out = outputs[0]
    if (!out || out.length === 0) return true

    const frames = RENDER_QUANTUM_FRAMES
    const needSamples = frames * this.channels
    const available = this.reader.getAvailable()

    // preroll gate
    if (!this.ready) {
      if (available >= this.prerollSamples) {
        this.ready = true
      } else {
        for (let c = 0; c < out.length; c++) out[c].fill(0)
        return true
      }
    }

    if (available < needSamples) {
      this.handleUnderrun(out, frames)
      return true
    }

    const read = this.reader.readTo(this.readerOutput)
    const framesRead = Math.floor(read / this.channels)
    if (framesRead === 0) {
      this.handleUnderrun(out, frames)
      return true
    }

    if (this.channels === 2) {
      const L = out[0]
      const R = out[1] || out[0]
      let i = 0
      for (let f = 0; f < framesRead; f++) {
        L[f] = this.toF32(this.readerOutput[i++])
        R[f] = this.toF32(this.readerOutput[i++])
      }
      if (this.rampSamples < this.rampLength) this.applyFadeIn(out, framesRead)
      if (framesRead < frames) {
        const lastL = framesRead > 0 ? L[framesRead - 1] : this.lastOutput[0]
        const lastR = framesRead > 0 ? R[framesRead - 1] : this.lastOutput[1]
        for (let f = framesRead; f < frames; f++) {
          L[f] = lastL
          R[f] = lastR
        }
        this.lastOutput[0] = lastL
        this.lastOutput[1] = lastR
      } else {
        this.lastOutput[0] = L[frames - 1]
        this.lastOutput[1] = R[frames - 1]
      }
      for (let c = 2; c < out.length; c++) out[c].fill(0)
    } else {
      // mono: write M and duplicate to other outs
      const M = out[0]
      for (let f = 0; f < framesRead; f++) M[f] = this.toF32(this.readerOutput[f])
      if (this.rampSamples < this.rampLength) this.applyFadeIn(out, framesRead)
      const lastM = framesRead > 0 ? M[framesRead - 1] : this.lastOutput[0]
      if (framesRead < frames) {
        for (let f = framesRead; f < frames; f++) M[f] = lastM
      }
      // duplicate mono to all additional channels
      for (let c = 1; c < out.length; c++) {
        const C = out[c]
        for (let f = 0; f < framesRead; f++) C[f] = M[f]
        if (framesRead < frames) for (let f = framesRead; f < frames; f++) C[f] = lastM
      }
      this.lastOutput[0] = lastM
    }

    if (framesRead === frames) {
      if (this.reportedUnderrun) {
        this.port.postMessage({ t: 'recovered' })
        this.reportedUnderrun = false
      }
      this.underrunCount = 0
    }

    return true
  }
}

registerProcessor('pcm-worklet-processor', PCMWorkletProcessor)
