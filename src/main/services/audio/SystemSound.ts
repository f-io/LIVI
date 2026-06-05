import { AudioOutput } from './AudioOutput'

/** Config subset the system-sound channel needs (read live so volume changes apply instantly). */
type SystemSoundConfig = {
  audioOutputDevice?: string
  disableAudioOutput?: boolean
  systemSoundsVolume?: number
}

const SR = 48000
const CHANNELS = 2
const BLINK_PERIOD_MS = 500 // must match the renderer's useBlink phase
const GEN_INTERVAL_MS = 30
const TEARDOWN_GRACE_MS = 600 // keep the sink warm briefly to avoid respawn churn
const MAX_CATCHUP_FRAMES = SR >> 2 // 250 ms — cap catch-up after a timer stall
const DEFAULT_VOLUME = 0.7

/** Mirror of ProjectionAudio's perceptual volume curve (-60 dB … 0 dB). */
function gainFromVolume(volume: number): number {
  const v = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0))
  if (v <= 0) return 0
  return 10 ** ((-60 + 60 * v) / 20)
}

/**
 * Synthesise a relay "click": several inharmonic housing resonances (the mechanical body) + a
 * short high-passed contact "snap" + a low "thunk" for weight, then a gentle low-pass for warmth.
 * The on-edge ("tick") is brighter/shorter, the off-edge ("tock") is lower/warmer/longer.
 */
export function renderRelayClick(kind: 'on' | 'off'): Float32Array {
  const on = kind === 'on'
  const len = Math.floor((SR * (on ? 34 : 42)) / 1000)
  const out = new Float32Array(len)

  let seed = on ? 0x9e3779b1 : 0x85ebca77
  const noise = () => {
    seed = (seed * 1664525 + 1013904223) >>> 0
    return (seed / 0xffffffff) * 2 - 1
  }

  // Inharmonic resonant modes [freq Hz, decay s, amp] — several read as "mechanical", not a beep.
  const modes: Array<[number, number, number]> = on
    ? [
        [2100, 0.009, 1],
        [3450, 0.006, 0.5],
        [5200, 0.0038, 0.28]
      ]
    : [
        [1300, 0.012, 1],
        [2050, 0.008, 0.46],
        [3050, 0.005, 0.24]
      ]

  // One-pole high-pass state for the crisp contact snap.
  let hpIn = 0
  let hpOut = 0
  const hpCoeff = 0.92

  for (let i = 0; i < len; i++) {
    const t = i / SR
    let s = 0
    for (let m = 0; m < modes.length; m++) {
      const mode = modes[m]!
      s += mode[2] * Math.sin(2 * Math.PI * mode[0] * t) * Math.exp(-t / mode[1])
    }
    s *= 0.5
    // Low thunk for body/weight, kept short so it doesn't muddy.
    s += 0.22 * Math.sin(2 * Math.PI * 175 * t) * Math.exp(-t / 0.004)
    // Crisp snap: high-passed noise, very short.
    const nz = noise() * Math.exp(-t / 0.0016)
    const hp = hpCoeff * (hpOut + nz - hpIn)
    hpIn = nz
    hpOut = hp
    s += 0.5 * hp
    out[i] = s
  }

  // Tiny attack ramp so sample 0 isn't a hard step.
  const atk = Math.max(1, Math.floor(SR * 0.0004))
  for (let i = 0; i < atk; i++) out[i] *= i / atk

  // Warmth: gentle one-pole low-pass over the whole click (smaller = warmer/duller).
  const lp = on ? 0.5 : 0.38
  let lpY = 0
  for (let i = 0; i < len; i++) {
    lpY += lp * (out[i] - lpY)
    out[i] = lpY
  }

  // Peak-normalise so volume is predictable.
  let peak = 0
  for (let i = 0; i < len; i++) peak = Math.max(peak, Math.abs(out[i]))
  if (peak > 0) {
    const norm = 0.85 / peak
    for (let i = 0; i < len; i++) out[i] *= norm
  }
  return out
}

/**
 * Independent "system sounds" audio channel
 */
