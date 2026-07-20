/**
 * timingSync — CarPlay clock sync on the timing port (UDP, RTCP-style NTP).
 *
 * We (the receiver) drive the sync: send timing requests (type 210) to the phone's
 * timing port with our transmit time in ntpTransmit (offset 24); the phone replies
 * (type 211) echoing it as ntpOriginate (T1) plus its receive (T2, offset 16) and
 * transmit (T3, offset 24) times. From T1..T4 we compute the clock offset and
 * round-trip time, and steer a local clock onto the phone's domain with them. Every
 * timestamp we put on the wire (T1, and T2/T3 when the phone syncs to us) comes from
 * that steered clock, so the measured offset collapses to near zero once it is locked.
 * The phone's timestamps run on its own monotonic base, so the
 * very first offset is arbitrarily large and simply steps the clock. syncedNtp() returns
 * the steered clock, which /feedback must use so the phone can place our media-clock
 * position.
 */

import dgram from 'node:dgram'
import { ntp64Now } from './audioStream'

const PT_REQUEST = 210
const PT_RESPONSE = 211
const REQUEST_INTERVAL_MS = 1000
const TWO32 = 0x100000000
/** Phase error above which the clock is stepped instead of slewed. */
const STEP_THRESHOLD_SEC = 0.128
/** Fraction of the residual offset applied per accepted sample while locked. */
const SLEW_GAIN = 1 / 8
/** Length of the round-trip-time window a sample must be the minimum of. */
const DELAY_WINDOW = 8
/** Number of responses collected before the lowest-RTT one is processed. */
const PICK_COUNT = 2

function nsToNtp(ns: bigint): bigint {
  const sec = ns / 1_000_000_000n
  const frac = ((ns % 1_000_000_000n) << 32n) / 1_000_000_000n
  return (sec << 32n) | frac
}

/** Offset that puts the steered clock on wall-clock NTP until the first sync lands. */
function wallClockNtpOffsetNs(monoNs: bigint): bigint {
  const ntp = ntp64Now()
  const ns = (ntp >> 32n) * 1_000_000_000n + (((ntp & 0xffffffffn) * 1_000_000_000n) >> 32n)
  return ns - monoNs
}

function writeNtp(buf: Buffer, offset: number, ntp: bigint): void {
  buf.writeUInt32BE(Number(ntp >> 32n) >>> 0, offset)
  buf.writeUInt32BE(Number(ntp & 0xffffffffn) >>> 0, offset + 4)
}

function readNtp(buf: Buffer, offset: number): bigint {
  return (BigInt(buf.readUInt32BE(offset)) << 32n) | BigInt(buf.readUInt32BE(offset + 4))
}

export class TimingSync {
  private _sock: dgram.Socket | null = null
  private _timer: NodeJS.Timeout | null = null
  private _peerHost = ''
  private _peerPort = 0
  /** Nanoseconds added to the monotonic clock, steered onto the phone's clock. */
  private _clockOffsetNs = wallClockNtpOffsetNs(process.hrtime.bigint())
  /** Rolling window of the last round-trip times, in seconds. */
  private readonly _delays = new Array<number>(DELAY_WINDOW).fill(Number.POSITIVE_INFINITY)
  private _delayIndex = 0
  /** Lowest-RTT sample of the current pick group. */
  private _pickCount = PICK_COUNT
  private _pickRtt = Number.POSITIVE_INFINITY
  private _pickOffset = 0
  /** T1 of the request still in flight, so stale or duplicate responses are dropped. */
  private _pendingT1: bigint | null = null
  private _synced = false

  /** Bind a dual-stack UDP port for timing and return it. */
  listen(): Promise<number> {
    return new Promise((resolve, reject) => {
      const sock = dgram.createSocket({ type: 'udp6', ipv6Only: false })
      sock.on('error', reject)
      sock.on('message', (msg, rinfo) => this._onMessage(msg, rinfo))
      sock.bind(0, '::', () => {
        this._sock = sock
        resolve(sock.address().port)
      })
    })
  }

  /** Begin sending periodic timing requests to the phone's timing port. */
  start(peerHost: string, peerPort: number): void {
    this._peerHost = peerHost
    this._peerPort = peerPort
    console.log(`[cpTiming] driving clock sync to ${peerHost}:${peerPort}`)
    this._sendRequest()
    this._timer = setInterval(() => this._sendRequest(), REQUEST_INTERVAL_MS)
  }

