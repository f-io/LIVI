/**
 * timingSync — CarPlay clock sync on the timing port (UDP, RTCP-style NTP).
 *
 * We (the receiver) drive the sync: send timing requests (type 210) to the phone's
 * timing port with our transmit time in ntpTransmit (offset 24); the phone replies
 * (type 211) echoing it as ntpOriginate (T1) plus its receive (T2, offset 16) and
 * transmit (T3, offset 24) times. From T1..T4 we compute the clock offset and
 * keep the sample with the lowest round-trip time. syncedNtp() then
 * returns our time in the phone's clock domain, which /feedback must use so the
 * phone can place our media-clock position; otherwise the timestamp is meaningless
 * to it. We also answer any request the phone sends (it may sync to us too).
 */

import dgram from 'node:dgram'
import { ntp64Now } from './audioStream'

const PT_REQUEST = 210
const PT_RESPONSE = 211
const REQUEST_INTERVAL_MS = 1000
const TWO32 = 0x100000000

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
  /** Applied clock offset (phone − us), seconds, from the lowest-RTT sample. */
  private _offsetSec = 0
  private _minRtt = Number.POSITIVE_INFINITY
  private _synced = false
  private _loggedBad = false

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
    return ntp64Now() + BigInt(Math.round(this._offsetSec * TWO32))
  }

  private _sendRequest(): void {
    if (!this._sock || !this._peerPort) return
    const pkt = Buffer.alloc(32)
    pkt[0] = 0x80 // version 2
    pkt[1] = PT_REQUEST
    pkt.writeUInt16BE(7, 2) // length in 32-bit words minus 1
    // Our transmit time goes in ntpTransmit (offset 24); the phone echoes it as
    // ntpOriginate. ntpOriginate (offset 8) stays zero.
    writeNtp(pkt, 24, ntp64Now())
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
      writeNtp(resp, 16, ntp64Now()) // T2 receive
      writeNtp(resp, 24, ntp64Now()) // T3 transmit
      this._sock?.send(resp, rinfo.port, rinfo.address)
      return
    }

    if (msg[1] === PT_RESPONSE) {
      // Response to our request. T1 echoed in ntpOriginate, T2 = phone receive,
      // T3 = phone transmit, T4 = our receive. Offset/RTT:
      //   offset = ((T2−T1) + (T3−T4)) / 2 ; rtt = (T4−T1) − (T3−T2)
      const t4 = ntp64Now()
      const t1 = readNtp(msg, 8)
      const t2 = readNtp(msg, 16)
      const t3 = readNtp(msg, 24)
      const offset = (0.5 * (Number(t2 - t1) + Number(t3 - t4))) / TWO32
      const rtt = (Number(t4 - t1) - Number(t3 - t2)) / TWO32
      // Two devices are at most seconds apart; a huge offset means we mis-read the
      // packet fields. Reject it (syncedNtp stays raw so audio still plays) and log
      // the raw T1..T4 once to fix the layout.
      if (Math.abs(offset) > 5) {
        if (!this._loggedBad) {
          this._loggedBad = true
          console.log(
            `[cpTiming] implausible offset=${offset.toFixed(1)}s rtt=${rtt.toFixed(3)}s | t1=${t1.toString(16)} t2=${t2.toString(16)} t3=${t3.toString(16)} t4=${t4.toString(16)} | raw=${msg.subarray(0, 32).toString('hex')}`
          )
        }
        return
      }
      // Keep the offset from the lowest-RTT (least jittered) sample.
      if (rtt >= 0 && rtt < this._minRtt) {
        this._minRtt = rtt
        this._offsetSec = offset
        if (!this._synced) {
          this._synced = true
          console.log(
            `[cpTiming] clock synced: offset=${(offset * 1000).toFixed(1)}ms rtt=${(rtt * 1000).toFixed(1)}ms`
          )
        }
      }
    }
  }
}