export class SystemSound {
  private out: AudioOutput | null = null
  private timer: ReturnType<typeof setInterval> | null = null
  private teardownTimer: ReturnType<typeof setTimeout> | null = null
  private active = false

  private streamStartMs = 0
  private framesProduced = 0
  private lastBlinkIndex = 0
  private click: { wave: Float32Array; pos: number } | null = null

  private readonly clickOn = renderRelayClick('on')
  private readonly clickOff = renderRelayClick('off')

  constructor(private readonly getConfig: () => SystemSoundConfig) {}

  /** Turn the relay click on/off, following the blinker telemetry (driven from main). */
  setBlinkerActive(active: boolean): void {
    if (active) {
      if (this.getConfig().disableAudioOutput) {
        this.stop()
        return
      }
      if (this.teardownTimer) {
        clearTimeout(this.teardownTimer)
        this.teardownTimer = null
      }
      this.active = true
      this.ensureRunning()
    } else {
      if (!this.active) return
      this.active = false
      if (!this.out) return
      if (this.teardownTimer) clearTimeout(this.teardownTimer)
      this.teardownTimer = setTimeout(() => this.stop(), TEARDOWN_GRACE_MS)
      this.teardownTimer.unref?.()
    }
  }

  /** Audio output device changed in config: re-open on the new device if currently playing. */
  onDeviceChanged(): void {
    if (!this.out) return
    const wasActive = this.active
    this.stop()
    if (wasActive) this.setBlinkerActive(true)
  }

  dispose(): void {
    this.stop()
  }

  private ensureRunning(): void {
    if (this.out) return
    const cfg = this.getConfig()
    this.out = new AudioOutput({
      sampleRate: SR,
      channels: CHANNELS,
      mode: 'realtime',
      device: cfg.audioOutputDevice || undefined
    })
    this.out.start()
    this.streamStartMs = Date.now()
    this.framesProduced = 0
    this.lastBlinkIndex = Math.floor(this.streamStartMs / BLINK_PERIOD_MS)
    this.click = null
    this.timer = setInterval(() => this.generate(), GEN_INTERVAL_MS)
    this.timer.unref?.()
  }

  private stop(): void {
    if (this.timer) {
      clearInterval(this.timer)
      this.timer = null
    }
    if (this.teardownTimer) {
      clearTimeout(this.teardownTimer)
      this.teardownTimer = null
    }
    if (this.out) {
      try {
        this.out.stop()
      } catch {
        // ignore
      }
      this.out = null
    }
    this.active = false
    this.click = null
  }

  /** Produce exactly the frames that should have played by now (wall-clock paced), stamping a
   *  click at each 500 ms blink boundary while active. */
  private generate(): void {
    const out = this.out
    if (!out) return

    const now = Date.now()
    const targetFrames = Math.floor(((now - this.streamStartMs) / 1000) * SR)
    let n = targetFrames - this.framesProduced
    if (n <= 0) return
    if (n > MAX_CATCHUP_FRAMES) {
      this.framesProduced = targetFrames - MAX_CATCHUP_FRAMES
      n = MAX_CATCHUP_FRAMES
    }

    const gain = gainFromVolume(this.getConfig().systemSoundsVolume ?? DEFAULT_VOLUME)
    const pcm = new Int16Array(n * CHANNELS)

    for (let i = 0; i < n; i++) {
      const frame = this.framesProduced + i
      const tMs = this.streamStartMs + (frame / SR) * 1000
      const bi = Math.floor(tMs / BLINK_PERIOD_MS)
      if (bi !== this.lastBlinkIndex) {
        this.lastBlinkIndex = bi
        if (this.active) this.click = { wave: bi % 2 === 0 ? this.clickOn : this.clickOff, pos: 0 }
      }

      let s = 0
      const c = this.click
      if (c) {
        s = c.wave[c.pos] ?? 0
        c.pos += 1
        if (c.pos >= c.wave.length) this.click = null
      }

      let v = s * gain * 32767
      if (v > 32767) v = 32767
      else if (v < -32768) v = -32768
      const iv = v | 0
      pcm[i * 2] = iv
      pcm[i * 2 + 1] = iv
    }

    this.framesProduced += n
    out.write(pcm)
  }
}
