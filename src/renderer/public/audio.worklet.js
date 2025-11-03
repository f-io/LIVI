 
'use strict'

const RENDER_QUANTUM_FRAMES = 128
const RING_POINTERS_SIZE = 8

// Helpers
function quantaFromMs(ms, sr) {
  return Math.max(1, Math.ceil(((ms / 1000) * sr) / RENDER_QUANTUM_FRAMES))
}

class RingBuffReader {
  constructor(buffer) {
    const storageSize = (buffer.byteLength - RING_POINTERS_SIZE) / Int16Array.BYTES_PER_ELEMENT
    this.storage = new Int16Array(buffer, RING_POINTERS_SIZE, storageSize)
    this.writePointer = new Uint32Array(buffer, 0, 1)
    this.readPointer = new Uint32Array(buffer, 4, 1)
  }

  readTo(target) {
    const info = this.getReadInfo()
    if (info.available === 0) return 0

    const readLength = Math.min(info.available, target.length)
    const first = Math.min(this.storage.length - info.readPos, readLength)
    const second = readLength - first

    target.set(this.storage.subarray(info.readPos, info.readPos + first), 0)
    if (second > 0) target.set(this.storage.subarray(0, second), first)

    Atomics.store(this.readPointer, 0, (info.readPos + readLength) % this.storage.length)
    return readLength
  }

  getReadInfo() {
    const readPos = Atomics.load(this.readPointer, 0)
    const writePos = Atomics.load(this.writePointer, 0)
    const available = (writePos + this.storage.length - readPos) % this.storage.length
    return { readPos, writePos, available }
  }
}

class PCMWorkletProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super()
    const p = (options && options.processorOptions) || {}
    const sab = p.sab
    this.channels = Math.max(1, p.channels | 0 || 1)
    this.reader = new RingBuffReader(sab)
    this.readerOutput = new Int16Array(RENDER_QUANTUM_FRAMES * this.channels)

    // Stream SR / ramp / preroll
    this.streamSR =
      typeof p.streamSampleRate === 'number' && p.streamSampleRate > 0
        ? p.streamSampleRate
        : sampleRate
    this.rampMs = typeof p.rampMs === 'number' && p.rampMs >= 0 ? p.rampMs : 8

    const baseMs = typeof p.prerollMs === 'number' && p.prerollMs > 0 ? p.prerollMs : 8
    const maxMs =
      typeof p.maxPrerollMs === 'number' && p.maxPrerollMs > baseMs ? p.maxPrerollMs : 40

    this.basePrerollQ = quantaFromMs(baseMs, this.streamSR)
    this.targetPrerollQ = this.basePrerollQ
    this.maxPrerollQ = quantaFromMs(maxMs, this.streamSR)

    // State
    this.primed = false
    this.stableBlocks = 0
    this.softUnderruns = 0
    this.hardUnderruns = 0

    // Ramp/clickless
    this.rampLen = 0
    this.rampLeft = 0
    this.needRamp = true
    this.xfFromL = 0
    this.xfFromR = 0
    this.xfFromM = 0
    this.lastL = 0
    this.lastR = 0
    this.lastM = 0

    this.reportedUnderrun = false

    // Runtime tuning
    this.port.onmessage = (e) => {
      const msg = e.data || {}
      if (msg.t === 'setPrerollMs' && typeof msg.ms === 'number' && msg.ms > 0) {
        this.basePrerollQ = quantaFromMs(msg.ms, this.streamSR)
        this.targetPrerollQ = Math.max(this.targetPrerollQ, this.basePrerollQ)
      } else if (msg.t === 'setRampMs' && typeof msg.ms === 'number' && msg.ms >= 0) {
        this.rampMs = msg.ms
        this.needRamp = true
        this.rampLeft = 0
      }
    }
  }

  toF32(s16) {
    return s16 / 32768
  }

  beginRamp() {
    this.rampLen = Math.max(1, Math.floor((this.streamSR * this.rampMs) / 1000))
    this.rampLeft = this.rampLen
    this.xfFromL = this.lastL
    this.xfFromR = this.lastR
    this.xfFromM = this.lastM
    this.needRamp = false
  }

  fillWithLast(out, frames) {
    if (out.length >= 2 && this.channels === 2) {
      const L = out[0],
        R = out[1] || out[0]
      for (let f = 0; f < frames; f++) {
        L[f] = this.lastL
        R[f] = this.lastR
      }
      for (let c = 2; c < out.length; c++) out[c].fill(0)
    } else {
      const M = out[0]
      for (let f = 0; f < frames; f++) M[f] = this.lastM
      for (let c = 1; c < out.length; c++) out[c].fill(this.lastM)
    }
  }

  applyRampStereo(L, R, written) {
    const start = this.rampLen - this.rampLeft
    const n = Math.min(written, this.rampLeft)
    for (let k = 0; k < n; k++) {
      const a = (start + k + 1) / this.rampLen,
        b = 1 - a
      L[k] = b * this.xfFromL + a * L[k]
      R[k] = b * this.xfFromR + a * R[k]
    }
    this.rampLeft -= n
    if (this.rampLeft < 0) this.rampLeft = 0
  }

  applyRampMono(M, written) {
    const start = this.rampLen - this.rampLeft
    const n = Math.min(written, this.rampLeft)
    for (let k = 0; k < n; k++) {
      const a = (start + k + 1) / this.rampLen,
        b = 1 - a
      M[k] = b * this.xfFromM + a * M[k]
    }
    this.rampLeft -= n
    if (this.rampLeft < 0) this.rampLeft = 0
  }

  process(_inputs, outputs) {
    const out = outputs[0]
    const ch = this.channels
    const frames = RENDER_QUANTUM_FRAMES
    const needSamples = frames * ch

    let info = this.reader.getReadInfo()

    // High-water guard: if producer outruns consumer
    // skip just enough to fall back near target preroll, smooth back in via ramp
    const cap = this.reader.storage.length
    const hi = (cap * 0.82) | 0 // ~82% capacity
    const target = this.targetPrerollQ * needSamples
    if (info.available > hi) {
      let toSkip = info.available - Math.max(target, needSamples)
      // align to full frames (channel multiple)
      toSkip = toSkip - (toSkip % ch)
      if (toSkip > 0) {
        const scratch = this.readerOutput // reuse buffer to drain
        while (toSkip > 0) {
          const n = Math.min(toSkip, scratch.length)
          const got = this.reader.readTo(scratch.subarray(0, n))
          if (got <= 0) break
          toSkip -= got
        }
        this.needRamp = true
        info = this.reader.getReadInfo() // refresh after drain
      }
    }

    // Priming with small, adaptive preroll
    if (!this.primed) {
      if (info.available >= this.targetPrerollQ * needSamples) {
        this.primed = true
        this.needRamp = true
        this.stableBlocks = 0
        this.softUnderruns = 0
        this.hardUnderruns = 0
      } else {
        for (let c = 0; c < out.length; c++) out[c].fill(0)
        return true
      }
    }

    // Channel-aligned pull
    const want = Math.min(this.readerOutput.length, info.available)
    const aligned = want - (want % ch)

    if (aligned === 0) {
      this.fillWithLast(out, frames)
      this.needRamp = true
      this.primed = false
      this.hardUnderruns++
      if (this.targetPrerollQ < this.maxPrerollQ) this.targetPrerollQ += 1
      this.stableBlocks = 0
      this.softUnderruns = 0
      if (!this.reportedUnderrun) {
        this.port.postMessage({ t: 'underrun' })
        this.reportedUnderrun = true
      }
      return true
    }

    const got = this.reader.readTo(this.readerOutput.subarray(0, aligned))
    const framesGot = (got / ch) | 0

    if (this.needRamp && this.rampLeft === 0) this.beginRamp()

    if (ch === 2) {
      const L = out[0],
        R = out[1] || out[0]
      let f = 0,
        i = 0
      for (; f < framesGot; f++, i += 2) {
        L[f] = this.toF32(this.readerOutput[i])
        R[f] = this.toF32(this.readerOutput[i + 1])
      }
      if (this.rampLeft > 0) this.applyRampStereo(L, R, f)
      const padL = f ? L[f - 1] : this.lastL
      const padR = f ? R[f - 1] : this.lastR
      for (; f < frames; f++) {
        L[f] = padL
        R[f] = padR
      }
      for (let c = 2; c < out.length; c++) out[c].fill(0)
      this.lastL = L[frames - 1]
      this.lastR = R[frames - 1]
    } else {
      const M = out[0]
      let f = 0
      for (; f < framesGot; f++) M[f] = this.toF32(this.readerOutput[f])
      if (this.rampLeft > 0) this.applyRampMono(M, f)
      const pad = f ? M[f - 1] : this.lastM
      for (; f < frames; f++) M[f] = pad
      for (let c = 1; c < out.length; c++) out[c].fill(0)
      this.lastM = M[frames - 1]
    }

    if (framesGot === frames) {
      this.stableBlocks++
      if (this.stableBlocks >= 128 && this.targetPrerollQ > this.basePrerollQ) {
        this.targetPrerollQ -= 1
        this.stableBlocks = 0
      }
      this.softUnderruns = 0
      if (this.reportedUnderrun) {
        this.port.postMessage({ t: 'recovered' })
        this.reportedUnderrun = false
      }
    } else {
      this.needRamp = true
      this.stableBlocks = 0
      this.softUnderruns++
      if (this.softUnderruns >= 4 && this.targetPrerollQ < this.maxPrerollQ) {
        this.targetPrerollQ += 1
        this.softUnderruns = 0
      }
    }

    return true
  }
}

registerProcessor('pcm-worklet-processor', PCMWorkletProcessor)
