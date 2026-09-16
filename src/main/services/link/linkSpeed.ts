import { dongleStatus } from '@main/services/link/dongleAp'
import { broadcastToRenderers } from '@main/window/broadcast'

// Polls the dongle's access-point status and reports the CarPlay Wi-Fi link, from the car's
// point of view: down = phone→car (the video/audio stream), up = car→phone (touch, mic).
// PHY rates come straight from the dongle (negotiated station bitrate); throughput is derived
// here from the byte counters between polls, so the sampling clock is the host's.

const POLL_MS = 1500

export type LinkSpeed = {
  /** Live throughput phone→car, Mbps. */
  downMbps: number
  /** Live throughput car→phone, Mbps. */
  upMbps: number
  /** Negotiated PHY bitrate phone→car, Mbps (0 until a phone is on the air). */
  downRate: number
  /** Negotiated PHY bitrate car→phone, Mbps. */
  upRate: number
}

let timer: ReturnType<typeof setInterval> | null = null
let busy = false
let prev: { down: number; up: number; at: number } | null = null

/** Byte delta over a time delta as Mbps, one decimal. Ignores counter resets (negative delta). */
function mbps(deltaBytes: number, deltaMs: number): number {
  if (deltaMs <= 0 || deltaBytes < 0) return 0
  return Math.round(((deltaBytes * 8) / (deltaMs / 1000) / 1e6) * 10) / 10
}

async function sample(): Promise<void> {
  if (busy) return
  busy = true
  try {
    const s = await dongleStatus()
    if (!s) {
      prev = null
      broadcastToRenderers('link-speed', null)
      return
    }
    const down = Number(s.downbytes)
    const up = Number(s.upbytes)
    const now = Date.now()
    const haveBytes = Number.isFinite(down) && Number.isFinite(up)
    let downMbps = 0
    let upMbps = 0
    if (prev && haveBytes) {
      const dt = now - prev.at
      downMbps = mbps(down - prev.down, dt)
      upMbps = mbps(up - prev.up, dt)
    }
    prev = haveBytes ? { down, up, at: now } : null
    const speed: LinkSpeed = {
      downMbps,
      upMbps,
      downRate: Number(s.downrate) || 0,
      upRate: Number(s.uprate) || 0
    }
    broadcastToRenderers('link-speed', speed)
  } finally {
    busy = false
  }
}

export function startLinkSpeedMonitor(): void {
  if (timer) return
  timer = setInterval(() => void sample(), POLL_MS)
  timer.unref?.()
}

export function stopLinkSpeedMonitor(): void {
  if (timer) {
    clearInterval(timer)
    timer = null
  }
  prev = null
}
