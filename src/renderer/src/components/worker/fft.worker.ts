import FFT from 'fft.js'

// Worker for FFT: init with parameters
const FLOOR_DB = -80
const MIN_FREQ = 20

// A-Weighting
function aWeight(freq: number): number {
  const f2 = freq * freq
  const ra = (f2 + 20.6 ** 2) * (f2 + 12200 ** 2) * Math.sqrt((f2 + 107.7 ** 2) * (f2 + 737.9 ** 2))
  const rb = 12200 ** 2 * f2 * f2
  return rb / ra
}

let fftSize: number
let points: number
let sampleRate: number
let windowFunc: Float32Array
let aWeightTable: Float32Array
let fftInstance: FFT
let fftOutput: number[]
let ringBuffer = new Float32Array(0)

// reusable buffers
let input: Float32Array
let sums: Float32Array
let counts: Uint16Array

// precomputed log-scale constants
let logMin: number
let logMax: number
let logDen: number

self.onmessage = (e: MessageEvent) => {
  const msg = e.data
  if (msg.type === 'init') {
    ;({ fftSize, points, sampleRate } = msg)

    // Hanning window
    windowFunc = new Float32Array(fftSize)
    for (let i = 0; i < fftSize; i++) {
      windowFunc[i] = 0.54 - 0.46 * Math.cos((2 * Math.PI * i) / (fftSize - 1))
    }

    // A-Weighting table
    const A_WEIGHT_1KHZ = aWeight(1000)
    aWeightTable = new Float32Array(fftSize / 2 + 1)
    for (let i = 1; i <= fftSize / 2; i++) {
      const freq = (i * sampleRate) / fftSize
      aWeightTable[i] = Math.sqrt(aWeight(freq) / A_WEIGHT_1KHZ)
    }

    fftInstance = new FFT(fftSize)
    fftOutput = fftInstance.createComplexArray()
    ringBuffer = new Float32Array(0)

    // reusable buffers
    input = new Float32Array(fftSize)
    sums = new Float32Array(points)
    counts = new Uint16Array(points)

    // log scale constants
    logMin = Math.log10(MIN_FREQ)
    logMax = Math.log10(sampleRate / 2)
    logDen = logMax - logMin
  } else if (msg.type === 'pcm' && msg.buffer) {
    // Ringbuffer
    const incoming = new Float32Array(msg.buffer)
    const old = ringBuffer
    ringBuffer = new Float32Array(old.length + incoming.length)
    ringBuffer.set(old)
    ringBuffer.set(incoming, old.length)

    while (ringBuffer.length >= fftSize) {
      const segment = ringBuffer.subarray(0, fftSize)

      // apply window (reuse input buffer)
      for (let i = 0; i < fftSize; i++) {
        input[i] = segment[i] * windowFunc[i]
      }

      // FFT
      fftInstance.realTransform(fftOutput, input)
      fftInstance.completeSpectrum(fftOutput)

      // reuse sums/counts buffers
      sums.fill(0)
      counts.fill(0)

      const half = fftSize / 2

      for (let i = 1; i <= half; i++) {
        const re = fftOutput[2 * i]
        const im = fftOutput[2 * i + 1]
        const mag = Math.hypot(re, im) / (fftSize / 2)
        const wmag = mag * aWeightTable[i]
        const freq = (i * sampleRate) / fftSize
        if (freq < MIN_FREQ || freq > sampleRate / 2) continue
        const pos = (Math.log10(freq) - logMin) / logDen
        const idx = Math.floor(pos * points)
        if (idx >= 0 && idx < points) {
          sums[idx] += wmag
          counts[idx]++
        }
      }

      // dB normalization
      const bins = new Float32Array(points)
      for (let i = 0; i < points; i++) {
        const avg = counts[i] > 0 ? sums[i] / counts[i] : 0
        const db = Math.min(Math.max(20 * Math.log10(avg + 1e-12), FLOOR_DB), 0)
        bins[i] = (db - FLOOR_DB) / -FLOOR_DB
      }

      const buffer = [bins.buffer] as unknown as string // TODO TS workaround. Fix type properly.
      self.postMessage({ type: 'bins', bins }, buffer)

      ringBuffer = ringBuffer.subarray(fftSize)
    }
  }
}