  stop(): void {
    if (this._timer) clearInterval(this._timer)
    this._timer = null
    this._sock?.close()
    this._sock = null
  }

  /** Now in the phone's synchronized clock domain, as a 64-bit NTP value. */
  syncedNtp(): bigint {
    return nsToNtp(process.hrtime.bigint() + this._clockOffsetNs)
  }

  private _sendRequest(): void {
    if (!this._sock || !this._peerPort) return
    const pkt = Buffer.alloc(32)
    pkt[0] = 0x80 // version 2
    pkt[1] = PT_REQUEST
    pkt.writeUInt16BE(7, 2) // length in 32-bit words minus 1
    // Our transmit time goes in ntpTransmit (offset 24); the phone echoes it as
    // ntpOriginate. ntpOriginate (offset 8) stays zero.
    const t1 = this.syncedNtp()
    this._pendingT1 = t1
    writeNtp(pkt, 24, t1)
    this._sock.send(pkt, this._peerPort, this._peerHost)
  }

  private _onMessage(msg: Buffer, rinfo: dgram.RemoteInfo): void {
    if (msg.length < 32) return

    if (msg[1] === PT_REQUEST) {
      // The phone syncs to us: echo its transmit (ntpTransmit, offset 24) as our
      // originate (offset 8), then stamp receive (T2) and transmit (T3).
      const resp = Buffer.alloc(32)
      resp[0] = 0x80
      resp[1] = PT_RESPONSE
      resp.writeUInt16BE(7, 2)
      msg.copy(resp, 8, 24, 32) // request ntpTransmit -> response ntpOriginate
      writeNtp(resp, 16, this.syncedNtp()) // T2 receive
      writeNtp(resp, 24, this.syncedNtp()) // T3 transmit
      this._sock?.send(resp, rinfo.port, rinfo.address)
      return
    }

    if (msg[1] === PT_RESPONSE) {
      // Response to our request. T1 echoed in ntpOriginate, T2 = phone receive,
      // T3 = phone transmit, T4 = our receive. Offset/RTT:
      //   offset = ((T2−T1) + (T3−T4)) / 2 ; rtt = (T4−T1) − (T3−T2)
      const t4 = this.syncedNtp()
      const t1 = readNtp(msg, 8)
      const t2 = readNtp(msg, 16)
      const t3 = readNtp(msg, 24)
      // Drop responses that do not answer the request still in flight.
      if (this._pendingT1 === null || t1 !== this._pendingT1) return
      this._pendingT1 = null
      const offset = (0.5 * (Number(t2 - t1) + Number(t3 - t4))) / TWO32
      const rtt = (Number(t4 - t1) - Number(t3 - t2)) / TWO32
      if (!Number.isFinite(offset) || !Number.isFinite(rtt) || rtt < 0) return

      // Collect a group of responses and carry only its lowest-RTT sample forward.
      if (rtt < this._pickRtt) {
        this._pickRtt = rtt
        this._pickOffset = offset
      }
      if (--this._pickCount > 0) return
      const pickedRtt = this._pickRtt
      const pickedOffset = this._pickOffset
      this._pickCount = PICK_COUNT
      this._pickRtt = Number.POSITIVE_INFINITY

      // Use the sample only if it is the least delayed of the recent window.
      const useSample = this._delays.every((d) => pickedRtt <= d)
      this._delays[this._delayIndex] = pickedRtt
      this._delayIndex = (this._delayIndex + 1) % DELAY_WINDOW
      if (!useSample) return

      this._applyOffset(pickedOffset, pickedRtt)
    }
  }

  /** Step the steered clock on a large phase error, otherwise slew it. */
  private _applyOffset(offsetSec: number, rttSec: number): void {
    const stepping = !this._synced || Math.abs(offsetSec) > STEP_THRESHOLD_SEC
    const appliedSec = stepping ? offsetSec : offsetSec * SLEW_GAIN
    this._clockOffsetNs += BigInt(Math.round(appliedSec * 1e9))
    if (stepping) {
      this._delays.fill(Number.POSITIVE_INFINITY)
      this._delayIndex = 0
      this._pendingT1 = null
      console.log(
        `[cpTiming] clock ${this._synced ? 'stepped' : 'synced'}: offset=${offsetSec.toFixed(3)}s rtt=${(rttSec * 1000).toFixed(1)}ms`
      )
      this._synced = true
    }
  }
}
